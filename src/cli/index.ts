import { constants as fsConstants } from "node:fs"
import { access, cp, mkdir, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises"
import { createRequire } from "node:module"
import { homedir } from "node:os"
import { dirname, join, relative, resolve, sep } from "node:path"
import { fileURLToPath } from "node:url"
import { Chalk } from "chalk"
import { parseArgs } from "citty"
import type { ArgsDef } from "citty"
import { execa } from "execa"
import stringWidth from "string-width"
import { getBorderCharacters, table } from "table"
import { loadResolvedConfig } from "../config/loader"
import { LIST_TABLE_COLUMNS, type ListTableColumn, type ResolvedConfig, type SelectorCdSurface } from "../config/types"
import {
  DEFAULT_HOOK_TIMEOUT_MS,
  DEFAULT_LOCK_TIMEOUT_MS,
  DEFAULT_STALE_LOCK_TTL_SECONDS,
  EXIT_CODE,
  SCHEMA_VERSION,
  WRITE_COMMANDS,
} from "../core/constants"
import { createCliError, ensureCliError, type CliError } from "../core/errors"
import { invokeHook, runPostHook, runPreHook, type HookExecutionContext } from "../core/hooks"
import { initializeRepository, isInitialized } from "../core/init"
import {
  branchToWorktreePath,
  ensurePathInsideRoot,
  ensurePathInsideRepo,
  getWorktreeRootPath,
  isManagedWorktreePath,
  resolvePathFromCwd,
  resolveRepoContext,
  resolveRepoRelativePath,
} from "../core/paths"
import { readNumberFromEnvOrDefault, withRepoLock } from "../core/repo-lock"
import {
  deleteWorktreeMergeLifecycle,
  moveWorktreeMergeLifecycle,
  upsertWorktreeMergeLifecycle,
} from "../core/worktree-merge-lifecycle"
import { deleteWorktreeLock, readWorktreeLock, upsertWorktreeLock } from "../core/worktree-lock"
import {
  collectWorktreeSnapshot as collectWorktreeSnapshotBase,
  type WorktreeSnapshot,
  type WorktreeStatus,
} from "../core/worktree-state"
import { doesGitRefExist, runGitCommand } from "../git/exec"
import {
  FzfDependencyError,
  FzfInteractiveRequiredError,
  selectPathWithFzf as defaultSelectPathWithFzf,
} from "../integrations/fzf"
import type { SelectPathWithFzfInput, SelectPathWithFzfResult } from "../integrations/fzf"
import { createLogger, LogLevel, type Logger } from "../utils/logger"
import {
  createEarlyRepoCommandHandlers,
  createMiscCommandHandlers,
  createSynchronizationHandlers,
  createWorktreeActionHandlers,
  createWriteCommandHandlers,
  createWriteMutationHandlers,
  dispatchCommandHandler,
} from "./commands/handler-groups"
import { dispatchReadOnlyCommands } from "./commands/read/dispatcher"
import { loadPackageVersion } from "./package-version"
import type { CommandContext } from "./runtime/command-context"

export type CLI = {
  run(args?: string[]): Promise<number>
}

type CLIOptions = {
  readonly version?: string
  readonly cwd?: string
  readonly stdout?: (line: string) => void
  readonly stderr?: (line: string) => void
  readonly selectPathWithFzf?: (input: SelectPathWithFzfInput) => Promise<SelectPathWithFzfResult>
  readonly isInteractive?: () => boolean
}

type OptionValueKind = "boolean" | "value"

type OptionSpec = {
  readonly kind: OptionValueKind
  readonly allowOptionLikeValue: boolean
  readonly allowNegation: boolean
}

type OptionSpecs = {
  readonly longOptions: Map<string, OptionSpec>
  readonly shortOptions: Map<string, OptionSpec>
}

type CommonRuntime = {
  readonly command: string
  readonly json: boolean
  readonly hooksEnabled: boolean
  readonly ghEnabled: boolean
  readonly strictPostHooks: boolean
  readonly hookTimeoutMs: number
  readonly lockTimeoutMs: number
  readonly allowUnsafe: boolean
  readonly isInteractive: boolean
}

type JsonSuccessStatus = "ok" | "created" | "existing" | "deleted"

type JsonSuccess = {
  readonly schemaVersion: number
  readonly command: string
  readonly status: JsonSuccessStatus
  readonly repoRoot: string | null
  readonly [key: string]: unknown
}

type ParsedForceFlags = {
  readonly forceDirty: boolean
  readonly allowUnpushed: boolean
  readonly forceUnmerged: boolean
  readonly forceLocked: boolean
}

type CompletionShell = "zsh" | "fish"

type CommandHelp = {
  readonly name: string
  readonly usage: string
  readonly summary: string
  readonly details: readonly string[]
  readonly options?: readonly string[]
  readonly examples?: readonly string[]
}

type CatppuccinTheme = {
  readonly header: (value: string) => string
  readonly branch: (value: string) => string
  readonly branchCurrent: (value: string) => string
  readonly branchDetached: (value: string) => string
  readonly dirty: (value: string) => string
  readonly clean: (value: string) => string
  readonly merged: (value: string) => string
  readonly unmerged: (value: string) => string
  readonly unknown: (value: string) => string
  readonly base: (value: string) => string
  readonly locked: (value: string) => string
  readonly path: (value: string) => string
  readonly muted: (value: string) => string
  readonly value: (value: string) => string
  readonly previewLabel: (value: string) => string
  readonly previewSection: (value: string) => string
}

const EXIT_CODE_CANCELLED = 130

const optionNamesAllowOptionLikeValue = new Set(["fzfArg", "fzf-arg"])
const CD_FZF_EXTRA_ARGS = [
  "--delimiter=\t",
  "--with-nth=1",
  "--preview=printf '%b' {3}",
  "--preview-window=right,60%,wrap",
  "--ansi",
] as const
const DEFAULT_LIST_TABLE_COLUMNS = [...LIST_TABLE_COLUMNS]
const LIST_TABLE_CELL_HORIZONTAL_PADDING = 2
const COMPLETION_SHELLS: readonly CompletionShell[] = ["zsh", "fish"] as const
const COMPLETION_FILE_BY_SHELL: Readonly<Record<CompletionShell, string>> = {
  zsh: "zsh/_vw",
  fish: "fish/vw.fish",
}

const CATPPUCCIN_MOCHA = {
  rosewater: "#f5e0dc",
  mauve: "#cba6f7",
  red: "#f38ba8",
  peach: "#fab387",
  yellow: "#f9e2af",
  green: "#a6e3a1",
  blue: "#89b4fa",
  lavender: "#b4befe",
  sapphire: "#74c7ec",
  text: "#cdd6f4",
  subtext0: "#a6adc8",
  overlay0: "#6c7086",
} as const

const identityColor = (value: string): string => value

const hasDefaultListColumnOrder = (columns: ReadonlyArray<ListTableColumn>): boolean => {
  if (columns.length !== DEFAULT_LIST_TABLE_COLUMNS.length) {
    return false
  }
  return columns.every((column, index) => column === DEFAULT_LIST_TABLE_COLUMNS[index])
}

const createCatppuccinTheme = ({ enabled }: { readonly enabled: boolean }): CatppuccinTheme => {
  if (enabled !== true) {
    return {
      header: identityColor,
      branch: identityColor,
      branchCurrent: identityColor,
      branchDetached: identityColor,
      dirty: identityColor,
      clean: identityColor,
      merged: identityColor,
      unmerged: identityColor,
      unknown: identityColor,
      base: identityColor,
      locked: identityColor,
      path: identityColor,
      muted: identityColor,
      value: identityColor,
      previewLabel: identityColor,
      previewSection: identityColor,
    }
  }

  const chalk = new Chalk({ level: 3 })
  const color =
    (hex: string) =>
    (value: string): string =>
      chalk.hex(hex)(value)

  return {
    header: color(CATPPUCCIN_MOCHA.rosewater),
    branch: color(CATPPUCCIN_MOCHA.lavender),
    branchCurrent: color(CATPPUCCIN_MOCHA.mauve),
    branchDetached: color(CATPPUCCIN_MOCHA.peach),
    dirty: color(CATPPUCCIN_MOCHA.peach),
    clean: color(CATPPUCCIN_MOCHA.green),
    merged: color(CATPPUCCIN_MOCHA.green),
    unmerged: color(CATPPUCCIN_MOCHA.red),
    unknown: color(CATPPUCCIN_MOCHA.yellow),
    base: color(CATPPUCCIN_MOCHA.blue),
    locked: color(CATPPUCCIN_MOCHA.red),
    path: color(CATPPUCCIN_MOCHA.sapphire),
    muted: color(CATPPUCCIN_MOCHA.overlay0),
    value: color(CATPPUCCIN_MOCHA.text),
    previewLabel: color(CATPPUCCIN_MOCHA.mauve),
    previewSection: color(CATPPUCCIN_MOCHA.rosewater),
  }
}

const shouldUseAnsiColors = ({ interactive }: { readonly interactive: boolean }): boolean => {
  return interactive === true
}

const colorizeCellContent = ({
  cell,
  color,
}: {
  readonly cell: string
  readonly color: (value: string) => string
}): string => {
  const matched = /^(\s*)(.*?)(\s*)$/.exec(cell)
  if (matched === null) {
    return cell
  }
  const leftPadding = matched[1] ?? ""
  const content = matched[2] ?? ""
  const rightPadding = matched[3] ?? ""
  if (content.length === 0) {
    return cell
  }
  return `${leftPadding}${color(content)}${rightPadding}`
}

const colorizeListTableLine = ({ line, theme }: { readonly line: string; readonly theme: CatppuccinTheme }): string => {
  if (line.startsWith("┌") || line.startsWith("├") || line.startsWith("└")) {
    return theme.muted(line)
  }
  if (line.startsWith("│") !== true) {
    return line
  }

  const segments = line.split("│")
  if (segments.length < 3) {
    return line
  }

  const cells = segments.slice(1, -1)
  if (cells.length !== 8) {
    return line
  }

  const headers = cells.map((cell) => cell.trim())
  const isHeaderRow =
    headers[0] === "branch" &&
    headers[1] === "dirty" &&
    headers[2] === "merged" &&
    headers[3] === "pr" &&
    headers[4] === "locked" &&
    headers[5] === "ahead" &&
    headers[6] === "behind" &&
    headers[7] === "path"

  if (isHeaderRow) {
    const nextCells = cells.map((cell) => colorizeCellContent({ cell, color: theme.header }))
    return [segments[0], ...nextCells, segments.at(-1) ?? ""].join("│")
  }

  const branchCell = cells[0] as string
  const dirtyCell = cells[1] as string
  const mergedCell = cells[2] as string
  const prCell = cells[3] as string
  const lockedCell = cells[4] as string
  const aheadCell = cells[5] as string
  const behindCell = cells[6] as string
  const pathCell = cells[7] as string

  const branchColor =
    branchCell.includes("(detached)") === true
      ? theme.branchDetached
      : branchCell.trimStart().startsWith("*")
        ? theme.branchCurrent
        : theme.branch
  const dirtyTrimmed = dirtyCell.trim()
  const dirtyColor = dirtyTrimmed === "dirty" ? theme.dirty : dirtyTrimmed === "clean" ? theme.clean : theme.value
  const mergedTrimmed = mergedCell.trim()
  const mergedColor =
    mergedTrimmed === "merged"
      ? theme.merged
      : mergedTrimmed === "unmerged"
        ? theme.unmerged
        : mergedTrimmed === "-"
          ? theme.base
          : theme.unknown
  const prTrimmed = prCell.trim()
  const prColor =
    prTrimmed === "merged"
      ? theme.merged
      : prTrimmed === "open"
        ? theme.value
        : prTrimmed === "closed_unmerged"
          ? theme.unmerged
          : prTrimmed === "none"
            ? theme.muted
            : prTrimmed === "-"
              ? theme.base
              : theme.unknown
  const lockedTrimmed = lockedCell.trim()
  const lockedColor = lockedTrimmed === "locked" ? theme.locked : theme.muted
  const aheadTrimmed = aheadCell.trim()
  const aheadValue = Number.parseInt(aheadTrimmed, 10)
  const aheadColor =
    aheadTrimmed === "-"
      ? theme.muted
      : Number.isNaN(aheadValue)
        ? theme.value
        : aheadValue > 0
          ? theme.unmerged
          : aheadValue === 0
            ? theme.merged
            : theme.unknown
  const behindTrimmed = behindCell.trim()
  const behindValue = Number.parseInt(behindTrimmed, 10)
  const behindColor =
    behindTrimmed === "-"
      ? theme.muted
      : Number.isNaN(behindValue)
        ? theme.value
        : behindValue > 0
          ? theme.unknown
          : behindValue === 0
            ? theme.merged
            : theme.unknown

  const nextCells = [
    colorizeCellContent({ cell: branchCell, color: branchColor }),
    colorizeCellContent({ cell: dirtyCell, color: dirtyColor }),
    colorizeCellContent({ cell: mergedCell, color: mergedColor }),
    colorizeCellContent({ cell: prCell, color: prColor }),
    colorizeCellContent({ cell: lockedCell, color: lockedColor }),
    colorizeCellContent({ cell: aheadCell, color: aheadColor }),
    colorizeCellContent({ cell: behindCell, color: behindColor }),
    colorizeCellContent({ cell: pathCell, color: theme.path }),
  ]

  return [segments[0], ...nextCells, segments.at(-1) ?? ""].join("│")
}

const colorizeListTable = ({
  rendered,
  theme,
}: {
  readonly rendered: string
  readonly theme: CatppuccinTheme
}): string => {
  return rendered
    .trimEnd()
    .split("\n")
    .map((line) => colorizeListTableLine({ line, theme }))
    .join("\n")
}

const commandHelpEntries: readonly CommandHelp[] = [
  {
    name: "init",
    usage: "vw init",
    summary: "Initialize directories, hooks, and managed exclude entries.",
    details: [
      "Creates .worktree and .vde/worktree directories.",
      "Appends managed entries to .git/info/exclude (idempotent).",
    ],
  },
  {
    name: "list",
    usage: "vw list [--json] [--full-path]",
    summary: "List worktrees with status metadata.",
    details: [
      "Table output includes branch, path, dirty, lock, merged, PR state, and ahead/behind vs base branch.",
      "By default, long path values are truncated to fit terminal width.",
      "JSON output includes PR status/url and upstream metadata fields.",
    ],
    options: ["--full-path"],
  },
  {
    name: "status",
    usage: "vw status [branch] [--json]",
    summary: "Show a single worktree status.",
    details: ["Without branch, resolves from current working directory."],
  },
  {
    name: "path",
    usage: "vw path <branch> [--json]",
    summary: "Print absolute worktree path for the branch.",
    details: [],
  },
  {
    name: "new",
    usage: "vw new [branch]",
    summary: "Create branch + worktree under .worktree.",
    details: ["Without branch, generates wip-xxxxxx."],
  },
  {
    name: "switch",
    usage: "vw switch <branch>",
    summary: "Idempotent branch entrypoint.",
    details: ["Reuses existing worktree when present, otherwise creates one."],
  },
  {
    name: "mv",
    usage: "vw mv <new-branch>",
    summary: "Rename current non-primary worktree branch and move its directory.",
    details: ["Requires branch checkout (detached HEAD is rejected)."],
  },
  {
    name: "del",
    usage: "vw del [branch] [flags]",
    summary: "Delete worktree + branch with safety checks.",
    details: [
      "Default rejects dirty, locked, unmerged/unknown, or unpushed/unknown states.",
      "For non-TTY force usage, --allow-unsafe is required.",
    ],
    options: ["--force-dirty", "--allow-unpushed", "--force-unmerged", "--force-locked", "--force", "--allow-unsafe"],
  },
  {
    name: "gone",
    usage: "vw gone [--json] [--apply|--dry-run]",
    summary: "Bulk cleanup by safety-filtered candidate selection.",
    details: ["Default mode is dry-run. Use --apply to delete candidates."],
  },
  {
    name: "adopt",
    usage: "vw adopt [--json] [--apply|--dry-run]",
    summary: "Move unmanaged non-primary worktrees into managed worktree root.",
    details: ["Default mode is dry-run. Use --apply to move candidates with git worktree move."],
  },
  {
    name: "get",
    usage: "vw get <remote/branch>",
    summary: "Fetch remote branch, create tracking local branch if needed, then attach worktree.",
    details: ["Example target format: origin/feature/foo."],
  },
  {
    name: "extract",
    usage: "vw extract --current [--stash]",
    summary: "Extract current primary branch into .worktree and switch primary back to base.",
    details: ["Current implementation targets primary worktree extraction flow."],
    options: ["--current", "--stash", "--from <path>"],
  },
  {
    name: "absorb",
    usage: "vw absorb <branch> [--from <worktree-name>] [--keep-stash] [--allow-agent --allow-unsafe]",
    summary: "Bring non-primary worktree changes (including uncommitted) into primary worktree.",
    details: [
      "Stashes source worktree changes, checks out target branch in primary, then applies stash.",
      "Non-TTY execution requires --allow-agent and --allow-unsafe.",
    ],
    options: ["--from <worktree-name>", "--keep-stash", "--allow-agent", "--allow-unsafe"],
  },
  {
    name: "unabsorb",
    usage: "vw unabsorb <branch> [--to <worktree-name>] [--keep-stash] [--allow-agent --allow-unsafe]",
    summary: "Push primary worktree changes (including uncommitted) into non-primary worktree.",
    details: [
      "Stashes primary worktree changes, applies them in target non-primary worktree, then optionally drops stash.",
      "Non-TTY execution requires --allow-agent and --allow-unsafe.",
    ],
    options: ["--to <worktree-name>", "--keep-stash", "--allow-agent", "--allow-unsafe"],
  },
  {
    name: "use",
    usage: "vw use <branch> [--allow-shared] [--allow-agent --allow-unsafe]",
    summary: "Checkout target branch in primary worktree.",
    details: [
      "If target branch is attached by another worktree, --allow-shared is required.",
      "Non-TTY execution requires --allow-agent and --allow-unsafe.",
    ],
    options: ["--allow-shared", "--allow-agent", "--allow-unsafe"],
  },
  {
    name: "exec",
    usage: "vw exec <branch> -- <cmd...>",
    summary: "Run command in target branch worktree.",
    details: ["Returns exit code 21 when child process exits non-zero."],
  },
  {
    name: "invoke",
    usage: "vw invoke <pre-*/post-*> [-- <args...>]",
    summary: "Manually run hook script for debugging/operations.",
    details: [],
  },
  {
    name: "copy",
    usage: "vw copy <repo-relative-path...>",
    summary: "Copy repo-root files/dirs to target worktree (typically WT_WORKTREE_PATH).",
    details: [],
  },
  {
    name: "link",
    usage: "vw link <repo-relative-path...> [--no-fallback]",
    summary: "Create symlink from target worktree to repo-root file.",
    details: ["On Windows, fallback copy is used unless --no-fallback is set."],
  },
  {
    name: "lock",
    usage: "vw lock <branch> [--owner <name>] [--reason <text>]",
    summary: "Create/update lock metadata to protect worktree from cleanup/deletion.",
    details: [],
  },
  {
    name: "unlock",
    usage: "vw unlock <branch> [--owner <name>] [--force]",
    summary: "Remove lock metadata with owner/force checks.",
    details: [],
  },
  {
    name: "cd",
    usage: "vw cd",
    summary: "Interactive fzf picker that prints selected worktree absolute path.",
    details: ['Use with shell: cd "$(vw cd)"'],
    options: ["--prompt <text>", "--fzf-arg <arg>"],
  },
  {
    name: "completion",
    usage: "vw completion <zsh|fish> [--install] [--path <file>]",
    summary: "Print or install shell completion scripts.",
    details: [
      "Without --install, prints completion script to stdout.",
      "With --install, writes completion file to default shell path or --path.",
    ],
    options: ["--install", "--path <file>"],
  },
] as const

const commandHelpNames = commandHelpEntries.map((entry) => entry.name)

const splitRawArgsByDoubleDash = (
  args: readonly string[],
): {
  readonly beforeDoubleDash: string[]
  readonly afterDoubleDash: string[]
} => {
  const separatorIndex = args.indexOf("--")
  if (separatorIndex < 0) {
    return {
      beforeDoubleDash: [...args],
      afterDoubleDash: [],
    }
  }
  return {
    beforeDoubleDash: args.slice(0, separatorIndex),
    afterDoubleDash: args.slice(separatorIndex + 1),
  }
}

const toKebabCase = (value: string): string => {
  return value.replace(/[A-Z]/g, (match) => `-${match.toLowerCase()}`)
}

const toOptionSpec = (kind: OptionValueKind, optionName: string): OptionSpec => {
  return {
    kind,
    allowOptionLikeValue: optionNamesAllowOptionLikeValue.has(optionName),
    allowNegation: kind === "boolean",
  }
}

const buildOptionSpecs = (argsDef: Readonly<ArgsDef>): OptionSpecs => {
  const longOptions = new Map<string, OptionSpec>()
  const shortOptions = new Map<string, OptionSpec>()

  for (const [argName, arg] of Object.entries(argsDef)) {
    if (arg.type === "positional") {
      continue
    }

    const valueKind: OptionValueKind = arg.type === "boolean" ? "boolean" : "value"
    const kebabName = toKebabCase(argName)
    longOptions.set(argName, toOptionSpec(valueKind, argName))
    longOptions.set(kebabName, toOptionSpec(valueKind, kebabName))

    const aliases =
      "alias" in arg ? (Array.isArray(arg.alias) ? arg.alias : typeof arg.alias === "string" ? [arg.alias] : []) : []

    for (const alias of aliases) {
      if (alias.length === 1) {
        shortOptions.set(alias, toOptionSpec(valueKind, alias))
        continue
      }

      longOptions.set(alias, toOptionSpec(valueKind, alias))
      const kebabAlias = toKebabCase(alias)
      longOptions.set(kebabAlias, toOptionSpec(valueKind, kebabAlias))
    }
  }

  return { longOptions, shortOptions }
}

const ensureOptionValueToken = ({
  valueToken,
  optionLabel,
  optionSpec,
}: {
  readonly valueToken: string
  readonly optionLabel: string
  readonly optionSpec: OptionSpec
}): void => {
  if (valueToken.length === 0) {
    throw createCliError("INVALID_ARGUMENT", { message: `Missing value for option: ${optionLabel}` })
  }
  if (valueToken.startsWith("-") && optionSpec.allowOptionLikeValue !== true) {
    throw createCliError("INVALID_ARGUMENT", { message: `Missing value for option: ${optionLabel}` })
  }
}

const resolveLongOption = ({
  rawOptionName,
  optionSpecs,
}: {
  readonly rawOptionName: string
  readonly optionSpecs: OptionSpecs
}):
  | {
      readonly optionSpec: OptionSpec
      readonly optionName: string
    }
  | undefined => {
  const directOptionSpec = optionSpecs.longOptions.get(rawOptionName)
  if (directOptionSpec !== undefined) {
    return {
      optionSpec: directOptionSpec,
      optionName: rawOptionName,
    }
  }

  if (rawOptionName.startsWith("no-")) {
    const optionName = rawOptionName.slice(3)
    const negatedOptionSpec = optionSpecs.longOptions.get(optionName)
    if (negatedOptionSpec?.allowNegation === true) {
      return {
        optionSpec: negatedOptionSpec,
        optionName,
      }
    }
  }

  return undefined
}

const validateLongOptionToken = ({
  args,
  index,
  token,
  optionSpecs,
}: {
  readonly args: readonly string[]
  readonly index: number
  readonly token: string
  readonly optionSpecs: OptionSpecs
}): number => {
  const value = token.slice(2)
  if (value.length === 0) {
    return index
  }

  const separatorIndex = value.indexOf("=")
  const rawOptionName = separatorIndex >= 0 ? value.slice(0, separatorIndex) : value
  const resolved = resolveLongOption({
    rawOptionName,
    optionSpecs,
  })
  if (resolved === undefined) {
    throw createCliError("INVALID_ARGUMENT", { message: `Unknown option: --${rawOptionName}` })
  }

  if (resolved.optionSpec.kind !== "value") {
    return index
  }

  if (separatorIndex >= 0) {
    const inlineValue = value.slice(separatorIndex + 1)
    if (inlineValue.length === 0) {
      throw createCliError("INVALID_ARGUMENT", { message: `Missing value for option: --${rawOptionName}` })
    }
    return index
  }

  const nextToken = args[index + 1]
  if (typeof nextToken !== "string") {
    throw createCliError("INVALID_ARGUMENT", { message: `Missing value for option: --${rawOptionName}` })
  }
  ensureOptionValueToken({
    valueToken: nextToken,
    optionLabel: `--${rawOptionName}`,
    optionSpec: resolved.optionSpec,
  })
  return index + 1
}

const validateShortOptionToken = ({
  args,
  index,
  token,
  optionSpecs,
}: {
  readonly args: readonly string[]
  readonly index: number
  readonly token: string
  readonly optionSpecs: OptionSpecs
}): number => {
  const shortFlags = token.slice(1)
  for (let flagIndex = 0; flagIndex < shortFlags.length; flagIndex += 1) {
    const option = shortFlags[flagIndex]
    if (typeof option !== "string" || option.length === 0) {
      continue
    }

    const optionSpec = optionSpecs.shortOptions.get(option)
    if (optionSpec === undefined) {
      throw createCliError("INVALID_ARGUMENT", { message: `Unknown option: -${option}` })
    }
    if (optionSpec.kind !== "value") {
      continue
    }

    if (flagIndex < shortFlags.length - 1) {
      throw createCliError("INVALID_ARGUMENT", {
        message: `Missing value for option: -${option}`,
      })
    }

    const nextToken = args[index + 1]
    if (typeof nextToken !== "string") {
      throw createCliError("INVALID_ARGUMENT", { message: `Missing value for option: -${option}` })
    }
    ensureOptionValueToken({
      valueToken: nextToken,
      optionLabel: `-${option}`,
      optionSpec,
    })
    return index + 1
  }
  return index
}

const validateRawOptions = (args: readonly string[], optionSpecs: OptionSpecs): void => {
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index]
    if (typeof token !== "string") {
      continue
    }
    if (token === "--") {
      break
    }
    if (!token.startsWith("-") || token === "-") {
      continue
    }

    if (token.startsWith("--")) {
      index = validateLongOptionToken({
        args,
        index,
        token,
        optionSpecs,
      })
      continue
    }

    index = validateShortOptionToken({
      args,
      index,
      token,
      optionSpecs,
    })
  }
}

const getPositionals = (args: { readonly _: unknown[] }): string[] => {
  return args._.filter((value): value is string => typeof value === "string")
}

const collectOptionValues = ({
  args,
  optionNames,
}: {
  readonly args: readonly string[]
  readonly optionNames: ReadonlyArray<string>
}): string[] => {
  const values: string[] = []
  const optionNameSet = new Set(optionNames)

  for (let index = 0; index < args.length; index += 1) {
    const token = args[index]
    if (typeof token !== "string") {
      continue
    }

    if (token === "--") {
      break
    }

    if (!token.startsWith("--")) {
      continue
    }

    const eqIndex = token.indexOf("=")
    const rawName = eqIndex >= 0 ? token.slice(2, eqIndex) : token.slice(2)
    if (optionNameSet.has(rawName) !== true) {
      continue
    }

    if (eqIndex >= 0) {
      values.push(token.slice(eqIndex + 1))
      continue
    }

    const nextToken = args[index + 1]
    if (typeof nextToken === "string") {
      values.push(nextToken)
      index += 1
    }
  }

  return values
}

const mergeFzfArgs = ({
  defaults,
  extras,
}: {
  readonly defaults: ReadonlyArray<string>
  readonly extras: ReadonlyArray<string>
}): string[] => {
  const merged = [...defaults]
  for (const arg of extras) {
    if (merged.includes(arg) !== true) {
      merged.push(arg)
    }
  }
  return merged
}

const toNumberOption = ({
  value,
  optionName,
}: {
  readonly value: unknown
  readonly optionName: string
}): number | undefined => {
  if (value === undefined) {
    return undefined
  }
  if (typeof value !== "string") {
    throw createCliError("INVALID_ARGUMENT", {
      message: `${optionName} must be a number`,
    })
  }

  const parsed = Number.parseInt(value, 10)
  if (Number.isFinite(parsed) !== true || parsed <= 0) {
    throw createCliError("INVALID_ARGUMENT", {
      message: `${optionName} must be a positive integer`,
    })
  }
  return parsed
}

const ensureArgumentCount = ({
  command,
  args,
  min,
  max,
}: {
  readonly command: string
  readonly args: readonly string[]
  readonly min: number
  readonly max: number
}): void => {
  if (args.length < min || args.length > max) {
    throw createCliError("INVALID_ARGUMENT", {
      message: `${command} expects ${String(min)}-${String(max)} positional argument(s), received ${String(args.length)}`,
      details: { command, args },
    })
  }
}

const ensureHasCommandAfterDoubleDash = ({
  command,
  argsAfterDoubleDash,
}: {
  readonly command: string
  readonly argsAfterDoubleDash: readonly string[]
}): void => {
  if (argsAfterDoubleDash.length === 0) {
    throw createCliError("INVALID_ARGUMENT", {
      message: `${command} requires arguments after --`,
    })
  }
}

const resolveBaseBranch = async ({
  repoRoot,
  config,
}: {
  readonly repoRoot: string
  readonly config: ResolvedConfig
}): Promise<string> => {
  if (typeof config.git.baseBranch === "string" && config.git.baseBranch.length > 0) {
    return config.git.baseBranch
  }

  const remote = config.git.baseRemote
  const resolved = await runGitCommand({
    cwd: repoRoot,
    args: ["symbolic-ref", "--quiet", "--short", `refs/remotes/${remote}/HEAD`],
    reject: false,
  })
  if (resolved.exitCode === 0) {
    const raw = resolved.stdout.trim()
    const prefix = `${remote}/`
    if (raw.startsWith(prefix)) {
      return raw.slice(prefix.length)
    }
  }

  for (const candidate of ["main", "master"]) {
    if (await doesGitRefExist(repoRoot, `refs/heads/${candidate}`)) {
      return candidate
    }
  }

  throw createCliError("INVALID_ARGUMENT", {
    message: "Unable to resolve base branch from config.yml (baseRemote/HEAD -> main/master).",
    details: {
      remote,
    },
  })
}

const ensureTargetPathWritable = async (targetPath: string): Promise<void> => {
  try {
    await access(targetPath, fsConstants.F_OK)
  } catch {
    await mkdir(dirname(targetPath), { recursive: true })
    return
  }

  const entries = await readdir(targetPath)
  if (entries.length > 0) {
    throw createCliError("TARGET_PATH_NOT_EMPTY", {
      message: `Target path is not empty: ${targetPath}`,
      details: { path: targetPath },
    })
  }
}

const doesPathExist = async (path: string): Promise<boolean> => {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

const buildJsonSuccess = ({
  command,
  status,
  repoRoot,
  details,
}: {
  readonly command: string
  readonly status: JsonSuccessStatus
  readonly repoRoot: string | null
  readonly details?: Record<string, unknown>
}): JsonSuccess => {
  return {
    schemaVersion: SCHEMA_VERSION,
    command,
    status,
    repoRoot,
    ...(details ?? {}),
  }
}

const buildJsonError = ({
  command,
  repoRoot,
  error,
}: {
  readonly command: string
  readonly repoRoot: string | null
  readonly error: CliError
}): Record<string, unknown> => {
  return {
    schemaVersion: SCHEMA_VERSION,
    command,
    status: "error",
    repoRoot,
    code: error.code,
    message: error.message,
    details: error.details,
  }
}

const resolveTargetWorktreeByBranch = ({
  branch,
  worktrees,
}: {
  readonly branch: string
  readonly worktrees: readonly WorktreeStatus[]
}): WorktreeStatus => {
  const found = worktrees.find((worktree) => worktree.branch === branch)
  if (found !== undefined) {
    return found
  }
  throw createCliError("WORKTREE_NOT_FOUND", {
    message: `Worktree not found for branch: ${branch}`,
    details: { branch },
  })
}

const resolveCurrentWorktree = ({
  snapshot,
  currentWorktreeRoot,
}: {
  readonly snapshot: WorktreeSnapshot
  readonly currentWorktreeRoot: string
}): WorktreeStatus => {
  const directMatch = snapshot.worktrees.find((worktree) => worktree.path === currentWorktreeRoot)
  if (directMatch !== undefined) {
    return directMatch
  }
  const containing = snapshot.worktrees.find((worktree) => {
    return currentWorktreeRoot.startsWith(`${worktree.path}${sep}`)
  })
  if (containing !== undefined) {
    return containing
  }
  throw createCliError("WORKTREE_NOT_FOUND", {
    message: "No worktree found for current location",
    details: { currentWorktreeRoot },
  })
}

const validateInitializedForWrite = async (repoRoot: string): Promise<void> => {
  if (await isInitialized(repoRoot)) {
    return
  }
  throw createCliError("NOT_INITIALIZED", {
    message: "Repository is not initialized. Run `vde-worktree init` first.",
    details: { repoRoot },
  })
}

const randomWipBranchName = (): string => {
  const random = Math.floor(Math.random() * 1_000_000)
  return `wip-${String(random).padStart(6, "0")}`
}

const parseForceFlags = (parsedArgs: Record<string, unknown>): ParsedForceFlags => {
  const globalForce = parsedArgs.force === true
  return {
    forceDirty: globalForce || parsedArgs.forceDirty === true,
    allowUnpushed: globalForce || parsedArgs.allowUnpushed === true,
    forceUnmerged: globalForce || parsedArgs.forceUnmerged === true,
    forceLocked: globalForce || parsedArgs.forceLocked === true,
  }
}

const hasAnyForceFlag = (flags: ParsedForceFlags): boolean => {
  return flags.forceDirty || flags.allowUnpushed || flags.forceUnmerged || flags.forceLocked
}

const ensureUnsafeForNonTty = ({
  runtime,
  reason,
}: {
  readonly runtime: CommonRuntime
  readonly reason: string
}): void => {
  if (runtime.isInteractive || runtime.allowUnsafe) {
    return
  }
  throw createCliError("UNSAFE_FLAG_REQUIRED", {
    message: `UNSAFE_FLAG_REQUIRED: ${reason}`,
  })
}

const resolveRemoteAndBranch = (
  remoteBranch: string,
): {
  readonly remote: string
  readonly branch: string
} => {
  const separatorIndex = remoteBranch.indexOf("/")
  if (separatorIndex <= 0 || separatorIndex >= remoteBranch.length - 1) {
    throw createCliError("INVALID_REMOTE_BRANCH_FORMAT", {
      message: `Invalid remote branch format: ${remoteBranch}`,
      details: { value: remoteBranch },
    })
  }
  return {
    remote: remoteBranch.slice(0, separatorIndex),
    branch: remoteBranch.slice(separatorIndex + 1),
  }
}

const resolveCompletionShell = (value: string): CompletionShell => {
  if (COMPLETION_SHELLS.includes(value as CompletionShell)) {
    return value as CompletionShell
  }

  throw createCliError("INVALID_ARGUMENT", {
    message: `Unsupported shell for completion: ${value}`,
    details: {
      value,
      supported: COMPLETION_SHELLS,
    },
  })
}

const resolveCompletionSourceCandidates = (shell: CompletionShell): string[] => {
  const relativeCompletionFile = COMPLETION_FILE_BY_SHELL[shell]
  const moduleDirectory = dirname(fileURLToPath(import.meta.url))
  return [
    resolve(moduleDirectory, "..", "..", "completions", relativeCompletionFile),
    resolve(moduleDirectory, "..", "completions", relativeCompletionFile),
    resolve(process.cwd(), "completions", relativeCompletionFile),
  ]
}

const loadCompletionScript = async (shell: CompletionShell): Promise<string> => {
  const candidates = resolveCompletionSourceCandidates(shell)
  for (const candidate of candidates) {
    try {
      return await readFile(candidate, "utf8")
    } catch {
      continue
    }
  }

  throw createCliError("INTERNAL_ERROR", {
    message: `Completion template not found for shell: ${shell}`,
    details: {
      shell,
      candidates,
    },
  })
}

const resolveDefaultCompletionInstallPath = (shell: CompletionShell): string => {
  const homeDirectory = homedir()
  if (homeDirectory.length === 0) {
    throw createCliError("INTERNAL_ERROR", {
      message: "Unable to resolve home directory for completion installation",
      details: {
        shell,
      },
    })
  }

  if (shell === "zsh") {
    return join(homeDirectory, ".zsh", "completions", "_vw")
  }
  return join(homeDirectory, ".config", "fish", "completions", "vw.fish")
}

const resolveCompletionInstallPath = ({
  shell,
  requestedPath,
}: {
  readonly shell: CompletionShell
  readonly requestedPath?: string
}): string => {
  if (typeof requestedPath === "string" && requestedPath.trim().length > 0) {
    return resolve(requestedPath)
  }
  return resolveDefaultCompletionInstallPath(shell)
}

const installCompletionScript = async ({
  content,
  destinationPath,
}: {
  readonly content: string
  readonly destinationPath: string
}): Promise<void> => {
  await mkdir(dirname(destinationPath), { recursive: true })
  await writeFile(destinationPath, content, "utf8")
}

const normalizeHookName = (value: string): string => {
  if (/^(pre|post)-[a-z0-9][a-z0-9-]*$/.test(value) !== true) {
    throw createCliError("INVALID_ARGUMENT", {
      message: "hookName must be pre-* or post-*",
      details: { hookName: value },
    })
  }
  return value
}

const defaultOwner = (): string => {
  return process.env.USER ?? process.env.USERNAME ?? "unknown"
}

const resolveBranchDeleteMode = (forceFlags: ParsedForceFlags): "-d" | "-D" => {
  if (forceFlags.forceDirty || forceFlags.forceUnmerged || forceFlags.allowUnpushed || forceFlags.forceLocked) {
    return "-D"
  }
  return "-d"
}

const validateDeleteSafety = ({
  target,
  forceFlags,
}: {
  readonly target: WorktreeStatus
  readonly forceFlags: ParsedForceFlags
}): void => {
  if (target.dirty && forceFlags.forceDirty !== true) {
    throw createCliError("DIRTY_WORKTREE", {
      message: "Worktree has uncommitted changes",
      details: { branch: target.branch, path: target.path },
    })
  }
  if (target.locked.value && forceFlags.forceLocked !== true) {
    throw createCliError("LOCKED_WORKTREE", {
      message: "Worktree is locked",
      details: { branch: target.branch, path: target.path, reason: target.locked.reason },
    })
  }
  if (target.merged.overall !== true && forceFlags.forceUnmerged !== true) {
    throw createCliError("UNMERGED_WORKTREE", {
      message: "Worktree is not merged (or merge state is unknown)",
      details: { branch: target.branch, path: target.path, merged: target.merged },
    })
  }
  if ((target.upstream.ahead === null || target.upstream.ahead > 0) && forceFlags.allowUnpushed !== true) {
    throw createCliError("UNPUSHED_WORKTREE", {
      message: "Worktree has unpushed commits (or push state is unknown)",
      details: { branch: target.branch, path: target.path, upstream: target.upstream },
    })
  }
}

const resolveLinkTargetPath = ({
  sourcePath,
  destinationPath,
}: {
  readonly sourcePath: string
  readonly destinationPath: string
}): string => {
  return relative(dirname(destinationPath), sourcePath)
}

const resolveFileCopyTargets = ({
  repoRoot,
  targetWorktreeRoot,
  relativePath,
}: {
  readonly repoRoot: string
  readonly targetWorktreeRoot: string
  readonly relativePath: string
}): {
  readonly sourcePath: string
  readonly destinationPath: string
  readonly relativeFromRoot: string
} => {
  const sourcePath = resolveRepoRelativePath({
    repoRoot,
    relativePath,
  })
  const relativeFromRoot = relative(repoRoot, sourcePath)
  const destinationPath = ensurePathInsideRoot({
    rootPath: targetWorktreeRoot,
    path: resolve(targetWorktreeRoot, relativeFromRoot),
    message: "Path is outside target worktree root",
  })
  return { sourcePath, destinationPath, relativeFromRoot }
}

const resolveTargetWorktreeRootForCopyLink = ({
  repoContext,
  snapshot,
}: {
  readonly repoContext: { currentWorktreeRoot: string }
  readonly snapshot: WorktreeSnapshot
}): string => {
  const rawTarget = process.env.WT_WORKTREE_PATH ?? repoContext.currentWorktreeRoot
  const resolvedTarget = resolvePathFromCwd({
    cwd: repoContext.currentWorktreeRoot,
    path: rawTarget,
  })

  const matched = snapshot.worktrees
    .filter((worktree) => {
      return worktree.path === resolvedTarget || resolvedTarget.startsWith(`${worktree.path}${sep}`)
    })
    .sort((a, b) => b.path.length - a.path.length)[0]

  if (matched === undefined) {
    throw createCliError("WORKTREE_NOT_FOUND", {
      message: "copy/link target worktree not found",
      details: {
        rawTarget,
        resolvedTarget,
      },
    })
  }

  return matched.path
}

const ensureBranchIsNotPrimary = ({
  branch,
  baseBranch,
}: {
  readonly branch: string
  readonly baseBranch: string
}): void => {
  if (branch !== baseBranch) {
    return
  }
  throw createCliError("INVALID_ARGUMENT", {
    message: "extract cannot target the base branch",
    details: { branch, baseBranch },
  })
}

const toManagedWorktreeName = ({
  managedWorktreeRoot,
  worktreePath,
}: {
  readonly managedWorktreeRoot: string
  readonly worktreePath: string
}): string | null => {
  if (
    isManagedWorktreePath({
      worktreePath,
      managedWorktreeRoot,
    }) !== true
  ) {
    return null
  }
  const relativePath = relative(managedWorktreeRoot, worktreePath)
  return relativePath.split(sep).join("/")
}

const resolveManagedWorktreePathFromName = ({
  managedWorktreeRoot,
  optionName,
  worktreeName,
}: {
  readonly managedWorktreeRoot: string
  readonly optionName: "--from" | "--to"
  readonly worktreeName: string
}): string => {
  const normalized = worktreeName.trim()
  if (normalized.length === 0) {
    throw createCliError("INVALID_ARGUMENT", {
      message: `${optionName} requires non-empty worktree name`,
      details: { optionName, worktreeName },
    })
  }

  let resolvedPath: string
  try {
    resolvedPath = resolveRepoRelativePath({
      repoRoot: managedWorktreeRoot,
      relativePath: normalized,
    })
  } catch (error) {
    throw createCliError("INVALID_ARGUMENT", {
      message: `${optionName} expects vw-managed worktree name`,
      details: { optionName, worktreeName },
      cause: error,
    })
  }

  if (resolvedPath === managedWorktreeRoot) {
    throw createCliError("INVALID_ARGUMENT", {
      message: `${optionName} expects vw-managed worktree name`,
      details: { optionName, worktreeName },
    })
  }

  return resolvedPath
}

const resolveManagedNonPrimaryWorktreeByBranch = ({
  repoRoot,
  managedWorktreeRoot,
  branch,
  worktrees,
  optionName,
  worktreeName,
  role,
}: {
  readonly repoRoot: string
  readonly managedWorktreeRoot: string
  readonly branch: string
  readonly worktrees: readonly WorktreeStatus[]
  readonly optionName: "--from" | "--to"
  readonly worktreeName?: string
  readonly role: "source" | "target"
}): WorktreeStatus => {
  const managedCandidates = worktrees.filter((worktree) => {
    return (
      worktree.branch === branch &&
      worktree.path !== repoRoot &&
      toManagedWorktreeName({ managedWorktreeRoot, worktreePath: worktree.path }) !== null
    )
  })

  if (typeof worktreeName === "string") {
    const resolvedPath = resolveManagedWorktreePathFromName({
      managedWorktreeRoot,
      optionName,
      worktreeName,
    })
    const selected = managedCandidates.find((worktree) => worktree.path === resolvedPath)
    if (selected === undefined) {
      throw createCliError("WORKTREE_NOT_FOUND", {
        message: `${role} worktree not found for branch '${branch}' and name '${worktreeName}'`,
        details: { branch, worktreeName, optionName, role },
      })
    }
    return selected
  }

  if (managedCandidates.length === 0) {
    throw createCliError("WORKTREE_NOT_FOUND", {
      message: `No managed ${role} worktree found for branch: ${branch}`,
      details: { branch, role },
    })
  }
  if (managedCandidates.length > 1) {
    throw createCliError("INVALID_ARGUMENT", {
      message: `Multiple managed ${role} worktrees found; use ${optionName} <worktree-name>`,
      details: {
        branch,
        role,
        optionName,
        candidates: managedCandidates.map((worktree) => {
          return toManagedWorktreeName({ managedWorktreeRoot, worktreePath: worktree.path }) ?? worktree.path
        }),
      },
    })
  }
  return managedCandidates[0]!
}

const createStashEntry = async ({
  cwd,
  message,
}: {
  readonly cwd: string
  readonly message: string
}): Promise<string> => {
  await runGitCommand({
    cwd,
    args: ["stash", "push", "-u", "-m", message],
  })
  const stashTop = await runGitCommand({
    cwd,
    args: ["rev-parse", "--verify", "-q", "stash@{0}"],
    reject: false,
  })
  const stashOid = stashTop.stdout.trim()
  if (stashTop.exitCode === 0 && stashOid.length > 0) {
    return stashOid
  }
  throw createCliError("INTERNAL_ERROR", {
    message: "Failed to resolve created stash entry",
    details: { cwd, message },
  })
}

const restoreStashedChanges = async ({
  cwd,
  stashOid,
}: {
  readonly cwd: string
  readonly stashOid: string
}): Promise<void> => {
  const applyResult = await runGitCommand({
    cwd,
    args: ["stash", "apply", stashOid],
    reject: false,
  })
  if (applyResult.exitCode !== 0) {
    throw createCliError("STASH_APPLY_FAILED", {
      message: "Failed to auto-restore stashed changes after pre-hook failure",
      details: { cwd, stashOid },
    })
  }
  await dropStashByOid({ cwd, stashOid })
}

const runPreHookWithAutoRestore = async ({
  name,
  context,
  restore,
}: {
  readonly name: string
  readonly context: HookExecutionContext
  readonly restore?: (() => Promise<void>) | undefined
}): Promise<void> => {
  try {
    await runPreHook({ name, context })
  } catch (error) {
    if (restore !== undefined) {
      try {
        await restore()
      } catch (restoreError) {
        const hookError = ensureCliError(error)
        const restoreCliError = ensureCliError(restoreError)
        throw createCliError(hookError.code, {
          message: `${hookError.message} (auto-restore failed)`,
          details: {
            ...hookError.details,
            autoRestoreFailed: true,
            autoRestoreError: {
              code: restoreCliError.code,
              message: restoreCliError.message,
              details: restoreCliError.details,
            },
          },
          cause: error,
        })
      }
    }
    throw error
  }
}

const resolveStashRefByOid = async ({
  cwd,
  stashOid,
}: {
  readonly cwd: string
  readonly stashOid: string
}): Promise<string | null> => {
  const stashList = await runGitCommand({
    cwd,
    args: ["stash", "list", "--format=%gd%x09%H"],
  })
  const lines = stashList.stdout
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
  for (const line of lines) {
    const [ref, oid] = line.split("\t")
    if (typeof ref === "string" && typeof oid === "string" && ref.length > 0 && oid === stashOid) {
      return ref
    }
  }
  return null
}

const dropStashByOid = async ({
  cwd,
  stashOid,
}: {
  readonly cwd: string
  readonly stashOid: string
}): Promise<void> => {
  const stashRef = await resolveStashRefByOid({ cwd, stashOid })
  if (stashRef === null) {
    return
  }
  await runGitCommand({
    cwd,
    args: ["stash", "drop", stashRef],
  })
}

const formatDisplayPath = (absolutePath: string): string => {
  const homeDirectory = homedir()
  if (homeDirectory.length === 0) {
    return absolutePath
  }
  if (absolutePath === homeDirectory) {
    return "~"
  }
  if (absolutePath.startsWith(`${homeDirectory}${sep}`)) {
    return `~${absolutePath.slice(homeDirectory.length)}`
  }
  return absolutePath
}

const encodeCdPreviewField = (value: string): string => {
  return value
    .replace(/\\/g, "\\\\")
    .split("\u001b")
    .join("\\033")
    .replace(/\t/g, " ")
    .replace(/\r\n?/g, "\n")
    .replace(/\n/g, "\\n")
}

const formatMergedDisplayState = ({
  mergedOverall,
  isBaseBranch,
  baseLabel = "base",
}: {
  readonly mergedOverall: boolean | null
  readonly isBaseBranch: boolean
  readonly baseLabel?: string
}): string => {
  if (isBaseBranch) {
    return baseLabel
  }
  if (mergedOverall === true) {
    return "merged"
  }
  if (mergedOverall === false) {
    return "unmerged"
  }
  return "unknown"
}

const formatMergedColor = ({
  mergedState,
  theme,
}: {
  readonly mergedState: string
  readonly theme: CatppuccinTheme
}): string => {
  const normalized = mergedState.toLowerCase()
  if (normalized === "merged") {
    return theme.merged(mergedState)
  }
  if (normalized === "unmerged") {
    return theme.unmerged(mergedState)
  }
  if (normalized === "base") {
    return theme.base(mergedState)
  }
  return theme.unknown(mergedState)
}

const formatPrDisplayState = ({
  prStatus,
  isBaseBranch,
}: {
  readonly prStatus: WorktreeStatus["pr"]["status"]
  readonly isBaseBranch: boolean
}): string => {
  if (isBaseBranch || prStatus === null) {
    return "-"
  }
  if (
    prStatus === "none" ||
    prStatus === "open" ||
    prStatus === "merged" ||
    prStatus === "closed_unmerged" ||
    prStatus === "unknown"
  ) {
    return prStatus
  }
  return "unknown"
}

const formatListUpstreamCount = (value: number | null): string => {
  if (value === null) {
    return "-"
  }
  return String(value)
}

const resolveListColumnContentWidth = ({
  rows,
  columnIndex,
}: {
  readonly rows: readonly (readonly string[])[]
  readonly columnIndex: number
}): number => {
  return rows.reduce((width, row) => {
    const cell = row[columnIndex] ?? ""
    return Math.max(width, stringWidth(cell))
  }, 0)
}

const resolveListPathColumnWidth = ({
  rows,
  columns,
  truncateMode,
  fullPath,
  minWidth,
}: {
  readonly rows: readonly (readonly string[])[]
  readonly columns: ReadonlyArray<ListTableColumn>
  readonly truncateMode: "auto" | "never"
  readonly fullPath: boolean
  readonly minWidth: number
}): number | null => {
  const pathColumnIndex = columns.indexOf("path")
  if (pathColumnIndex < 0) {
    return null
  }
  if (fullPath || truncateMode === "never") {
    return null
  }
  if (process.stdout.isTTY !== true) {
    return null
  }
  const terminalColumns = process.stdout.columns
  if (typeof terminalColumns !== "number" || Number.isFinite(terminalColumns) !== true || terminalColumns <= 0) {
    return null
  }

  const measuredNonPathWidth = columns
    .map((_, index) => {
      if (index === pathColumnIndex) {
        return 0
      }
      return resolveListColumnContentWidth({ rows, columnIndex: index })
    })
    .reduce((sum, width) => sum + width, 0)
  const borderWidth = columns.length + 1
  const paddingWidth = columns.length * LIST_TABLE_CELL_HORIZONTAL_PADDING
  const availablePathWidth = Math.floor(terminalColumns) - borderWidth - paddingWidth - measuredNonPathWidth

  return Math.max(minWidth, availablePathWidth)
}

const resolveAheadBehindAgainstBaseBranch = async ({
  repoRoot,
  baseBranch,
  worktree,
}: {
  readonly repoRoot: string
  readonly baseBranch: string | null
  readonly worktree: WorktreeStatus
}): Promise<{
  readonly ahead: number | null
  readonly behind: number | null
}> => {
  if (baseBranch === null) {
    return { ahead: null, behind: null }
  }

  const targetRef = worktree.branch ?? worktree.head
  const distance = await runGitCommand({
    cwd: repoRoot,
    args: ["rev-list", "--left-right", "--count", `${baseBranch}...${targetRef}`],
    reject: false,
  })

  if (distance.exitCode !== 0) {
    return { ahead: null, behind: null }
  }

  const [behindRaw, aheadRaw] = distance.stdout.trim().split(/\s+/)
  const behind = Number.parseInt(behindRaw ?? "", 10)
  const ahead = Number.parseInt(aheadRaw ?? "", 10)

  return {
    ahead: Number.isNaN(ahead) ? null : ahead,
    behind: Number.isNaN(behind) ? null : behind,
  }
}

const padToDisplayWidth = ({ value, width }: { readonly value: string; readonly width: number }): string => {
  const visibleLength = stringWidth(value)
  if (visibleLength >= width) {
    return value
  }
  return `${value}${" ".repeat(width - visibleLength)}`
}

const buildCdBranchLabel = ({
  worktree,
  currentWorktreeRoot,
}: {
  readonly worktree: WorktreeStatus
  readonly currentWorktreeRoot: string
}): string => {
  const isCurrent = worktree.path === currentWorktreeRoot
  return `${isCurrent ? "*" : " "} ${worktree.branch ?? "(detached)"}`
}

const buildCdStateSummary = ({
  worktree,
  isBaseBranch,
  theme,
}: {
  readonly worktree: WorktreeStatus
  readonly isBaseBranch: boolean
  readonly theme: CatppuccinTheme
}): string => {
  const dirtyLabel = worktree.dirty ? "DIRTY" : "CLEAN"
  const mergedLabel = formatMergedDisplayState({
    mergedOverall: worktree.merged.overall,
    isBaseBranch,
  }).toUpperCase()
  const lockLabel = worktree.locked.value ? "LOCK" : "OPEN"

  const dirtyBadge = (worktree.dirty ? theme.unmerged : theme.clean)(
    padToDisplayWidth({
      value: dirtyLabel,
      width: 5,
    }),
  )
  const mergedBadge = formatMergedColor({
    mergedState: padToDisplayWidth({
      value: mergedLabel,
      width: 8,
    }),
    theme,
  })
  const lockBadge = (worktree.locked.value ? theme.locked : theme.muted)(
    padToDisplayWidth({
      value: lockLabel,
      width: 4,
    }),
  )

  return `${dirtyBadge} ${theme.muted("|")} ${mergedBadge} ${theme.muted("|")} ${lockBadge}`
}

const buildCdPreviewText = ({
  worktree,
  baseBranch,
  theme,
}: {
  readonly worktree: WorktreeStatus
  readonly baseBranch: string | null
  readonly theme: CatppuccinTheme
}): string => {
  const isBaseBranch = typeof worktree.branch === "string" && baseBranch !== null && worktree.branch === baseBranch
  const branchLabel =
    worktree.branch === null
      ? theme.branchDetached("(detached)")
      : isBaseBranch
        ? theme.base(worktree.branch)
        : theme.branch(worktree.branch)
  const pathLabel = theme.path(formatDisplayPath(worktree.path))
  const dirtyValue = worktree.dirty ? theme.unmerged("[DIRTY]") : theme.merged("[CLEAN]")
  const lockedValue = worktree.locked.value ? theme.locked("[LOCKED]") : theme.clean("[OPEN]")
  const mergedState = formatMergedDisplayState({
    mergedOverall: worktree.merged.overall,
    isBaseBranch,
  })
  const mergedValue = formatMergedColor({
    mergedState: mergedState.toUpperCase(),
    theme,
  })
  const remoteValue =
    worktree.upstream.remote === null ? theme.muted("none") : theme.value(worktree.upstream.remote ?? "none")
  const aheadValue =
    worktree.upstream.ahead === null
      ? theme.unknown("UNKNOWN")
      : worktree.upstream.ahead > 0
        ? theme.unmerged(String(worktree.upstream.ahead))
        : theme.merged("0")
  const behindValue =
    worktree.upstream.behind === null
      ? theme.unknown("UNKNOWN")
      : worktree.upstream.behind > 0
        ? theme.unknown(String(worktree.upstream.behind))
        : theme.merged("0")
  const divider = theme.muted("----------------------------------------")
  const lines = [
    theme.previewSection("WORKTREE"),
    divider,
    `  ${theme.previewLabel("Branch ")}: ${branchLabel}`,
    `  ${theme.previewLabel("Path   ")}: ${pathLabel}`,
    "",
    theme.previewSection("STATUS"),
    divider,
    `  ${theme.previewLabel("Dirty  ")}: ${dirtyValue}`,
    `  ${theme.previewLabel("Locked ")}: ${lockedValue}`,
    `  ${theme.previewLabel("Merged ")}: ${mergedValue}`,
    `  ${theme.previewLabel("Remote ")}: ${remoteValue}`,
    `  ${theme.previewLabel("Ahead  ")}: ${aheadValue}`,
    `  ${theme.previewLabel("Behind ")}: ${behindValue}`,
  ]

  if (worktree.locked.value) {
    lines.push("")
    lines.push(theme.previewSection("LOCK"))
    lines.push(divider)
    if (typeof worktree.locked.reason === "string" && worktree.locked.reason.length > 0) {
      lines.push(`  ${theme.previewLabel("Reason ")}: ${theme.value(worktree.locked.reason)}`)
    }
    if (typeof worktree.locked.owner === "string" && worktree.locked.owner.length > 0) {
      lines.push(`  ${theme.previewLabel("Owner  ")}: ${theme.value(worktree.locked.owner)}`)
    }
  }

  return lines.join("\n")
}

const buildCdCandidateLine = ({
  worktree,
  baseBranch,
  theme,
  currentWorktreeRoot,
  branchColumnWidth,
}: {
  readonly worktree: WorktreeStatus
  readonly baseBranch: string | null
  readonly theme: CatppuccinTheme
  readonly currentWorktreeRoot: string
  readonly branchColumnWidth: number
}): string => {
  const isBaseBranch = typeof worktree.branch === "string" && baseBranch !== null && worktree.branch === baseBranch
  const branchLabel = buildCdBranchLabel({
    worktree,
    currentWorktreeRoot,
  })
  const branchLabelPadded = padToDisplayWidth({
    value: branchLabel,
    width: branchColumnWidth,
  })
  const isCurrent = worktree.path === currentWorktreeRoot
  const branchDisplay =
    worktree.branch === null
      ? theme.branchDetached(branchLabelPadded)
      : isCurrent
        ? theme.branchCurrent(branchLabelPadded)
        : isBaseBranch
          ? theme.base(branchLabelPadded)
          : theme.branch(branchLabelPadded)
  const stateSummary = buildCdStateSummary({
    worktree,
    isBaseBranch,
    theme,
  })

  return [
    `${branchDisplay}  ${stateSummary}`,
    worktree.path,
    encodeCdPreviewField(
      buildCdPreviewText({
        worktree,
        baseBranch,
        theme,
      }),
    ),
  ].join("\t")
}

const resolveCdSelectionPath = (selectedLine: string): string => {
  const parts = selectedLine.split("\t")
  const rawPath = parts[1]
  if (typeof rawPath === "string" && rawPath.length > 0) {
    return rawPath
  }
  return selectedLine
}

const containsBranch = ({
  branch,
  worktrees,
}: {
  readonly branch: string
  readonly worktrees: readonly WorktreeStatus[]
}): boolean => {
  return worktrees.some((worktree) => worktree.branch === branch)
}

const readStringOption = (parsedArgsRecord: Record<string, unknown>, key: string): string | undefined => {
  const value = parsedArgsRecord[key]
  if (typeof value === "string") {
    return value
  }
  return undefined
}

const findCommandHelp = (commandName: string): CommandHelp | undefined => {
  return commandHelpEntries.find((entry) => entry.name === commandName)
}

const renderGeneralHelpText = ({ version }: { readonly version: string }): string => {
  const commandList = commandHelpEntries.map((entry) => `  ${entry.name.padEnd(8)} ${entry.summary}`).join("\n")
  return [
    "vde-worktree",
    "",
    "Usage:",
    "  vw <command> [options]",
    "  vde-worktree <command> [options]",
    "",
    `Version: ${version}`,
    "",
    "Commands:",
    commandList,
    "",
    "Global options:",
    "  --json                  Output machine-readable JSON.",
    "  --verbose               Enable verbose logs.",
    "  --no-hooks              Disable hooks for this run (requires --allow-unsafe).",
    "  --no-gh                 Disable GitHub CLI based PR status checks for this run.",
    "  --full-path             Disable list table path truncation.",
    "  --allow-unsafe          Explicitly allow unsafe behavior in non-TTY mode.",
    "  --hook-timeout-ms <ms>  Override hook timeout.",
    "  --lock-timeout-ms <ms>  Override repository lock timeout.",
    "  -h, --help              Show help.",
    "  -v, --version           Show version.",
    "",
    "Help commands:",
    "  vw help",
    "  vw help <command>",
    "  vw <command> --help",
    "",
    "Examples:",
    "  vw switch feature/foo",
    '  cd "$(vw cd)"',
    "  vw completion zsh --install",
    "  vw del feature/foo --force-unmerged --allow-unpushed --allow-unsafe",
  ].join("\n")
}

const renderCommandHelpText = ({ entry }: { readonly entry: CommandHelp }): string => {
  const lines = [`Command: ${entry.name}`, "", "Usage:", `  ${entry.usage}`, "", "Summary:", `  ${entry.summary}`]

  if (entry.details.length > 0) {
    lines.push("", "Details:")
    for (const detail of entry.details) {
      lines.push(`  - ${detail}`)
    }
  }

  if (entry.options !== undefined && entry.options.length > 0) {
    lines.push("", "Options:")
    for (const option of entry.options) {
      lines.push(`  - ${option}`)
    }
  }

  if (entry.examples !== undefined && entry.examples.length > 0) {
    lines.push("", "Examples:")
    for (const example of entry.examples) {
      lines.push(`  ${example}`)
    }
  }

  lines.push("", "Show all commands: vw help")
  return lines.join("\n")
}

const createHookContext = ({
  runtime,
  repoRoot,
  action,
  branch,
  worktreePath,
  stderr,
  extraEnv,
}: {
  readonly runtime: CommonRuntime
  readonly repoRoot: string
  readonly action: string
  readonly branch?: string | null
  readonly worktreePath?: string
  readonly stderr: (line: string) => void
  readonly extraEnv?: Record<string, string>
}): HookExecutionContext => {
  return {
    repoRoot,
    action,
    branch,
    worktreePath,
    timeoutMs: runtime.hookTimeoutMs,
    enabled: runtime.hooksEnabled,
    strictPostHooks: runtime.strictPostHooks,
    stderr,
    extraEnv,
  }
}

export const createCli = (options: CLIOptions = {}): CLI => {
  const require = createRequire(import.meta.url)
  const version =
    options.version ??
    ((): string => {
      try {
        return loadPackageVersion(require)
      } catch {
        return "0.0.0"
      }
    })()

  const runtimeCwd = options.cwd ?? process.cwd()
  const stdout = options.stdout ?? ((line: string): void => console.log(line))
  const stderr = options.stderr ?? ((line: string): void => console.error(line))
  const selectPathWithFzf = options.selectPathWithFzf ?? defaultSelectPathWithFzf
  const isInteractiveFn =
    options.isInteractive ?? ((): boolean => process.stdout.isTTY === true && process.stderr.isTTY === true)

  let logger: Logger = createLogger()

  const rootArgsDef = {
    command: {
      type: "positional",
      description: "Command name",
      required: false,
    },
    prompt: {
      type: "string",
      valueHint: "text",
      description: "Custom fzf prompt for cd command",
    },
    fzfArg: {
      type: "string",
      valueHint: "arg",
      description: "Additional argument passed to fzf (repeatable)",
    },
    json: {
      type: "boolean",
      description: "Output JSON on stdout",
    },
    verbose: {
      type: "boolean",
      description: "Show detailed logs",
    },
    hooks: {
      type: "boolean",
      description: "Enable hooks (disable with --no-hooks)",
      default: true,
    },
    gh: {
      type: "boolean",
      description: "Enable GitHub CLI based PR status checks (disable with --no-gh)",
      default: true,
    },
    fullPath: {
      type: "boolean",
      description: "Disable list table path truncation",
    },
    allowUnsafe: {
      type: "boolean",
      description: "Allow unsafe operations",
    },
    strictPostHooks: {
      type: "boolean",
      description: "Fail when post hooks fail",
    },
    hookTimeoutMs: {
      type: "string",
      valueHint: "ms",
      description: "Override hook timeout (ms)",
    },
    lockTimeoutMs: {
      type: "string",
      valueHint: "ms",
      description: "Override lock timeout (ms)",
    },
    allowAgent: {
      type: "boolean",
      description: "Allow non-TTY execution for use command",
    },
    allowShared: {
      type: "boolean",
      description: "Allow use checkout when target branch is attached by another worktree",
    },
    reason: {
      type: "string",
      valueHint: "text",
      description: "Reason text for lock command",
    },
    owner: {
      type: "string",
      valueHint: "owner",
      description: "Owner for lock/unlock commands",
    },
    force: {
      type: "boolean",
      description: "Force operation",
    },
    forceDirty: {
      type: "boolean",
      description: "Allow dirty worktree for del",
    },
    allowUnpushed: {
      type: "boolean",
      description: "Allow unpushed commits for del",
    },
    forceUnmerged: {
      type: "boolean",
      description: "Allow unmerged worktree for del",
    },
    forceLocked: {
      type: "boolean",
      description: "Allow deleting locked worktree",
    },
    apply: {
      type: "boolean",
      description: "Apply changes",
    },
    dryRun: {
      type: "boolean",
      description: "Dry-run mode",
    },
    current: {
      type: "boolean",
      description: "Use current worktree for extract",
    },
    from: {
      type: "string",
      valueHint: "value",
      description: "For extract: filesystem path. For absorb: managed worktree name.",
    },
    to: {
      type: "string",
      valueHint: "worktree-name",
      description: "Worktree name used by unabsorb --to",
    },
    stash: {
      type: "boolean",
      description: "Allow stash for extract",
    },
    keepStash: {
      type: "boolean",
      description: "Keep stash entry after absorb/unabsorb",
    },
    fallback: {
      type: "boolean",
      description: "Enable fallback behavior (disable with --no-fallback)",
      default: true,
    },
    install: {
      type: "boolean",
      description: "Install generated artifacts to default location (used by completion command)",
    },
    path: {
      type: "string",
      valueHint: "path",
      description: "Custom file path (used by completion command install)",
    },
    help: {
      type: "boolean",
      alias: "h",
      description: "Show help",
    },
    version: {
      type: "boolean",
      alias: "v",
      description: "Show version",
    },
  } satisfies ArgsDef

  const optionSpecs = buildOptionSpecs(rootArgsDef)

  const run = async (rawArgs: string[] = process.argv.slice(2)): Promise<number> => {
    logger = createLogger()
    let command = "unknown"
    let jsonEnabled = false
    let repoRootForJson: string | null = null

    try {
      const { beforeDoubleDash, afterDoubleDash } = splitRawArgsByDoubleDash(rawArgs)
      validateRawOptions(beforeDoubleDash, optionSpecs)
      const parsedArgs = parseArgs(beforeDoubleDash, rootArgsDef)
      const parsedArgsRecord = parsedArgs as Record<string, unknown>
      const positionals = getPositionals(parsedArgs)
      command = positionals[0] ?? "unknown"
      const commandArgs = positionals.slice(1)
      jsonEnabled = parsedArgs.json === true
      const commandContext: CommandContext = {
        command,
        commandArgs,
        positionals,
        parsedArgs: parsedArgsRecord,
        jsonEnabled,
      }

      const readOnlyDispatch = await dispatchReadOnlyCommands({
        ...commandContext,
        version,
        availableCommandNames: commandHelpNames,
        stdout,
        findCommandHelp,
        renderGeneralHelpText,
        renderCommandHelpText,
        ensureArgumentCount,
        resolveCompletionShell,
        loadCompletionScript,
        resolveCompletionInstallPath,
        installCompletionScript,
        readStringOption,
        buildJsonSuccess,
      })
      if (readOnlyDispatch.handled) {
        return readOnlyDispatch.exitCode
      }

      logger = parsedArgs.verbose === true ? createLogger({ level: LogLevel.INFO }) : createLogger()

      const allowUnsafe = parsedArgs.allowUnsafe === true
      if (parsedArgs.hooks === false && allowUnsafe !== true) {
        throw createCliError("UNSAFE_FLAG_REQUIRED", {
          message: "UNSAFE_FLAG_REQUIRED: --no-hooks requires --allow-unsafe",
        })
      }

      const repoContext = await resolveRepoContext(runtimeCwd)
      const repoRoot = repoContext.repoRoot
      repoRootForJson = repoRoot
      const { config: resolvedConfig } = await loadResolvedConfig({
        cwd: runtimeCwd,
        repoRoot,
      })
      const managedWorktreeRoot = getWorktreeRootPath(repoRoot, resolvedConfig.paths.worktreeRoot)

      const runtime: CommonRuntime = {
        command,
        json: jsonEnabled,
        hooksEnabled: parsedArgs.hooks !== false && resolvedConfig.hooks.enabled,
        ghEnabled: parsedArgs.gh !== false && resolvedConfig.github.enabled,
        strictPostHooks: parsedArgs.strictPostHooks === true,
        hookTimeoutMs: readNumberFromEnvOrDefault({
          rawValue:
            toNumberOption({ value: parsedArgs.hookTimeoutMs, optionName: "--hook-timeout-ms" }) ??
            resolvedConfig.hooks.timeoutMs,
          defaultValue: DEFAULT_HOOK_TIMEOUT_MS,
        }),
        lockTimeoutMs: readNumberFromEnvOrDefault({
          rawValue:
            toNumberOption({ value: parsedArgs.lockTimeoutMs, optionName: "--lock-timeout-ms" }) ??
            resolvedConfig.locks.timeoutMs,
          defaultValue: DEFAULT_LOCK_TIMEOUT_MS,
        }),
        allowUnsafe,
        isInteractive: isInteractiveFn(),
      }

      const staleLockTTLSeconds = readNumberFromEnvOrDefault({
        rawValue: resolvedConfig.locks.staleLockTTLSeconds,
        defaultValue: DEFAULT_STALE_LOCK_TTL_SECONDS,
      })

      const collectWorktreeSnapshot = async (_ignoredRepoRoot: string): Promise<WorktreeSnapshot> => {
        const baseBranch = await resolveBaseBranch({
          repoRoot,
          config: resolvedConfig,
        })
        return collectWorktreeSnapshotBase(repoRoot, {
          baseBranch,
          ghEnabled: runtime.ghEnabled,
          noGh: runtime.ghEnabled !== true,
        })
      }

      const runWriteOperation = async <T>(task: () => Promise<T>): Promise<T> => {
        if (WRITE_COMMANDS.has(command) !== true) {
          return task()
        }
        if (command !== "init") {
          await validateInitializedForWrite(repoRoot)
        }
        return withRepoLock(
          {
            repoRoot,
            command,
            timeoutMs: runtime.lockTimeoutMs,
            staleLockTTLSeconds,
          },
          task,
        )
      }

      type WorktreeMutationName = "new" | "switch" | "mv" | "del"

      type WorktreeMutationPlan<TPrecheckResult, TResult> = {
        readonly name: WorktreeMutationName
        readonly branch: string | null
        readonly worktreePath: string
        readonly extraEnv?: Record<string, string>
        readonly precheck: () => Promise<TPrecheckResult>
        readonly runGit: (precheckResult: TPrecheckResult) => Promise<TResult>
        readonly finalize?: (precheckResult: TPrecheckResult, result: TResult) => Promise<void>
      }

      const executeWorktreeMutation = async <TPrecheckResult, TResult>({
        name,
        branch,
        worktreePath,
        extraEnv,
        precheck,
        runGit,
        finalize,
      }: WorktreeMutationPlan<TPrecheckResult, TResult>): Promise<TResult> => {
        const precheckResult = await precheck()
        const hookContext = createHookContext({
          runtime,
          repoRoot,
          action: name,
          branch,
          worktreePath,
          stderr,
          extraEnv,
        })
        await runPreHook({ name, context: hookContext })
        const result = await runGit(precheckResult)
        if (finalize !== undefined) {
          await finalize(precheckResult, result)
        }
        await runPostHook({ name, context: hookContext })
        return result
      }

      const handleInit = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 0 })
        const result = await runWriteOperation(async () => {
          const hookContext = createHookContext({
            runtime,
            repoRoot,
            action: "init",
            branch: null,
            worktreePath: repoRoot,
            stderr,
          })
          await runPreHook({ name: "init", context: hookContext })
          const initialized = await initializeRepository({
            repoRoot,
            managedWorktreeRoot,
          })
          await runPostHook({ name: "init", context: hookContext })
          return initialized
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  initialized: true,
                  alreadyInitialized: result.alreadyInitialized,
                },
              }),
            ),
          )
        }
        return EXIT_CODE.OK
      }

      const handleList = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 0 })
        const snapshot = await collectWorktreeSnapshot(repoRoot)
        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  baseBranch: snapshot.baseBranch,
                  managedWorktreeRoot,
                  worktrees: snapshot.worktrees,
                },
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        const theme = createCatppuccinTheme({
          enabled: shouldUseAnsiColors({ interactive: runtime.isInteractive }),
        })
        const columns = resolvedConfig.list.table.columns
        const rows: string[][] = [
          [...columns],
          ...(await Promise.all(
            snapshot.worktrees.map(async (worktree) => {
              const distanceFromBase = await resolveAheadBehindAgainstBaseBranch({
                repoRoot,
                baseBranch: snapshot.baseBranch,
                worktree,
              })
              const isBaseBranch =
                worktree.branch !== null && snapshot.baseBranch !== null && worktree.branch === snapshot.baseBranch
              const mergedState =
                isBaseBranch === true
                  ? "-"
                  : worktree.merged.overall === true
                    ? "merged"
                    : worktree.merged.overall === false
                      ? "unmerged"
                      : "unknown"
              const prState = formatPrDisplayState({
                prStatus: worktree.pr.status,
                isBaseBranch,
              })
              const isCurrent = worktree.path === repoContext.currentWorktreeRoot
              const valuesByColumn: Record<ListTableColumn, string> = {
                branch: `${isCurrent ? "*" : " "} ${worktree.branch ?? "(detached)"}`,
                dirty: worktree.dirty ? "dirty" : "clean",
                merged: mergedState,
                pr: prState,
                locked: worktree.locked.value ? "locked" : "-",
                ahead: formatListUpstreamCount(distanceFromBase.ahead),
                behind: formatListUpstreamCount(distanceFromBase.behind),
                path: formatDisplayPath(worktree.path),
              }
              return columns.map((column) => valuesByColumn[column])
            }),
          )),
        ]

        const pathColumnWidth = resolveListPathColumnWidth({
          rows,
          columns,
          truncateMode: resolvedConfig.list.table.path.truncate,
          fullPath: parsedArgs.fullPath === true,
          minWidth: resolvedConfig.list.table.path.minWidth,
        })
        const pathColumnIndex = columns.indexOf("path")
        const columnsConfig =
          pathColumnWidth === null || pathColumnIndex < 0
            ? undefined
            : {
                [pathColumnIndex]: {
                  width: pathColumnWidth,
                  truncate: pathColumnWidth,
                },
              }
        const rendered = table(rows, {
          border: getBorderCharacters("norc"),
          drawHorizontalLine: (lineIndex, rowCount) => {
            return lineIndex === 0 || lineIndex === 1 || lineIndex === rowCount
          },
          columns: columnsConfig,
        })
        const colorized = hasDefaultListColumnOrder(columns)
          ? colorizeListTable({
              rendered,
              theme,
            })
          : rendered.trimEnd()

        for (const line of colorized.split("\n")) {
          stdout(line)
        }
        return EXIT_CODE.OK
      }

      const handleStatus = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 1 })
        const snapshot = await collectWorktreeSnapshot(repoRoot)
        const targetBranch = commandArgs[0]
        const targetWorktree =
          typeof targetBranch === "string" && targetBranch.length > 0
            ? resolveTargetWorktreeByBranch({ branch: targetBranch, worktrees: snapshot.worktrees })
            : resolveCurrentWorktree({
                snapshot,
                currentWorktreeRoot: repoContext.currentWorktreeRoot,
              })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  worktree: targetWorktree,
                },
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        stdout(`branch: ${targetWorktree.branch ?? "(detached)"}`)
        stdout(`path: ${formatDisplayPath(targetWorktree.path)}`)
        stdout(`dirty: ${targetWorktree.dirty ? "true" : "false"}`)
        stdout(`locked: ${targetWorktree.locked.value ? "true" : "false"}`)
        return EXIT_CODE.OK
      }

      const handlePath = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const branch = commandArgs[0] as string
        const snapshot = await collectWorktreeSnapshot(repoRoot)
        const target = resolveTargetWorktreeByBranch({ branch, worktrees: snapshot.worktrees })
        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  branch,
                  path: target.path,
                },
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        stdout(target.path)
        return EXIT_CODE.OK
      }

      const earlyRepoExitCode = await dispatchCommandHandler({
        command,
        handlers: createEarlyRepoCommandHandlers({
          initHandler: handleInit,
          listHandler: handleList,
          statusHandler: handleStatus,
          pathHandler: handlePath,
        }),
      })
      if (earlyRepoExitCode !== undefined) {
        return earlyRepoExitCode
      }

      const handleNew = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 1 })
        const branch = commandArgs[0] ?? randomWipBranchName()
        const targetPath = branchToWorktreePath(repoRoot, branch, resolvedConfig.paths.worktreeRoot)
        const result = await runWriteOperation(async () => {
          return executeWorktreeMutation({
            name: "new",
            branch,
            worktreePath: targetPath,
            precheck: async () => {
              const snapshot = await collectWorktreeSnapshot(repoRoot)
              if (containsBranch({ branch, worktrees: snapshot.worktrees })) {
                throw createCliError("BRANCH_ALREADY_ATTACHED", {
                  message: `Branch is already attached to a worktree: ${branch}`,
                  details: { branch },
                })
              }

              if (await doesGitRefExist(repoRoot, `refs/heads/${branch}`)) {
                throw createCliError("BRANCH_ALREADY_EXISTS", {
                  message: `Branch already exists locally: ${branch}`,
                  details: { branch },
                })
              }

              await ensureTargetPathWritable(targetPath)
              const baseBranch = await resolveBaseBranch({
                repoRoot,
                config: resolvedConfig,
              })
              return { baseBranch }
            },
            runGit: async ({ baseBranch }) => {
              await runGitCommand({
                cwd: repoRoot,
                args: ["worktree", "add", "-b", branch, targetPath, baseBranch],
              })
              return { branch, path: targetPath }
            },
            finalize: async ({ baseBranch }) => {
              await upsertWorktreeMergeLifecycle({
                repoRoot,
                branch,
                baseBranch,
                observedDivergedHead: null,
              })
            },
          })
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "created",
                repoRoot,
                details: result,
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        stdout(result.path)
        return EXIT_CODE.OK
      }

      const handleSwitch = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const branch = commandArgs[0] as string
        const result = await runWriteOperation(async () => {
          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const existing = snapshot.worktrees.find((worktree) => worktree.branch === branch)
          if (existing !== undefined) {
            if (snapshot.baseBranch !== null) {
              await upsertWorktreeMergeLifecycle({
                repoRoot,
                branch,
                baseBranch: snapshot.baseBranch,
                observedDivergedHead: null,
              })
            }
            return { status: "existing" as const, branch, path: existing.path }
          }

          const targetPath = branchToWorktreePath(repoRoot, branch, resolvedConfig.paths.worktreeRoot)
          return executeWorktreeMutation({
            name: "switch",
            branch,
            worktreePath: targetPath,
            precheck: async () => {
              await ensureTargetPathWritable(targetPath)
              if (await doesGitRefExist(repoRoot, `refs/heads/${branch}`)) {
                return {
                  gitArgs: ["worktree", "add", targetPath, branch] as const,
                  lifecycleBaseBranch: snapshot.baseBranch,
                }
              }
              const baseBranch = await resolveBaseBranch({
                repoRoot,
                config: resolvedConfig,
              })
              return {
                gitArgs: ["worktree", "add", "-b", branch, targetPath, baseBranch] as const,
                lifecycleBaseBranch: baseBranch,
              }
            },
            runGit: async ({ gitArgs }) => {
              await runGitCommand({
                cwd: repoRoot,
                args: [...gitArgs],
              })
              return { status: "created" as const, branch, path: targetPath }
            },
            finalize: async ({ lifecycleBaseBranch }) => {
              if (lifecycleBaseBranch !== null) {
                await upsertWorktreeMergeLifecycle({
                  repoRoot,
                  branch,
                  baseBranch: lifecycleBaseBranch,
                  observedDivergedHead: null,
                })
              }
            },
          })
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: result.status,
                repoRoot,
                details: {
                  branch: result.branch,
                  path: result.path,
                },
              }),
            ),
          )
          return EXIT_CODE.OK
        }
        stdout(result.path)
        return EXIT_CODE.OK
      }

      const writeCommandExitCode = await dispatchCommandHandler({
        command,
        handlers: createWriteCommandHandlers({
          newHandler: handleNew,
          switchHandler: handleSwitch,
        }),
      })
      if (writeCommandExitCode !== undefined) {
        return writeCommandExitCode
      }

      const handleMv = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const newBranch = commandArgs[0] as string
        const result = await runWriteOperation(async () => {
          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const current = resolveCurrentWorktree({ snapshot, currentWorktreeRoot: repoContext.currentWorktreeRoot })
          const oldBranch = current.branch
          if (oldBranch === null) {
            throw createCliError("DETACHED_HEAD", {
              message: "mv requires a branch checkout (detached HEAD is not supported)",
              details: { path: current.path },
            })
          }
          if (current.path === repoRoot) {
            throw createCliError("INVALID_ARGUMENT", {
              message: "mv cannot move the primary worktree",
              details: { path: current.path },
            })
          }
          if (oldBranch === newBranch) {
            return {
              branch: newBranch,
              path: current.path,
            }
          }
          const newPath = branchToWorktreePath(repoRoot, newBranch, resolvedConfig.paths.worktreeRoot)
          return executeWorktreeMutation({
            name: "mv",
            branch: newBranch,
            worktreePath: newPath,
            extraEnv: {
              WT_OLD_BRANCH: oldBranch,
              WT_NEW_BRANCH: newBranch,
            },
            precheck: async () => {
              if (containsBranch({ branch: newBranch, worktrees: snapshot.worktrees })) {
                throw createCliError("BRANCH_ALREADY_ATTACHED", {
                  message: `Branch is already attached to another worktree: ${newBranch}`,
                  details: { branch: newBranch },
                })
              }
              if (await doesGitRefExist(repoRoot, `refs/heads/${newBranch}`)) {
                throw createCliError("BRANCH_ALREADY_EXISTS", {
                  message: `Branch already exists locally: ${newBranch}`,
                  details: { branch: newBranch },
                })
              }

              await ensureTargetPathWritable(newPath)
              return {
                oldBranch,
                currentPath: current.path,
                baseBranch: snapshot.baseBranch,
              }
            },
            runGit: async ({ oldBranch: resolvedOldBranch, currentPath }) => {
              await runGitCommand({
                cwd: currentPath,
                args: ["branch", "-m", resolvedOldBranch, newBranch],
              })
              await runGitCommand({
                cwd: repoRoot,
                args: ["worktree", "move", currentPath, newPath],
              })
              return {
                branch: newBranch,
                path: newPath,
              }
            },
            finalize: async ({ oldBranch: resolvedOldBranch, baseBranch }) => {
              if (baseBranch !== null) {
                await moveWorktreeMergeLifecycle({
                  repoRoot,
                  fromBranch: resolvedOldBranch,
                  toBranch: newBranch,
                  baseBranch,
                  observedDivergedHead: null,
                })
              }
            },
          })
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: result,
              }),
            ),
          )
          return EXIT_CODE.OK
        }
        stdout(result.path)
        return EXIT_CODE.OK
      }

      const handleDel = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 1 })
        const forceFlags = parseForceFlags(parsedArgsRecord)
        if (hasAnyForceFlag(forceFlags)) {
          ensureUnsafeForNonTty({
            runtime,
            reason: "force flags in non-TTY mode require --allow-unsafe",
          })
        }
        const branchArg = commandArgs[0]

        const result = await runWriteOperation(async () => {
          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const target =
            typeof branchArg === "string" && branchArg.length > 0
              ? resolveTargetWorktreeByBranch({ branch: branchArg, worktrees: snapshot.worktrees })
              : resolveCurrentWorktree({
                  snapshot,
                  currentWorktreeRoot: repoContext.currentWorktreeRoot,
                })

          if (target.branch === null) {
            throw createCliError("DETACHED_HEAD", {
              message: "Cannot delete detached worktree without branch",
              details: { path: target.path },
            })
          }
          if (target.path === repoRoot) {
            throw createCliError("INVALID_ARGUMENT", {
              message: "Cannot delete the primary worktree",
              details: { path: target.path },
            })
          }
          if (
            isManagedWorktreePath({
              worktreePath: target.path,
              managedWorktreeRoot,
            }) !== true
          ) {
            throw createCliError("WORKTREE_NOT_FOUND", {
              message: "Target branch is not in managed worktree root",
              details: {
                branch: target.branch,
                path: target.path,
                managedWorktreeRoot,
              },
            })
          }
          const targetBranch = target.branch

          return executeWorktreeMutation({
            name: "del",
            branch: targetBranch,
            worktreePath: target.path,
            precheck: async () => {
              validateDeleteSafety({
                target,
                forceFlags,
              })

              const removeArgs = ["worktree", "remove", target.path]
              if (forceFlags.forceDirty) {
                removeArgs.push("--force")
              }
              return {
                branch: targetBranch,
                path: target.path,
                removeArgs,
                branchDeleteMode: resolveBranchDeleteMode(forceFlags),
              }
            },
            runGit: async ({ branch: targetBranch, removeArgs, branchDeleteMode, path }) => {
              await runGitCommand({
                cwd: repoRoot,
                args: removeArgs,
              })
              await runGitCommand({
                cwd: repoRoot,
                args: ["branch", branchDeleteMode, targetBranch],
              })
              return {
                branch: targetBranch,
                path,
              }
            },
            finalize: async ({ branch: targetBranch }) => {
              await deleteWorktreeLock({ repoRoot, branch: targetBranch })
              await deleteWorktreeMergeLifecycle({ repoRoot, branch: targetBranch })
            },
          })
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "deleted",
                repoRoot,
                details: result,
              }),
            ),
          )
          return EXIT_CODE.OK
        }
        stdout(result.path)
        return EXIT_CODE.OK
      }

      const writeMutationExitCode = await dispatchCommandHandler({
        command,
        handlers: createWriteMutationHandlers({
          mvHandler: handleMv,
          delHandler: handleDel,
        }),
      })
      if (writeMutationExitCode !== undefined) {
        return writeMutationExitCode
      }

      const handleGone = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 0 })
        if (parsedArgs.apply === true && parsedArgs.dryRun === true) {
          throw createCliError("INVALID_ARGUMENT", {
            message: "Cannot use --apply and --dry-run together",
          })
        }

        const dryRun = parsedArgs.apply !== true
        const execute = async (): Promise<{ deleted: string[]; candidates: string[]; dryRun: boolean }> => {
          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const candidates = snapshot.worktrees
            .filter((worktree) => worktree.branch !== null)
            .filter((worktree) => worktree.path !== repoRoot)
            .filter((worktree) =>
              isManagedWorktreePath({
                worktreePath: worktree.path,
                managedWorktreeRoot,
              }),
            )
            .filter((worktree) => worktree.dirty === false)
            .filter((worktree) => worktree.locked.value === false)
            .filter((worktree) => worktree.merged.overall === true)
            .map((worktree) => worktree.branch as string)

          if (dryRun) {
            return {
              deleted: [],
              candidates,
              dryRun: true,
            }
          }

          const hookContext = createHookContext({
            runtime,
            repoRoot,
            action: "gone",
            branch: null,
            worktreePath: repoRoot,
            stderr,
          })
          await runPreHook({ name: "gone", context: hookContext })

          const deleted: string[] = []
          for (const branch of candidates) {
            const latestSnapshot = await collectWorktreeSnapshot(repoRoot)
            const target = resolveTargetWorktreeByBranch({
              branch,
              worktrees: latestSnapshot.worktrees,
            })
            await runGitCommand({
              cwd: repoRoot,
              args: ["worktree", "remove", target.path],
            })
            await runGitCommand({
              cwd: repoRoot,
              args: ["branch", "-d", branch],
            })
            await deleteWorktreeLock({ repoRoot, branch })
            await deleteWorktreeMergeLifecycle({ repoRoot, branch })
            deleted.push(branch)
          }

          await runPostHook({ name: "gone", context: hookContext })
          return {
            deleted,
            candidates,
            dryRun: false,
          }
        }

        const result = await runWriteOperation(execute)
        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  dryRun: result.dryRun,
                  candidates: result.candidates,
                  deleted: result.deleted,
                },
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        const label = result.dryRun ? "candidates" : "deleted"
        const branches = result.dryRun ? result.candidates : result.deleted
        for (const branch of branches) {
          stdout(`${label}: ${branch}`)
        }
        return EXIT_CODE.OK
      }

      const handleAdopt = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 0 })
        if (parsedArgs.apply === true && parsedArgs.dryRun === true) {
          throw createCliError("INVALID_ARGUMENT", {
            message: "Cannot use --apply and --dry-run together",
          })
        }

        type AdoptCandidate = {
          readonly branch: string
          readonly fromPath: string
          readonly toPath: string
        }

        type AdoptSkippedReason = "detached" | "locked" | "target_exists" | "target_conflict"

        type AdoptSkipped = {
          readonly branch: string | null
          readonly fromPath: string
          readonly toPath: string | null
          readonly reason: AdoptSkippedReason
        }

        type AdoptFailed = AdoptCandidate & {
          readonly code: string
          readonly message: string
        }

        const dryRun = parsedArgs.apply !== true
        const result = await runWriteOperation(async () => {
          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const candidates: AdoptCandidate[] = []
          const skipped: AdoptSkipped[] = []
          const reservedTargetPaths = new Set<string>()
          const sortedWorktrees = [...snapshot.worktrees].sort((a, b) => a.path.localeCompare(b.path))

          for (const worktree of sortedWorktrees) {
            if (worktree.path === repoRoot) {
              continue
            }
            if (
              isManagedWorktreePath({
                worktreePath: worktree.path,
                managedWorktreeRoot,
              })
            ) {
              continue
            }
            if (worktree.branch === null) {
              skipped.push({
                branch: null,
                fromPath: worktree.path,
                toPath: null,
                reason: "detached",
              })
              continue
            }
            if (worktree.locked.value) {
              skipped.push({
                branch: worktree.branch,
                fromPath: worktree.path,
                toPath: null,
                reason: "locked",
              })
              continue
            }

            const toPath = branchToWorktreePath(repoRoot, worktree.branch, resolvedConfig.paths.worktreeRoot)
            if (reservedTargetPaths.has(toPath)) {
              skipped.push({
                branch: worktree.branch,
                fromPath: worktree.path,
                toPath,
                reason: "target_conflict",
              })
              continue
            }
            if (await doesPathExist(toPath)) {
              skipped.push({
                branch: worktree.branch,
                fromPath: worktree.path,
                toPath,
                reason: "target_exists",
              })
              continue
            }

            reservedTargetPaths.add(toPath)
            candidates.push({
              branch: worktree.branch,
              fromPath: worktree.path,
              toPath,
            })
          }

          if (dryRun) {
            return {
              dryRun: true as const,
              managedWorktreeRoot,
              candidates,
              moved: [] as AdoptCandidate[],
              skipped,
              failed: [] as AdoptFailed[],
            }
          }

          const hookContext = createHookContext({
            runtime,
            repoRoot,
            action: "adopt",
            branch: null,
            worktreePath: managedWorktreeRoot,
            stderr,
          })
          await runPreHook({ name: "adopt", context: hookContext })

          const moved: AdoptCandidate[] = []
          const failed: AdoptFailed[] = []
          for (const candidate of candidates) {
            try {
              await mkdir(dirname(candidate.toPath), { recursive: true })
              await runGitCommand({
                cwd: repoRoot,
                args: ["worktree", "move", candidate.fromPath, candidate.toPath],
              })
              moved.push(candidate)
            } catch (error) {
              const cliError = ensureCliError(error)
              failed.push({
                ...candidate,
                code: cliError.code,
                message: cliError.message,
              })
            }
          }

          await runPostHook({ name: "adopt", context: hookContext })
          return {
            dryRun: false as const,
            managedWorktreeRoot,
            candidates,
            moved,
            skipped,
            failed,
          }
        })

        const hasFailures = result.failed.length > 0

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: result,
              }),
            ),
          )
          return hasFailures ? EXIT_CODE.SAFETY_REJECTED : EXIT_CODE.OK
        }

        if (result.candidates.length === 0 && result.skipped.length === 0) {
          stdout("no unmanaged worktree candidates")
          return EXIT_CODE.OK
        }

        const formatMoveLine = (prefix: string, candidate: AdoptCandidate): string => {
          return `${prefix}: ${candidate.branch}\t${candidate.fromPath} -> ${candidate.toPath}`
        }
        const formatSkipLine = (entry: AdoptSkipped): string => {
          const branch = entry.branch ?? "(detached)"
          const toPart = entry.toPath === null ? "" : ` -> ${entry.toPath}`
          return `skipped(${entry.reason}): ${branch}\t${entry.fromPath}${toPart}`
        }
        const formatFailedLine = (entry: AdoptFailed): string => {
          return `${formatMoveLine("failed", entry)} [${entry.code}] ${entry.message}`
        }

        for (const candidate of result.candidates) {
          const prefix = result.dryRun ? "candidate" : "planned"
          stdout(formatMoveLine(prefix, candidate))
        }
        for (const moved of result.moved) {
          stdout(formatMoveLine("moved", moved))
        }
        for (const skipped of result.skipped) {
          stdout(formatSkipLine(skipped))
        }
        for (const failed of result.failed) {
          stderr(formatFailedLine(failed))
        }
        return hasFailures ? EXIT_CODE.SAFETY_REJECTED : EXIT_CODE.OK
      }

      const handleGet = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const remoteBranchArg = commandArgs[0] as string
        const { remote, branch } = resolveRemoteAndBranch(remoteBranchArg)

        const result = await runWriteOperation(async () => {
          const remoteCheck = await runGitCommand({
            cwd: repoRoot,
            args: ["remote", "get-url", remote],
            reject: false,
          })
          if (remoteCheck.exitCode !== 0) {
            throw createCliError("REMOTE_NOT_FOUND", {
              message: `Remote not found: ${remote}`,
              details: { remote },
            })
          }

          const hookContext = createHookContext({
            runtime,
            repoRoot,
            action: "get",
            branch,
            worktreePath: branchToWorktreePath(repoRoot, branch, resolvedConfig.paths.worktreeRoot),
            stderr,
          })
          await runPreHook({ name: "get", context: hookContext })

          const fetchResult = await runGitCommand({
            cwd: repoRoot,
            args: ["fetch", remote, branch],
            reject: false,
          })
          if (fetchResult.exitCode !== 0) {
            throw createCliError("REMOTE_BRANCH_NOT_FOUND", {
              message: `Remote branch not found: ${remote}/${branch}`,
              details: { remote, branch },
            })
          }

          if ((await doesGitRefExist(repoRoot, `refs/heads/${branch}`)) !== true) {
            await runGitCommand({
              cwd: repoRoot,
              args: ["branch", "--track", branch, `${remote}/${branch}`],
            })
          }

          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const lifecycleBaseBranch = snapshot.baseBranch
          const existing = snapshot.worktrees.find((worktree) => worktree.branch === branch)
          if (existing !== undefined) {
            if (lifecycleBaseBranch !== null) {
              await upsertWorktreeMergeLifecycle({
                repoRoot,
                branch,
                baseBranch: lifecycleBaseBranch,
                observedDivergedHead: null,
              })
            }
            await runPostHook({ name: "get", context: hookContext })
            return {
              status: "existing" as const,
              branch,
              path: existing.path,
            }
          }

          const targetPath = branchToWorktreePath(repoRoot, branch, resolvedConfig.paths.worktreeRoot)
          await ensureTargetPathWritable(targetPath)
          await runGitCommand({
            cwd: repoRoot,
            args: ["worktree", "add", targetPath, branch],
          })
          if (lifecycleBaseBranch !== null) {
            await upsertWorktreeMergeLifecycle({
              repoRoot,
              branch,
              baseBranch: lifecycleBaseBranch,
              observedDivergedHead: null,
            })
          }
          await runPostHook({ name: "get", context: hookContext })
          return {
            status: "created" as const,
            branch,
            path: targetPath,
          }
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: result.status,
                repoRoot,
                details: {
                  branch: result.branch,
                  path: result.path,
                },
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        stdout(result.path)
        return EXIT_CODE.OK
      }

      const handleExtract = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 0 })
        const fromPath = typeof parsedArgs.from === "string" ? parsedArgs.from : undefined
        if (fromPath !== undefined && parsedArgs.current === true) {
          throw createCliError("INVALID_ARGUMENT", {
            message: "extract cannot use --current and --from together",
          })
        }

        const result = await runWriteOperation(async () => {
          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const resolvedSourcePathRaw =
            fromPath !== undefined
              ? resolvePathFromCwd({
                  cwd: runtimeCwd,
                  path: fromPath,
                })
              : repoContext.currentWorktreeRoot
          const resolvedSourcePath = ensurePathInsideRepo({
            repoRoot,
            path: resolvedSourcePathRaw,
          })
          const sourceWorktree =
            snapshot.worktrees.find((worktree) => worktree.path === resolvedSourcePath) ??
            snapshot.worktrees.find((worktree) => resolvedSourcePath.startsWith(`${worktree.path}${sep}`))

          if (sourceWorktree === undefined) {
            throw createCliError("WORKTREE_NOT_FOUND", {
              message: "extract source worktree not found",
              details: { path: resolvedSourcePath },
            })
          }
          if (sourceWorktree.path !== repoRoot) {
            throw createCliError("INVALID_ARGUMENT", {
              message: "extract currently supports only the primary worktree",
              details: { sourcePath: sourceWorktree.path },
            })
          }
          if (sourceWorktree.branch === null) {
            throw createCliError("DETACHED_HEAD", {
              message: "extract requires current branch checkout",
              details: { path: sourceWorktree.path },
            })
          }

          const branch = sourceWorktree.branch
          const baseBranch = await resolveBaseBranch({
            repoRoot,
            config: resolvedConfig,
          })
          ensureBranchIsNotPrimary({ branch, baseBranch })
          const targetPath = branchToWorktreePath(repoRoot, branch, resolvedConfig.paths.worktreeRoot)
          await ensureTargetPathWritable(targetPath)

          const status = await runGitCommand({
            cwd: repoRoot,
            args: ["status", "--porcelain"],
            reject: false,
          })
          const dirty = status.stdout.trim().length > 0
          if (dirty && parsedArgs.stash !== true) {
            throw createCliError("DIRTY_WORKTREE", {
              message: "extract requires clean worktree unless --stash is specified",
            })
          }

          let stashOid: string | null = null
          if (dirty && parsedArgs.stash === true) {
            stashOid = await createStashEntry({
              cwd: repoRoot,
              message: `vde-worktree extract ${branch}`,
            })
          }

          const hookContext = createHookContext({
            runtime,
            repoRoot,
            action: "extract",
            branch,
            worktreePath: targetPath,
            stderr,
          })
          await runPreHookWithAutoRestore({
            name: "extract",
            context: hookContext,
            restore:
              stashOid !== null
                ? async (): Promise<void> => {
                    await restoreStashedChanges({
                      cwd: repoRoot,
                      stashOid,
                    })
                  }
                : undefined,
          })
          await runGitCommand({
            cwd: repoRoot,
            args: ["checkout", baseBranch],
          })
          await runGitCommand({
            cwd: repoRoot,
            args: ["worktree", "add", targetPath, branch],
          })
          await upsertWorktreeMergeLifecycle({
            repoRoot,
            branch,
            baseBranch,
            observedDivergedHead: null,
          })

          if (stashOid !== null) {
            const applyResult = await runGitCommand({
              cwd: targetPath,
              args: ["stash", "apply", stashOid],
              reject: false,
            })
            if (applyResult.exitCode !== 0) {
              throw createCliError("STASH_APPLY_FAILED", {
                message: "Failed to apply stash to extracted worktree",
                details: { stashOid, branch, path: targetPath },
              })
            }
            await dropStashByOid({
              cwd: repoRoot,
              stashOid,
            })
          }

          await runPostHook({ name: "extract", context: hookContext })
          return {
            branch,
            path: targetPath,
          }
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "created",
                repoRoot,
                details: result,
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        stdout(result.path)
        return EXIT_CODE.OK
      }

      const worktreeActionExitCode = await dispatchCommandHandler({
        command,
        handlers: createWorktreeActionHandlers({
          goneHandler: handleGone,
          adoptHandler: handleAdopt,
          getHandler: handleGet,
          extractHandler: handleExtract,
        }),
      })
      if (worktreeActionExitCode !== undefined) {
        return worktreeActionExitCode
      }

      const handleAbsorb = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const branch = commandArgs[0] as string
        const fromWorktreeName = typeof parsedArgs.from === "string" ? parsedArgs.from : undefined
        const keepStash = parsedArgs.keepStash === true
        if (runtime.isInteractive !== true) {
          if (parsedArgs.allowAgent !== true) {
            throw createCliError("UNSAFE_FLAG_REQUIRED", {
              message: "UNSAFE_FLAG_REQUIRED: absorb in non-TTY requires --allow-agent",
            })
          }
          ensureUnsafeForNonTty({
            runtime,
            reason: "absorb in non-TTY mode requires --allow-unsafe",
          })
        }

        const result = await runWriteOperation(async () => {
          const primaryStatus = await runGitCommand({
            cwd: repoRoot,
            args: ["status", "--porcelain"],
            reject: false,
          })
          if (primaryStatus.stdout.trim().length > 0) {
            throw createCliError("DIRTY_WORKTREE", {
              message: "absorb requires clean primary worktree",
              details: { repoRoot },
            })
          }

          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const sourceWorktree = resolveManagedNonPrimaryWorktreeByBranch({
            repoRoot,
            managedWorktreeRoot,
            branch,
            worktrees: snapshot.worktrees,
            optionName: "--from",
            worktreeName: fromWorktreeName,
            role: "source",
          })

          const sourceStatus = await runGitCommand({
            cwd: sourceWorktree.path,
            args: ["status", "--porcelain"],
            reject: false,
          })
          const sourceDirty = sourceStatus.stdout.trim().length > 0
          let stashOid: string | null = null
          if (sourceDirty) {
            stashOid = await createStashEntry({
              cwd: sourceWorktree.path,
              message: `vde-worktree absorb ${branch}`,
            })
          }

          const hookContext = createHookContext({
            runtime,
            repoRoot,
            action: "absorb",
            branch,
            worktreePath: repoRoot,
            stderr,
            extraEnv: {
              WT_SOURCE_WORKTREE_PATH: sourceWorktree.path,
            },
          })
          await runPreHookWithAutoRestore({
            name: "absorb",
            context: hookContext,
            restore:
              stashOid !== null
                ? async (): Promise<void> => {
                    await restoreStashedChanges({
                      cwd: sourceWorktree.path,
                      stashOid,
                    })
                  }
                : undefined,
          })
          await runGitCommand({
            cwd: repoRoot,
            args: ["checkout", "--ignore-other-worktrees", branch],
          })

          if (stashOid !== null) {
            const applyResult = await runGitCommand({
              cwd: repoRoot,
              args: ["stash", "apply", stashOid],
              reject: false,
            })
            if (applyResult.exitCode !== 0) {
              throw createCliError("STASH_APPLY_FAILED", {
                message: "Failed to apply stash to primary worktree",
                details: { stashOid, branch, sourcePath: sourceWorktree.path, path: repoRoot },
              })
            }
            if (!keepStash) {
              await dropStashByOid({
                cwd: repoRoot,
                stashOid,
              })
            }
          }

          await runPostHook({ name: "absorb", context: hookContext })
          const stashOutputRef =
            keepStash && stashOid !== null
              ? ((await resolveStashRefByOid({ cwd: repoRoot, stashOid })) ?? stashOid)
              : null
          return {
            branch,
            path: repoRoot,
            sourcePath: sourceWorktree.path,
            stashed: sourceDirty,
            stashRef: stashOutputRef,
          }
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: result,
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        stdout(result.path)
        return EXIT_CODE.OK
      }

      const handleUnabsorb = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const branch = commandArgs[0] as string
        const targetWorktreeName = typeof parsedArgs.to === "string" ? parsedArgs.to : undefined
        const keepStash = parsedArgs.keepStash === true
        if (runtime.isInteractive !== true) {
          if (parsedArgs.allowAgent !== true) {
            throw createCliError("UNSAFE_FLAG_REQUIRED", {
              message: "UNSAFE_FLAG_REQUIRED: unabsorb in non-TTY requires --allow-agent",
            })
          }
          ensureUnsafeForNonTty({
            runtime,
            reason: "unabsorb in non-TTY mode requires --allow-unsafe",
          })
        }

        const result = await runWriteOperation(async () => {
          const currentBranchResult = await runGitCommand({
            cwd: repoRoot,
            args: ["branch", "--show-current"],
            reject: false,
          })
          const currentBranch = currentBranchResult.stdout.trim()
          if (currentBranch !== branch) {
            throw createCliError("INVALID_ARGUMENT", {
              message: "unabsorb requires primary worktree to be on the target branch",
              details: { branch, currentBranch },
            })
          }

          const primaryStatus = await runGitCommand({
            cwd: repoRoot,
            args: ["status", "--porcelain"],
            reject: false,
          })
          if (primaryStatus.stdout.trim().length === 0) {
            throw createCliError("DIRTY_WORKTREE", {
              message: "unabsorb requires dirty primary worktree",
              details: { repoRoot },
            })
          }

          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const targetWorktree = resolveManagedNonPrimaryWorktreeByBranch({
            repoRoot,
            managedWorktreeRoot,
            branch,
            worktrees: snapshot.worktrees,
            optionName: "--to",
            worktreeName: targetWorktreeName,
            role: "target",
          })
          const targetStatus = await runGitCommand({
            cwd: targetWorktree.path,
            args: ["status", "--porcelain"],
            reject: false,
          })
          if (targetStatus.stdout.trim().length > 0) {
            throw createCliError("DIRTY_WORKTREE", {
              message: "unabsorb requires clean target worktree",
              details: { branch, path: targetWorktree.path },
            })
          }

          const stashOid = await createStashEntry({
            cwd: repoRoot,
            message: `vde-worktree unabsorb ${branch}`,
          })

          const hookContext = createHookContext({
            runtime,
            repoRoot,
            action: "unabsorb",
            branch,
            worktreePath: targetWorktree.path,
            stderr,
            extraEnv: {
              WT_SOURCE_WORKTREE_PATH: repoRoot,
              WT_TARGET_WORKTREE_PATH: targetWorktree.path,
            },
          })
          await runPreHookWithAutoRestore({
            name: "unabsorb",
            context: hookContext,
            restore: async (): Promise<void> => {
              await restoreStashedChanges({
                cwd: repoRoot,
                stashOid,
              })
            },
          })

          const applyResult = await runGitCommand({
            cwd: targetWorktree.path,
            args: ["stash", "apply", stashOid],
            reject: false,
          })
          if (applyResult.exitCode !== 0) {
            throw createCliError("STASH_APPLY_FAILED", {
              message: "Failed to apply stash to target worktree",
              details: { stashOid, branch, sourcePath: repoRoot, targetPath: targetWorktree.path },
            })
          }
          if (!keepStash) {
            await dropStashByOid({
              cwd: repoRoot,
              stashOid,
            })
          }

          await runPostHook({ name: "unabsorb", context: hookContext })
          const stashOutputRef = keepStash
            ? ((await resolveStashRefByOid({ cwd: repoRoot, stashOid })) ?? stashOid)
            : null
          return {
            branch,
            path: targetWorktree.path,
            sourcePath: repoRoot,
            stashed: true,
            stashRef: stashOutputRef,
          }
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: result,
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        stdout(result.path)
        return EXIT_CODE.OK
      }

      const handleUse = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const branch = commandArgs[0] as string
        const allowShared = parsedArgs.allowShared === true
        if (runtime.isInteractive !== true) {
          if (parsedArgs.allowAgent !== true) {
            throw createCliError("UNSAFE_FLAG_REQUIRED", {
              message: "UNSAFE_FLAG_REQUIRED: use in non-TTY requires --allow-agent",
            })
          }
          ensureUnsafeForNonTty({
            runtime,
            reason: "use in non-TTY mode requires --allow-unsafe",
          })
        }

        const result = await runWriteOperation(async () => {
          const status = await runGitCommand({
            cwd: repoRoot,
            args: ["status", "--porcelain"],
            reject: false,
          })
          if (status.stdout.trim().length > 0) {
            throw createCliError("DIRTY_WORKTREE", {
              message: "use requires clean primary worktree",
              details: { repoRoot },
            })
          }

          const snapshot = await collectWorktreeSnapshot(repoRoot)
          const branchCheckedOutInOtherWorktree = snapshot.worktrees.find((worktree) => {
            return worktree.branch === branch && worktree.path !== repoRoot
          })
          if (branchCheckedOutInOtherWorktree !== undefined && allowShared !== true) {
            throw createCliError("BRANCH_IN_USE", {
              message: [
                `branch '${branch}' is already checked out in another worktree.`,
                `  path: ${branchCheckedOutInOtherWorktree.path}`,
                "",
                "To continue (unsafe), re-run with:",
                `  vw use ${branch} --allow-shared`,
                "",
                "Risk:",
                "  multiple worktrees will share the same branch.",
              ].join("\n"),
              details: {
                branch,
                path: branchCheckedOutInOtherWorktree.path,
                hint: "re-run with --allow-shared to continue",
                risk: "unsafe: multiple worktrees will share the same branch",
              },
            })
          }
          if (branchCheckedOutInOtherWorktree !== undefined && allowShared === true) {
            stderr(
              [
                "warning: --allow-shared enabled.",
                `  branch: ${branch}`,
                `  path: ${branchCheckedOutInOtherWorktree.path}`,
                "  risk (unsafe): multiple worktrees will share the same branch.",
              ].join("\n"),
            )
          }
          const checkoutArgs = branchCheckedOutInOtherWorktree
            ? ["checkout", "--ignore-other-worktrees", branch]
            : ["checkout", branch]

          const hookContext = createHookContext({
            runtime,
            repoRoot,
            action: "use",
            branch,
            worktreePath: repoRoot,
            stderr,
          })
          await runPreHook({ name: "use", context: hookContext })
          await runGitCommand({
            cwd: repoRoot,
            args: checkoutArgs,
          })
          await runPostHook({ name: "use", context: hookContext })
          return {
            branch,
            path: repoRoot,
          }
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: result,
              }),
            ),
          )
          return EXIT_CODE.OK
        }

        stdout(result.path)
        return EXIT_CODE.OK
      }

      const synchronizationExitCode = await dispatchCommandHandler({
        command,
        handlers: createSynchronizationHandlers({
          absorbHandler: handleAbsorb,
          unabsorbHandler: handleUnabsorb,
          useHandler: handleUse,
        }),
      })
      if (synchronizationExitCode !== undefined) {
        return synchronizationExitCode
      }

      const handleExec = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        ensureHasCommandAfterDoubleDash({
          command,
          argsAfterDoubleDash: afterDoubleDash,
        })
        const branch = commandArgs[0] as string
        const snapshot = await collectWorktreeSnapshot(repoRoot)
        const target = resolveTargetWorktreeByBranch({ branch, worktrees: snapshot.worktrees })
        const executable = afterDoubleDash[0]
        if (typeof executable !== "string" || executable.length === 0) {
          throw createCliError("INVALID_ARGUMENT", {
            message: "exec requires executable after --",
          })
        }

        const child = await execa(executable, afterDoubleDash.slice(1), {
          cwd: target.path,
          stdin: "inherit",
          stdout: "inherit",
          stderr: "inherit",
          reject: false,
        })
        const childExitCode = child.exitCode ?? 0
        if (runtime.json) {
          if (childExitCode === 0) {
            stdout(
              JSON.stringify(
                buildJsonSuccess({
                  command,
                  status: "ok",
                  repoRoot,
                  details: {
                    branch,
                    path: target.path,
                    childExitCode,
                  },
                }),
              ),
            )
            return EXIT_CODE.OK
          }

          stdout(
            JSON.stringify({
              schemaVersion: SCHEMA_VERSION,
              command,
              status: "error",
              repoRoot,
              code: "CHILD_PROCESS_FAILED",
              message: "target command exited with non-zero status",
              details: {
                branch,
                path: target.path,
                childExitCode,
              },
            }),
          )
          return EXIT_CODE.CHILD_PROCESS_FAILED
        }
        return childExitCode === 0 ? EXIT_CODE.OK : EXIT_CODE.CHILD_PROCESS_FAILED
      }

      const handleInvoke = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const hookName = normalizeHookName(commandArgs[0] as string)
        const snapshot = await collectWorktreeSnapshot(repoRoot)
        const current = resolveCurrentWorktree({
          snapshot,
          currentWorktreeRoot: repoContext.currentWorktreeRoot,
        })
        const hookContext = createHookContext({
          runtime,
          repoRoot,
          action: `invoke:${hookName}`,
          branch: current.branch,
          worktreePath: current.path,
          stderr,
        })
        await invokeHook({
          hookName,
          args: afterDoubleDash,
          context: hookContext,
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  hook: hookName,
                  exitCode: 0,
                },
              }),
            ),
          )
        }
        return EXIT_CODE.OK
      }

      const handleCopy = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: Number.MAX_SAFE_INTEGER })
        const snapshot = await collectWorktreeSnapshot(repoRoot)
        const targetWorktreeRoot = resolveTargetWorktreeRootForCopyLink({
          repoContext,
          snapshot,
        })

        for (const relativePath of commandArgs) {
          const { sourcePath, destinationPath } = resolveFileCopyTargets({
            repoRoot,
            targetWorktreeRoot,
            relativePath,
          })
          await access(sourcePath, fsConstants.F_OK)
          await mkdir(dirname(destinationPath), { recursive: true })
          await cp(sourcePath, destinationPath, {
            recursive: true,
            force: true,
            errorOnExist: false,
            dereference: false,
          })
        }

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  copied: commandArgs,
                  worktreePath: targetWorktreeRoot,
                },
              }),
            ),
          )
        }
        return EXIT_CODE.OK
      }

      const handleLink = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: Number.MAX_SAFE_INTEGER })
        const snapshot = await collectWorktreeSnapshot(repoRoot)
        const targetWorktreeRoot = resolveTargetWorktreeRootForCopyLink({
          repoContext,
          snapshot,
        })
        const fallbackEnabled = parsedArgs.fallback !== false

        for (const relativePath of commandArgs) {
          const { sourcePath, destinationPath } = resolveFileCopyTargets({
            repoRoot,
            targetWorktreeRoot,
            relativePath,
          })
          await access(sourcePath, fsConstants.F_OK)
          await rm(destinationPath, { recursive: true, force: true })
          await mkdir(dirname(destinationPath), { recursive: true })

          try {
            await symlink(resolveLinkTargetPath({ sourcePath, destinationPath }), destinationPath)
          } catch (error) {
            if (process.platform === "win32" && fallbackEnabled) {
              stderr(`symlink failed for ${relativePath}; fallback to copy`)
              await cp(sourcePath, destinationPath, {
                recursive: true,
                force: true,
                errorOnExist: false,
                dereference: false,
              })
              continue
            }
            throw createCliError("INVALID_ARGUMENT", {
              message: `Failed to create symlink for ${relativePath}`,
              cause: error,
            })
          }
        }

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  linked: commandArgs,
                  worktreePath: targetWorktreeRoot,
                  fallback: fallbackEnabled,
                },
              }),
            ),
          )
        }
        return EXIT_CODE.OK
      }

      const handleLock = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const branch = commandArgs[0] as string
        const ownerOption = readStringOption(parsedArgsRecord, "owner")
        const reasonOption = readStringOption(parsedArgsRecord, "reason")
        const owner = typeof ownerOption === "string" && ownerOption.length > 0 ? ownerOption : defaultOwner()
        const reason = typeof reasonOption === "string" && reasonOption.length > 0 ? reasonOption : "locked"

        const result = await runWriteOperation(async () => {
          const snapshot = await collectWorktreeSnapshot(repoRoot)
          resolveTargetWorktreeByBranch({ branch, worktrees: snapshot.worktrees })
          const existing = await readWorktreeLock({ repoRoot, branch })
          if (existing.exists && existing.valid !== true) {
            throw createCliError("LOCK_CONFLICT", {
              message: "Cannot update lock with invalid metadata; fix or remove lock file first",
              details: { branch, path: existing.path },
            })
          }
          if (existing.record !== null && existing.record.owner !== owner) {
            throw createCliError("LOCK_CONFLICT", {
              message: "Lock is owned by another owner",
              details: { branch, owner: existing.record.owner },
            })
          }
          const lock = await upsertWorktreeLock({
            repoRoot,
            branch,
            reason,
            owner,
          })
          return lock
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  branch,
                  locked: {
                    value: true,
                    reason: result.reason,
                    owner: result.owner,
                  },
                },
              }),
            ),
          )
        }
        return EXIT_CODE.OK
      }

      const handleUnlock = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 1, max: 1 })
        const branch = commandArgs[0] as string
        const ownerOption = readStringOption(parsedArgsRecord, "owner")
        const owner = typeof ownerOption === "string" && ownerOption.length > 0 ? ownerOption : defaultOwner()
        const force = parsedArgs.force === true

        await runWriteOperation(async () => {
          const existing = await readWorktreeLock({ repoRoot, branch })
          if (existing.exists !== true) {
            return
          }
          if (existing.valid !== true) {
            if (force) {
              await deleteWorktreeLock({ repoRoot, branch })
              return
            }
            throw createCliError("LOCK_CONFLICT", {
              message: "Lock metadata is invalid; use --force to unlock",
              details: { branch, path: existing.path },
            })
          }
          if (existing.record !== null && existing.record.owner !== owner && force !== true) {
            throw createCliError("LOCK_CONFLICT", {
              message: "Lock is owned by another owner. Use --force to unlock.",
              details: { branch, owner: existing.record.owner },
            })
          }
          await deleteWorktreeLock({ repoRoot, branch })
        })

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  branch,
                  locked: {
                    value: false,
                    reason: null,
                  },
                },
              }),
            ),
          )
        }
        return EXIT_CODE.OK
      }

      const handleCd = async (): Promise<number> => {
        ensureArgumentCount({ command, args: commandArgs, min: 0, max: 0 })
        const snapshot = await collectWorktreeSnapshot(repoRoot)
        const theme = createCatppuccinTheme({
          enabled: shouldUseAnsiColors({ interactive: runtime.isInteractive || process.stderr.isTTY === true }),
        })
        const branchColumnWidth = snapshot.worktrees.reduce((maxWidth, worktree) => {
          const label = buildCdBranchLabel({
            worktree,
            currentWorktreeRoot: repoContext.currentWorktreeRoot,
          })
          return Math.max(maxWidth, stringWidth(label))
        }, 0)
        const candidates = snapshot.worktrees.map((worktree) =>
          buildCdCandidateLine({
            worktree,
            baseBranch: snapshot.baseBranch,
            theme,
            currentWorktreeRoot: repoContext.currentWorktreeRoot,
            branchColumnWidth,
          }),
        )
        if (candidates.length === 0) {
          throw createCliError("WORKTREE_NOT_FOUND", {
            message: "No worktree candidates found",
          })
        }

        const promptValue = readStringOption(parsedArgsRecord, "prompt")
        const prompt =
          typeof promptValue === "string" && promptValue.length > 0 ? promptValue : resolvedConfig.selector.cd.prompt
        const cliFzfExtraArgs = collectOptionValues({
          args: beforeDoubleDash,
          optionNames: ["fzfArg", "fzf-arg"],
        })
        const mergedConfigFzfArgs = mergeFzfArgs({
          defaults: resolvedConfig.selector.cd.fzf.extraArgs,
          extras: cliFzfExtraArgs,
        })
        const surface: SelectorCdSurface = resolvedConfig.selector.cd.surface

        const selection = await selectPathWithFzf({
          candidates,
          prompt,
          fzfExtraArgs: mergeFzfArgs({
            defaults: CD_FZF_EXTRA_ARGS,
            extras: mergedConfigFzfArgs,
          }),
          surface,
          tmuxPopupOpts: resolvedConfig.selector.cd.tmuxPopupOpts,
          cwd: repoRoot,
          isInteractive: () => runtime.isInteractive || process.stderr.isTTY === true,
        }).catch((error: unknown) => {
          if (error instanceof FzfDependencyError || error instanceof FzfInteractiveRequiredError) {
            throw createCliError("DEPENDENCY_MISSING", {
              message: `DEPENDENCY_MISSING: ${error.message}`,
            })
          }
          throw error
        })

        if (selection.status === "cancelled") {
          return EXIT_CODE_CANCELLED
        }
        const selectedPath = resolveCdSelectionPath(selection.path)

        if (runtime.json) {
          stdout(
            JSON.stringify(
              buildJsonSuccess({
                command,
                status: "ok",
                repoRoot,
                details: {
                  path: selectedPath,
                },
              }),
            ),
          )
          return EXIT_CODE.OK
        }
        stdout(selectedPath)
        return EXIT_CODE.OK
      }

      const miscCommandExitCode = await dispatchCommandHandler({
        command,
        handlers: createMiscCommandHandlers({
          execHandler: handleExec,
          invokeHandler: handleInvoke,
          copyHandler: handleCopy,
          linkHandler: handleLink,
          lockHandler: handleLock,
          unlockHandler: handleUnlock,
          cdHandler: handleCd,
        }),
      })
      if (miscCommandExitCode !== undefined) {
        return miscCommandExitCode
      }

      throw createCliError("UNKNOWN_COMMAND", {
        message: `Unknown command: ${command}`,
      })
    } catch (error) {
      const cliError = ensureCliError(error)
      if (jsonEnabled) {
        stdout(
          JSON.stringify(
            buildJsonError({
              command,
              repoRoot: repoRootForJson,
              error: cliError,
            }),
          ),
        )
      } else {
        stderr(`[${cliError.code}] ${cliError.message}`)
        logger.debug(JSON.stringify(cliError.details))
      }
      return cliError.exitCode
    }
  }

  return { run }
}
