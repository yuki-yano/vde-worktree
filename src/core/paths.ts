import { createHash } from "node:crypto"
import { dirname, isAbsolute, join, normalize, relative, resolve, sep } from "node:path"
import { runGitCommand } from "../git/exec"
import { createCliError } from "./errors"

export type RepoContext = {
  readonly repoRoot: string
  readonly currentWorktreeRoot: string
  readonly gitCommonDir: string
}

const GIT_DIR_NAME = ".git"
const DEFAULT_WORKTREE_ROOT = ".worktree"
const WORKTREE_ID_HASH_LENGTH = 12
const WORKTREE_ID_SLUG_MAX_LENGTH = 48

const resolveRepoRootFromCommonDir = ({
  currentWorktreeRoot,
  gitCommonDir,
}: {
  readonly currentWorktreeRoot: string
  readonly gitCommonDir: string
}): string => {
  if (gitCommonDir.endsWith(`/${GIT_DIR_NAME}`)) {
    return dirname(gitCommonDir)
  }

  if (gitCommonDir.endsWith(`\\${GIT_DIR_NAME}`)) {
    return dirname(gitCommonDir)
  }

  return currentWorktreeRoot
}

export const resolveRepoContext = async (cwd: string): Promise<RepoContext> => {
  const toplevelResult = await runGitCommand({
    cwd,
    args: ["rev-parse", "--show-toplevel"],
    reject: false,
  })

  if (toplevelResult.exitCode !== 0) {
    throw createCliError("NOT_GIT_REPOSITORY", {
      message: "Current directory is not inside a Git repository",
      details: { cwd },
    })
  }

  const currentWorktreeRoot = toplevelResult.stdout.trim()
  const commonDirResult = await runGitCommand({
    cwd,
    args: ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    reject: false,
  })
  const gitCommonDir =
    commonDirResult.exitCode === 0 ? commonDirResult.stdout.trim() : join(currentWorktreeRoot, GIT_DIR_NAME)

  return {
    repoRoot: resolveRepoRootFromCommonDir({ currentWorktreeRoot, gitCommonDir }),
    currentWorktreeRoot,
    gitCommonDir,
  }
}

export const getWorktreeRootPath = (
  repoRoot: string,
  configuredWorktreeRoot: string = DEFAULT_WORKTREE_ROOT,
): string => {
  if (isAbsolute(configuredWorktreeRoot)) {
    return resolve(configuredWorktreeRoot)
  }
  return resolve(repoRoot, configuredWorktreeRoot)
}

export const getWorktreeMetaRootPath = (repoRoot: string): string => {
  return join(repoRoot, ".vde", "worktree")
}

export const getHooksDirectoryPath = (repoRoot: string): string => {
  return join(getWorktreeMetaRootPath(repoRoot), "hooks")
}

export const getLogsDirectoryPath = (repoRoot: string): string => {
  return join(getWorktreeMetaRootPath(repoRoot), "logs")
}

export const getLocksDirectoryPath = (repoRoot: string): string => {
  return join(getWorktreeMetaRootPath(repoRoot), "locks")
}

export const getStateDirectoryPath = (repoRoot: string): string => {
  return join(getWorktreeMetaRootPath(repoRoot), "state")
}

export const branchToWorktreeId = (branch: string): string => {
  const slug =
    branch
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, WORKTREE_ID_SLUG_MAX_LENGTH) || "branch"
  const hash = createHash("sha256").update(branch).digest("hex").slice(0, WORKTREE_ID_HASH_LENGTH)
  return `${slug}--${hash}`
}

export const branchToWorktreePath = (
  repoRoot: string,
  branch: string,
  configuredWorktreeRoot: string = DEFAULT_WORKTREE_ROOT,
): string => {
  const worktreeRoot = getWorktreeRootPath(repoRoot, configuredWorktreeRoot)
  const targetPath = join(worktreeRoot, ...branch.split("/"))
  return ensurePathInsideRoot({
    rootPath: worktreeRoot,
    path: targetPath,
    message: "Path is outside managed worktree root",
  })
}

export const ensurePathInsideRoot = ({
  rootPath,
  path,
  message = "Path is outside allowed root",
}: {
  readonly rootPath: string
  readonly path: string
  readonly message?: string
}): string => {
  const rel = relative(rootPath, path)
  if (rel === "") {
    return path
  }
  if (rel === ".." || rel.startsWith(`..${sep}`)) {
    throw createCliError("PATH_OUTSIDE_REPO", {
      message,
      details: { rootPath, path },
    })
  }
  return path
}

export const ensurePathInsideRepo = ({
  repoRoot,
  path,
}: {
  readonly repoRoot: string
  readonly path: string
}): string => {
  return ensurePathInsideRoot({
    rootPath: repoRoot,
    path,
    message: "Path is outside repository root",
  })
}

export const resolveRepoRelativePath = ({
  repoRoot,
  relativePath,
}: {
  readonly repoRoot: string
  readonly relativePath: string
}): string => {
  if (isAbsolute(relativePath)) {
    throw createCliError("ABSOLUTE_PATH_NOT_ALLOWED", {
      message: "Absolute path is not allowed",
      details: { path: relativePath },
    })
  }
  const normalizedRelative = normalize(relativePath)
  const resolved = resolve(repoRoot, normalizedRelative)
  return ensurePathInsideRepo({
    repoRoot,
    path: resolved,
  })
}

export const resolvePathFromCwd = ({ cwd, path }: { readonly cwd: string; readonly path: string }): string => {
  if (isAbsolute(path)) {
    return path
  }
  return resolve(cwd, path)
}

export const isManagedWorktreePath = ({
  worktreePath,
  managedWorktreeRoot,
}: {
  readonly worktreePath: string
  readonly managedWorktreeRoot: string
}): boolean => {
  const rel = relative(managedWorktreeRoot, worktreePath)
  if (rel === "" || rel === "." || rel === "..") {
    return false
  }
  return rel.startsWith(`..${sep}`) !== true
}
