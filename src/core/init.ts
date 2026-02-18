import { access, chmod, mkdir, readFile, writeFile } from "node:fs/promises"
import { constants as fsConstants } from "node:fs"
import { join, relative, sep } from "node:path"
import {
  getHooksDirectoryPath,
  getLocksDirectoryPath,
  getLogsDirectoryPath,
  getStateDirectoryPath,
  getWorktreeMetaRootPath,
} from "./paths"

const EXCLUDE_MARKER = "# vde-worktree (managed)"

const DEFAULT_HOOKS: ReadonlyArray<{ name: string; lines: string[] }> = [
  {
    name: "post-new",
    lines: [
      "#!/usr/bin/env bash",
      "set -eu",
      "",
      "# example:",
      "#   vde-worktree copy .envrc .claude/settings.local.json",
      "",
      "exit 0",
    ],
  },
  {
    name: "post-switch",
    lines: ["#!/usr/bin/env bash", "set -eu", "", "# example:", "#   vde-worktree link .envrc", "", "exit 0"],
  },
] as const

export type InitResult = {
  readonly alreadyInitialized: boolean
}

const createHookTemplate = async (hooksDir: string, name: string, lines: readonly string[]): Promise<void> => {
  const targetPath = join(hooksDir, name)
  try {
    await access(targetPath, fsConstants.F_OK)
    return
  } catch {
    await writeFile(targetPath, `${lines.join("\n")}\n`, "utf8")
    await chmod(targetPath, 0o755)
  }
}

const isPathInsideOrEqual = ({
  rootPath,
  candidatePath,
}: {
  readonly rootPath: string
  readonly candidatePath: string
}): boolean => {
  const rel = relative(rootPath, candidatePath)
  if (rel.length === 0) {
    return true
  }
  return rel !== ".." && rel.startsWith(`..${sep}`) !== true
}

const toExcludeEntry = ({
  repoRoot,
  managedWorktreeRoot,
}: {
  readonly repoRoot: string
  readonly managedWorktreeRoot: string
}): string | null => {
  if (
    isPathInsideOrEqual({
      rootPath: repoRoot,
      candidatePath: managedWorktreeRoot,
    }) !== true
  ) {
    return null
  }

  const rel = relative(repoRoot, managedWorktreeRoot).split(sep).join("/")
  const normalized = rel.length === 0 ? "." : rel
  return normalized.endsWith("/") ? normalized : `${normalized}/`
}

const ensureExcludeBlock = async ({
  repoRoot,
  managedWorktreeRoot,
}: {
  readonly repoRoot: string
  readonly managedWorktreeRoot: string
}): Promise<void> => {
  const managedEntry = toExcludeEntry({ repoRoot, managedWorktreeRoot })
  if (managedEntry === null) {
    return
  }

  const managedExcludeBlock = `${EXCLUDE_MARKER}\n${managedEntry}\n.vde/worktree/\n`
  const excludePath = join(repoRoot, ".git", "info", "exclude")
  let current = ""
  try {
    current = await readFile(excludePath, "utf8")
  } catch {
    current = ""
  }

  if (current.includes(managedExcludeBlock)) {
    return
  }

  const normalizedCurrent = current.endsWith("\n") || current.length === 0 ? current : `${current}\n`
  await writeFile(excludePath, `${normalizedCurrent}${managedExcludeBlock}`, "utf8")
}

export const isInitialized = async (repoRoot: string): Promise<boolean> => {
  try {
    await access(getWorktreeMetaRootPath(repoRoot), fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

export const initializeRepository = async ({
  repoRoot,
  managedWorktreeRoot,
}: {
  readonly repoRoot: string
  readonly managedWorktreeRoot: string
}): Promise<InitResult> => {
  const wasInitialized = await isInitialized(repoRoot)
  await mkdir(managedWorktreeRoot, { recursive: true })
  await mkdir(getHooksDirectoryPath(repoRoot), { recursive: true })
  await mkdir(getLogsDirectoryPath(repoRoot), { recursive: true })
  await mkdir(getLocksDirectoryPath(repoRoot), { recursive: true })
  await mkdir(getStateDirectoryPath(repoRoot), { recursive: true })
  await ensureExcludeBlock({ repoRoot, managedWorktreeRoot })

  for (const hook of DEFAULT_HOOKS) {
    await createHookTemplate(getHooksDirectoryPath(repoRoot), hook.name, hook.lines)
  }

  return {
    alreadyInitialized: wasInitialized,
  }
}
