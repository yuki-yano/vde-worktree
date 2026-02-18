import { constants as fsConstants } from "node:fs"
import { access, lstat, readFile, realpath } from "node:fs/promises"
import { homedir } from "node:os"
import { isAbsolute, join, relative, resolve, sep } from "node:path"
import { parse } from "yaml"
import { createCliError } from "../core/errors"
import { collectConfigSearchDirectories } from "./git-boundary"
import {
  DEFAULT_CONFIG,
  LIST_PATH_TRUNCATE_VALUES,
  LIST_TABLE_COLUMNS,
  SELECTOR_CD_SURFACE_VALUES,
  type ListPathTruncate,
  type ListTableColumn,
  type PartialConfig,
  type ResolvedConfig,
} from "./types"

const CONFIG_FILE_BASENAME = "config.yml"
const LOCAL_CONFIG_PATH_SEGMENTS = [".vde", "worktree", CONFIG_FILE_BASENAME] as const
const GLOBAL_CONFIG_PATH_SEGMENTS = ["vde", "worktree", CONFIG_FILE_BASENAME] as const

type ValidationContext = {
  readonly file: string
}

type LoadResolvedConfigInput = {
  readonly cwd: string
  readonly repoRoot: string
}

export type LoadResolvedConfigResult = {
  readonly config: ResolvedConfig
  readonly loadedFiles: ReadonlyArray<string>
}

const isRecord = (value: unknown): value is Record<string, unknown> => {
  return value !== null && typeof value === "object" && Array.isArray(value) !== true
}

const toKeyPath = (segments: readonly string[]): string => {
  if (segments.length === 0) {
    return "<root>"
  }
  return segments.join(".")
}

const throwInvalidConfig = ({
  file,
  keyPath,
  reason,
}: {
  readonly file: string
  readonly keyPath: string
  readonly reason: string
}): never => {
  throw createCliError("INVALID_CONFIG", {
    message: `Invalid config: ${file} (${keyPath}: ${reason})`,
    details: {
      file,
      keyPath,
      reason,
    },
  })
}

const expectRecord = ({
  value,
  ctx,
  keyPath,
}: {
  readonly value: unknown
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): Record<string, unknown> => {
  if (isRecord(value)) {
    return value
  }
  return throwInvalidConfig({
    file: ctx.file,
    keyPath: toKeyPath(keyPath),
    reason: "must be an object",
  })
}

const ensureNoUnknownKeys = ({
  record,
  allowedKeys,
  ctx,
  keyPath,
}: {
  readonly record: Record<string, unknown>
  readonly allowedKeys: ReadonlyArray<string>
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): void => {
  const allowed = new Set(allowedKeys)
  for (const key of Object.keys(record)) {
    if (allowed.has(key)) {
      continue
    }
    const path = [...keyPath, key]
    throwInvalidConfig({
      file: ctx.file,
      keyPath: toKeyPath(path),
      reason: "unknown key",
    })
  }
}

const parseBoolean = ({
  value,
  ctx,
  keyPath,
}: {
  readonly value: unknown
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): boolean => {
  if (typeof value === "boolean") {
    return value
  }
  return throwInvalidConfig({
    file: ctx.file,
    keyPath: toKeyPath(keyPath),
    reason: "must be boolean",
  })
}

const parseNonEmptyString = ({
  value,
  ctx,
  keyPath,
}: {
  readonly value: unknown
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): string => {
  if (typeof value === "string" && value.trim().length > 0) {
    return value
  }
  return throwInvalidConfig({
    file: ctx.file,
    keyPath: toKeyPath(keyPath),
    reason: "must be a non-empty string",
  })
}

const parsePositiveInteger = ({
  value,
  ctx,
  keyPath,
}: {
  readonly value: unknown
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): number => {
  if (typeof value === "number" && Number.isInteger(value) && value > 0) {
    return value
  }
  return throwInvalidConfig({
    file: ctx.file,
    keyPath: toKeyPath(keyPath),
    reason: "must be a positive integer",
  })
}

const parseStringArray = ({
  value,
  ctx,
  keyPath,
}: {
  readonly value: unknown
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): string[] => {
  if (Array.isArray(value) !== true) {
    return throwInvalidConfig({
      file: ctx.file,
      keyPath: toKeyPath(keyPath),
      reason: "must be an array",
    })
  }
  const values = value
  const result: string[] = []
  for (const [index, item] of values.entries()) {
    if (typeof item !== "string" || item.length === 0) {
      throwInvalidConfig({
        file: ctx.file,
        keyPath: toKeyPath([...keyPath, String(index)]),
        reason: "must be a non-empty string",
      })
    }
    result.push(item)
  }
  return result
}

const parseColumns = ({
  value,
  ctx,
  keyPath,
}: {
  readonly value: unknown
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): ReadonlyArray<ListTableColumn> => {
  if (Array.isArray(value) !== true) {
    return throwInvalidConfig({
      file: ctx.file,
      keyPath: toKeyPath(keyPath),
      reason: "must be an array",
    })
  }
  const values = value
  if (values.length === 0) {
    return throwInvalidConfig({
      file: ctx.file,
      keyPath: toKeyPath(keyPath),
      reason: "must not be empty",
    })
  }

  const allowed = new Set(LIST_TABLE_COLUMNS)
  const seen = new Set<string>()
  const parsed: ListTableColumn[] = []
  for (const [index, item] of values.entries()) {
    if (typeof item !== "string") {
      throwInvalidConfig({
        file: ctx.file,
        keyPath: toKeyPath([...keyPath, String(index)]),
        reason: "must be a string",
      })
    }
    if (allowed.has(item as ListTableColumn) !== true) {
      throwInvalidConfig({
        file: ctx.file,
        keyPath: toKeyPath([...keyPath, String(index)]),
        reason: `unsupported column: ${item}`,
      })
    }
    if (seen.has(item)) {
      throwInvalidConfig({
        file: ctx.file,
        keyPath: toKeyPath([...keyPath, String(index)]),
        reason: `duplicate column: ${item}`,
      })
    }
    seen.add(item)
    parsed.push(item as ListTableColumn)
  }
  return parsed
}

const parseListPathTruncate = ({
  value,
  ctx,
  keyPath,
}: {
  readonly value: unknown
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): ListPathTruncate => {
  if (typeof value !== "string" || (LIST_PATH_TRUNCATE_VALUES as readonly string[]).includes(value) !== true) {
    throwInvalidConfig({
      file: ctx.file,
      keyPath: toKeyPath(keyPath),
      reason: `must be one of: ${LIST_PATH_TRUNCATE_VALUES.join(", ")}`,
    })
  }
  return value as ListPathTruncate
}

const parseSelectorSurface = ({
  value,
  ctx,
  keyPath,
}: {
  readonly value: unknown
  readonly ctx: ValidationContext
  readonly keyPath: readonly string[]
}): ResolvedConfig["selector"]["cd"]["surface"] => {
  if (typeof value !== "string" || (SELECTOR_CD_SURFACE_VALUES as readonly string[]).includes(value) !== true) {
    throwInvalidConfig({
      file: ctx.file,
      keyPath: toKeyPath(keyPath),
      reason: `must be one of: ${SELECTOR_CD_SURFACE_VALUES.join(", ")}`,
    })
  }
  return value as ResolvedConfig["selector"]["cd"]["surface"]
}

const validatePartialConfig = ({
  rawConfig,
  ctx,
}: {
  readonly rawConfig: unknown
  readonly ctx: ValidationContext
}): PartialConfig => {
  if (rawConfig === null || rawConfig === undefined) {
    return {}
  }
  const root = expectRecord({
    value: rawConfig,
    ctx,
    keyPath: [],
  })

  ensureNoUnknownKeys({
    record: root,
    allowedKeys: ["paths", "git", "github", "hooks", "locks", "list", "selector"],
    ctx,
    keyPath: [],
  })

  const partial: PartialConfig = {}

  if (root.paths !== undefined) {
    const paths = expectRecord({
      value: root.paths,
      ctx,
      keyPath: ["paths"],
    })
    ensureNoUnknownKeys({
      record: paths,
      allowedKeys: ["worktreeRoot"],
      ctx,
      keyPath: ["paths"],
    })
    partial.paths = {}
    if (paths.worktreeRoot !== undefined) {
      partial.paths.worktreeRoot = parseNonEmptyString({
        value: paths.worktreeRoot,
        ctx,
        keyPath: ["paths", "worktreeRoot"],
      })
    }
  }

  if (root.git !== undefined) {
    const git = expectRecord({
      value: root.git,
      ctx,
      keyPath: ["git"],
    })
    ensureNoUnknownKeys({
      record: git,
      allowedKeys: ["baseBranch", "baseRemote"],
      ctx,
      keyPath: ["git"],
    })
    partial.git = {}
    if (git.baseBranch !== undefined) {
      if (git.baseBranch !== null && typeof git.baseBranch !== "string") {
        throwInvalidConfig({
          file: ctx.file,
          keyPath: toKeyPath(["git", "baseBranch"]),
          reason: "must be a string or null",
        })
      }
      partial.git.baseBranch =
        git.baseBranch === null
          ? null
          : parseNonEmptyString({
              value: git.baseBranch,
              ctx,
              keyPath: ["git", "baseBranch"],
            })
    }
    if (git.baseRemote !== undefined) {
      partial.git.baseRemote = parseNonEmptyString({
        value: git.baseRemote,
        ctx,
        keyPath: ["git", "baseRemote"],
      })
    }
  }

  if (root.github !== undefined) {
    const github = expectRecord({
      value: root.github,
      ctx,
      keyPath: ["github"],
    })
    ensureNoUnknownKeys({
      record: github,
      allowedKeys: ["enabled"],
      ctx,
      keyPath: ["github"],
    })
    partial.github = {}
    if (github.enabled !== undefined) {
      partial.github.enabled = parseBoolean({
        value: github.enabled,
        ctx,
        keyPath: ["github", "enabled"],
      })
    }
  }

  if (root.hooks !== undefined) {
    const hooks = expectRecord({
      value: root.hooks,
      ctx,
      keyPath: ["hooks"],
    })
    ensureNoUnknownKeys({
      record: hooks,
      allowedKeys: ["enabled", "timeoutMs"],
      ctx,
      keyPath: ["hooks"],
    })
    partial.hooks = {}
    if (hooks.enabled !== undefined) {
      partial.hooks.enabled = parseBoolean({
        value: hooks.enabled,
        ctx,
        keyPath: ["hooks", "enabled"],
      })
    }
    if (hooks.timeoutMs !== undefined) {
      partial.hooks.timeoutMs = parsePositiveInteger({
        value: hooks.timeoutMs,
        ctx,
        keyPath: ["hooks", "timeoutMs"],
      })
    }
  }

  if (root.locks !== undefined) {
    const locks = expectRecord({
      value: root.locks,
      ctx,
      keyPath: ["locks"],
    })
    ensureNoUnknownKeys({
      record: locks,
      allowedKeys: ["timeoutMs", "staleLockTTLSeconds"],
      ctx,
      keyPath: ["locks"],
    })
    partial.locks = {}
    if (locks.timeoutMs !== undefined) {
      partial.locks.timeoutMs = parsePositiveInteger({
        value: locks.timeoutMs,
        ctx,
        keyPath: ["locks", "timeoutMs"],
      })
    }
    if (locks.staleLockTTLSeconds !== undefined) {
      partial.locks.staleLockTTLSeconds = parsePositiveInteger({
        value: locks.staleLockTTLSeconds,
        ctx,
        keyPath: ["locks", "staleLockTTLSeconds"],
      })
    }
  }

  if (root.list !== undefined) {
    const list = expectRecord({
      value: root.list,
      ctx,
      keyPath: ["list"],
    })
    ensureNoUnknownKeys({
      record: list,
      allowedKeys: ["table"],
      ctx,
      keyPath: ["list"],
    })
    partial.list = {}
    if (list.table !== undefined) {
      const table = expectRecord({
        value: list.table,
        ctx,
        keyPath: ["list", "table"],
      })
      ensureNoUnknownKeys({
        record: table,
        allowedKeys: ["columns", "path"],
        ctx,
        keyPath: ["list", "table"],
      })
      partial.list.table = {}
      if (table.columns !== undefined) {
        partial.list.table.columns = parseColumns({
          value: table.columns,
          ctx,
          keyPath: ["list", "table", "columns"],
        })
      }
      if (table.path !== undefined) {
        const pathConfig = expectRecord({
          value: table.path,
          ctx,
          keyPath: ["list", "table", "path"],
        })
        ensureNoUnknownKeys({
          record: pathConfig,
          allowedKeys: ["truncate", "minWidth"],
          ctx,
          keyPath: ["list", "table", "path"],
        })
        partial.list.table.path = {}
        if (pathConfig.truncate !== undefined) {
          partial.list.table.path.truncate = parseListPathTruncate({
            value: pathConfig.truncate,
            ctx,
            keyPath: ["list", "table", "path", "truncate"],
          })
        }
        if (pathConfig.minWidth !== undefined) {
          const minWidth = parsePositiveInteger({
            value: pathConfig.minWidth,
            ctx,
            keyPath: ["list", "table", "path", "minWidth"],
          })
          if (minWidth < 8 || minWidth > 200) {
            throwInvalidConfig({
              file: ctx.file,
              keyPath: toKeyPath(["list", "table", "path", "minWidth"]),
              reason: "must be in range 8..200",
            })
          }
          partial.list.table.path.minWidth = minWidth
        }
      }
    }
  }

  if (root.selector !== undefined) {
    const selector = expectRecord({
      value: root.selector,
      ctx,
      keyPath: ["selector"],
    })
    ensureNoUnknownKeys({
      record: selector,
      allowedKeys: ["cd"],
      ctx,
      keyPath: ["selector"],
    })
    partial.selector = {}
    if (selector.cd !== undefined) {
      const cd = expectRecord({
        value: selector.cd,
        ctx,
        keyPath: ["selector", "cd"],
      })
      ensureNoUnknownKeys({
        record: cd,
        allowedKeys: ["prompt", "surface", "tmuxPopupOpts", "fzf"],
        ctx,
        keyPath: ["selector", "cd"],
      })
      partial.selector.cd = {}
      if (cd.prompt !== undefined) {
        partial.selector.cd.prompt = parseNonEmptyString({
          value: cd.prompt,
          ctx,
          keyPath: ["selector", "cd", "prompt"],
        })
      }
      if (cd.surface !== undefined) {
        partial.selector.cd.surface = parseSelectorSurface({
          value: cd.surface,
          ctx,
          keyPath: ["selector", "cd", "surface"],
        })
      }
      if (cd.tmuxPopupOpts !== undefined) {
        partial.selector.cd.tmuxPopupOpts = parseNonEmptyString({
          value: cd.tmuxPopupOpts,
          ctx,
          keyPath: ["selector", "cd", "tmuxPopupOpts"],
        })
      }
      if (cd.fzf !== undefined) {
        const fzf = expectRecord({
          value: cd.fzf,
          ctx,
          keyPath: ["selector", "cd", "fzf"],
        })
        ensureNoUnknownKeys({
          record: fzf,
          allowedKeys: ["extraArgs"],
          ctx,
          keyPath: ["selector", "cd", "fzf"],
        })
        partial.selector.cd.fzf = {}
        if (fzf.extraArgs !== undefined) {
          partial.selector.cd.fzf.extraArgs = parseStringArray({
            value: fzf.extraArgs,
            ctx,
            keyPath: ["selector", "cd", "fzf", "extraArgs"],
          })
        }
      }
    }
  }

  return partial
}

const mergeConfig = (base: ResolvedConfig, partial: PartialConfig): ResolvedConfig => {
  return {
    paths: {
      worktreeRoot: partial.paths?.worktreeRoot ?? base.paths.worktreeRoot,
    },
    git: {
      baseBranch: partial.git?.baseBranch === undefined ? base.git.baseBranch : partial.git.baseBranch,
      baseRemote: partial.git?.baseRemote ?? base.git.baseRemote,
    },
    github: {
      enabled: partial.github?.enabled ?? base.github.enabled,
    },
    hooks: {
      enabled: partial.hooks?.enabled ?? base.hooks.enabled,
      timeoutMs: partial.hooks?.timeoutMs ?? base.hooks.timeoutMs,
    },
    locks: {
      timeoutMs: partial.locks?.timeoutMs ?? base.locks.timeoutMs,
      staleLockTTLSeconds: partial.locks?.staleLockTTLSeconds ?? base.locks.staleLockTTLSeconds,
    },
    list: {
      table: {
        columns: partial.list?.table?.columns ? [...partial.list.table.columns] : [...base.list.table.columns],
        path: {
          truncate: partial.list?.table?.path?.truncate ?? base.list.table.path.truncate,
          minWidth: partial.list?.table?.path?.minWidth ?? base.list.table.path.minWidth,
        },
      },
    },
    selector: {
      cd: {
        prompt: partial.selector?.cd?.prompt ?? base.selector.cd.prompt,
        surface: partial.selector?.cd?.surface ?? base.selector.cd.surface,
        tmuxPopupOpts: partial.selector?.cd?.tmuxPopupOpts ?? base.selector.cd.tmuxPopupOpts,
        fzf: {
          extraArgs: partial.selector?.cd?.fzf?.extraArgs
            ? [...partial.selector.cd.fzf.extraArgs]
            : [...base.selector.cd.fzf.extraArgs],
        },
      },
    },
  }
}

const configPathExists = async (filePath: string): Promise<boolean> => {
  try {
    await access(filePath, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

const resolveLocalConfigPath = (directory: string): string => {
  return join(directory, ...LOCAL_CONFIG_PATH_SEGMENTS)
}

const resolveGlobalConfigPath = (): string => {
  const xdgConfigHome = process.env.XDG_CONFIG_HOME
  if (typeof xdgConfigHome === "string" && xdgConfigHome.length > 0) {
    return join(resolve(xdgConfigHome), ...GLOBAL_CONFIG_PATH_SEGMENTS)
  }
  return join(homedir(), ".config", ...GLOBAL_CONFIG_PATH_SEGMENTS)
}

const resolveExistingConfigFiles = async ({
  cwd,
  repoRoot,
}: {
  readonly cwd: string
  readonly repoRoot: string
}): Promise<ReadonlyArray<string>> => {
  const searchDirectories = await collectConfigSearchDirectories(cwd)
  const localCandidates = searchDirectories.map((directory) => resolveLocalConfigPath(directory))
  const repoRootCandidate = resolveLocalConfigPath(repoRoot)
  const globalCandidate = resolveGlobalConfigPath()
  const lowToHighCandidates = [globalCandidate, repoRootCandidate, ...localCandidates]

  const deduped = new Map<string, { path: string; order: number }>()
  for (const [order, candidate] of lowToHighCandidates.entries()) {
    if ((await configPathExists(candidate)) !== true) {
      continue
    }
    const canonical = await realpath(candidate).catch(() => resolve(candidate))
    deduped.set(canonical, {
      path: candidate,
      order,
    })
  }

  return [...deduped.values()].sort((a, b) => a.order - b.order).map((entry) => entry.path)
}

const isPathInsideOrEqual = ({
  parentPath,
  childPath,
}: {
  readonly parentPath: string
  readonly childPath: string
}): boolean => {
  const rel = relative(parentPath, childPath)
  if (rel.length === 0) {
    return true
  }
  return rel !== ".." && rel.startsWith(`..${sep}`) !== true
}

const validateWorktreeRoot = async ({
  repoRoot,
  config,
}: {
  readonly repoRoot: string
  readonly config: ResolvedConfig
}): Promise<void> => {
  const rawWorktreeRoot = config.paths.worktreeRoot
  const resolvedWorktreeRoot = isAbsolute(rawWorktreeRoot)
    ? resolve(rawWorktreeRoot)
    : resolve(repoRoot, rawWorktreeRoot)

  const gitDirPath = resolve(repoRoot, ".git")
  if (
    isPathInsideOrEqual({
      parentPath: gitDirPath,
      childPath: resolvedWorktreeRoot,
    })
  ) {
    throwInvalidConfig({
      file: "<resolved>",
      keyPath: "paths.worktreeRoot",
      reason: "must not point inside .git",
    })
  }

  try {
    const stat = await lstat(resolvedWorktreeRoot)
    if (stat.isDirectory() !== true) {
      throwInvalidConfig({
        file: "<resolved>",
        keyPath: "paths.worktreeRoot",
        reason: "must not point to an existing file",
      })
    }
  } catch (error) {
    const nodeError = error as NodeJS.ErrnoException
    if (nodeError.code === "ENOENT") {
      return
    }
    throw error
  }
}

const parseConfigFile = async (file: string): Promise<PartialConfig> => {
  const rawContent = await readFile(file, "utf8")
  let parsed: unknown
  try {
    parsed = parse(rawContent)
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    throwInvalidConfig({
      file,
      keyPath: "<root>",
      reason: message,
    })
  }
  return validatePartialConfig({
    rawConfig: parsed,
    ctx: { file },
  })
}

const cloneDefaultConfig = (): ResolvedConfig => {
  return mergeConfig(DEFAULT_CONFIG, {})
}

export const loadResolvedConfig = async ({
  cwd,
  repoRoot,
}: LoadResolvedConfigInput): Promise<LoadResolvedConfigResult> => {
  const files = await resolveExistingConfigFiles({ cwd, repoRoot })
  let config = cloneDefaultConfig()
  for (const file of files) {
    const partial = await parseConfigFile(file)
    config = mergeConfig(config, partial)
  }

  await validateWorktreeRoot({
    repoRoot,
    config,
  })

  return {
    config,
    loadedFiles: files,
  }
}
