import { runGitCommand } from "./exec"

export type GitWorktree = {
  readonly path: string
  readonly head: string
  readonly branch: string | null
}

const BRANCH_PREFIX = "refs/heads/"

const parseBranchName = (rawRef: string): string | null => {
  if (rawRef.startsWith(BRANCH_PREFIX)) {
    return rawRef.slice(BRANCH_PREFIX.length)
  }
  return rawRef.length > 0 ? rawRef : null
}

export const parseWorktreePorcelain = (raw: string): GitWorktree[] => {
  const tokens = raw.split("\0")
  const worktrees: GitWorktree[] = []

  let currentPath = ""
  let currentHead = ""
  let currentBranch: string | null = null

  const flush = (): void => {
    if (currentPath.length === 0) {
      return
    }

    worktrees.push({
      path: currentPath,
      head: currentHead,
      branch: currentBranch,
    })
    currentPath = ""
    currentHead = ""
    currentBranch = null
  }

  for (const token of tokens) {
    if (token.length === 0) {
      flush()
      continue
    }

    if (token.startsWith("worktree ")) {
      flush()
      currentPath = token.slice("worktree ".length)
      continue
    }

    if (token.startsWith("HEAD ")) {
      currentHead = token.slice("HEAD ".length)
      continue
    }

    if (token.startsWith("branch ")) {
      currentBranch = parseBranchName(token.slice("branch ".length))
      continue
    }

    if (token === "detached") {
      currentBranch = null
    }
  }

  flush()
  return worktrees
}

export const listGitWorktrees = async (repoRoot: string): Promise<GitWorktree[]> => {
  const result = await runGitCommand({
    cwd: repoRoot,
    args: ["worktree", "list", "--porcelain", "-z"],
  })
  return parseWorktreePorcelain(result.stdout)
}
