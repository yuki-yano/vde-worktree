import { constants as fsConstants } from "node:fs"
import { access, readFile } from "node:fs/promises"
import { join } from "node:path"
import { doesGitRefExist, runGitCommand } from "../git/exec"
import { resolveMergedByPrBatch } from "../integrations/gh"
import { type GitWorktree, listGitWorktrees } from "../git/worktree"
import { branchToWorktreeId, getLocksDirectoryPath } from "./paths"
import { upsertWorktreeMergeLifecycle } from "./worktree-merge-lifecycle"

type LockPayload = {
  readonly schemaVersion: 1
  readonly branch: string
  readonly worktreeId: string
  readonly reason: string
  readonly owner?: string
}

export type WorktreeLockState = {
  readonly value: boolean
  readonly reason: string | null
  readonly owner: string | null
}

export type WorktreeMergedState = {
  readonly byAncestry: boolean | null
  readonly byPR: boolean | null
  readonly overall: boolean | null
}

export type WorktreeUpstreamState = {
  readonly ahead: number | null
  readonly behind: number | null
  readonly remote: string | null
}

export type WorktreeStatus = {
  readonly branch: string | null
  readonly path: string
  readonly head: string
  readonly dirty: boolean
  readonly locked: WorktreeLockState
  readonly merged: WorktreeMergedState
  readonly upstream: WorktreeUpstreamState
}

const resolveBaseBranch = async (repoRoot: string): Promise<string | null> => {
  const explicit = await runGitCommand({
    cwd: repoRoot,
    args: ["config", "--get", "vde-worktree.baseBranch"],
    reject: false,
  })
  if (explicit.exitCode === 0 && explicit.stdout.trim().length > 0) {
    return explicit.stdout.trim()
  }

  for (const candidate of ["main", "master"]) {
    if (await doesGitRefExist(repoRoot, `refs/heads/${candidate}`)) {
      return candidate
    }
  }
  return null
}

const resolveEnableGh = async (repoRoot: string): Promise<boolean> => {
  const result = await runGitCommand({
    cwd: repoRoot,
    args: ["config", "--bool", "--get", "vde-worktree.enableGh"],
    reject: false,
  })
  if (result.exitCode !== 0) {
    return true
  }
  const value = result.stdout.trim().toLowerCase()
  if (value === "false" || value === "no" || value === "off" || value === "0") {
    return false
  }
  return true
}

const resolveDirty = async (worktreePath: string): Promise<boolean> => {
  const status = await runGitCommand({
    cwd: worktreePath,
    args: ["status", "--porcelain"],
    reject: false,
  })
  return status.stdout.trim().length > 0
}

const parseLockPayload = (content: string): LockPayload | null => {
  try {
    const parsed = JSON.parse(content) as Partial<LockPayload>
    if (parsed.schemaVersion !== 1) {
      return null
    }
    if (
      typeof parsed.branch !== "string" ||
      typeof parsed.worktreeId !== "string" ||
      typeof parsed.reason !== "string" ||
      parsed.reason.length === 0
    ) {
      return null
    }
    return parsed as LockPayload
  } catch {
    return null
  }
}

const resolveLockState = async ({
  repoRoot,
  branch,
}: {
  readonly repoRoot: string
  readonly branch: string | null
}): Promise<WorktreeLockState> => {
  if (branch === null) {
    return { value: false, reason: null, owner: null }
  }

  const id = branchToWorktreeId(branch)
  const lockPath = join(getLocksDirectoryPath(repoRoot), `${id}.json`)
  try {
    await access(lockPath, fsConstants.F_OK)
  } catch {
    return { value: false, reason: null, owner: null }
  }

  try {
    const content = await readFile(lockPath, "utf8")
    const lock = parseLockPayload(content)
    if (lock === null) {
      return {
        value: true,
        reason: "invalid lock metadata",
        owner: null,
      }
    }
    return {
      value: true,
      reason: lock.reason,
      owner: typeof lock.owner === "string" && lock.owner.length > 0 ? lock.owner : null,
    }
  } catch {
    return {
      value: true,
      reason: "invalid lock metadata",
      owner: null,
    }
  }
}

const WORK_REFLOG_MESSAGE_PATTERN = /^(commit(?: \([^)]*\))?|cherry-pick|revert|rebase \(pick\)|merge):/

const resolveLifecycleFromReflog = async ({
  repoRoot,
  branch,
  baseBranch,
}: {
  readonly repoRoot: string
  readonly branch: string
  readonly baseBranch: string
}): Promise<{ merged: boolean | null; divergedHead: string | null }> => {
  const reflog = await runGitCommand({
    cwd: repoRoot,
    args: ["reflog", "show", "--format=%H%x09%gs", branch],
    reject: false,
  })
  if (reflog.exitCode !== 0) {
    return {
      merged: null,
      divergedHead: null,
    }
  }

  let latestWorkHead: string | null = null
  for (const line of reflog.stdout.split("\n")) {
    const trimmed = line.trim()
    if (trimmed.length === 0) {
      continue
    }

    const separatorIndex = trimmed.indexOf("\t")
    if (separatorIndex <= 0) {
      continue
    }

    const head = trimmed.slice(0, separatorIndex).trim()
    const message = trimmed.slice(separatorIndex + 1).trim()
    if (head.length === 0 || WORK_REFLOG_MESSAGE_PATTERN.test(message) !== true) {
      continue
    }
    if (latestWorkHead === null) {
      latestWorkHead = head
    }

    const result = await runGitCommand({
      cwd: repoRoot,
      args: ["merge-base", "--is-ancestor", head, baseBranch],
      reject: false,
    })
    if (result.exitCode === 0) {
      return {
        merged: true,
        divergedHead: head,
      }
    }
    if (result.exitCode !== 1) {
      return {
        merged: null,
        divergedHead: latestWorkHead,
      }
    }
  }

  return {
    merged: false,
    divergedHead: latestWorkHead,
  }
}

const resolveMergedState = async ({
  repoRoot,
  branch,
  head,
  baseBranch,
  mergedByPrByBranch,
}: {
  readonly repoRoot: string
  readonly branch: string | null
  readonly head: string
  readonly baseBranch: string | null
  readonly mergedByPrByBranch: ReadonlyMap<string, boolean | null>
}): Promise<WorktreeMergedState> => {
  if (branch === null) {
    return { byAncestry: null, byPR: null, overall: null }
  }

  let byAncestry: boolean | null = null
  if (baseBranch !== null) {
    const result = await runGitCommand({
      cwd: repoRoot,
      args: ["merge-base", "--is-ancestor", branch, baseBranch],
      reject: false,
    })

    if (result.exitCode === 0) {
      byAncestry = true
    } else if (result.exitCode === 1) {
      byAncestry = false
    }
  }

  const byPR = branch === baseBranch ? null : (mergedByPrByBranch.get(branch) ?? null)

  let byLifecycle: boolean | null = null
  if (baseBranch !== null) {
    const lifecycle = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch,
      baseBranch,
      observedDivergedHead: byAncestry === false ? head : null,
    })
    if (byAncestry === false) {
      byLifecycle = false
    } else if (byAncestry === true) {
      if (lifecycle.everDiverged !== true || lifecycle.lastDivergedHead === null) {
        if (byPR === true) {
          byLifecycle = null
        } else {
          const probe = await resolveLifecycleFromReflog({
            repoRoot,
            branch,
            baseBranch,
          })
          byLifecycle = probe.merged
          if (probe.divergedHead !== null) {
            await upsertWorktreeMergeLifecycle({
              repoRoot,
              branch,
              baseBranch,
              observedDivergedHead: probe.divergedHead,
            })
          }
        }
      } else {
        const lifecycleResult = await runGitCommand({
          cwd: repoRoot,
          args: ["merge-base", "--is-ancestor", lifecycle.lastDivergedHead, baseBranch],
          reject: false,
        })
        if (lifecycleResult.exitCode === 0) {
          byLifecycle = true
        } else if (lifecycleResult.exitCode === 1) {
          byLifecycle = false
        } else {
          byLifecycle = null
        }
      }
    }
  }

  return {
    byAncestry,
    byPR,
    overall: resolveMergedOverall({
      byAncestry,
      byPR,
      byLifecycle,
    }),
  }
}

export const resolveMergedOverall = ({
  byAncestry,
  byPR,
  byLifecycle,
}: {
  readonly byAncestry: boolean | null
  readonly byPR: boolean | null
  readonly byLifecycle: boolean | null
}): boolean | null => {
  if (byPR === true || byLifecycle === true) {
    return true
  }
  if (byAncestry === false) {
    return false
  }
  if (byPR === false || byLifecycle === false) {
    return false
  }
  return null
}

const resolveUpstreamState = async (worktreePath: string): Promise<WorktreeUpstreamState> => {
  const upstreamRef = await runGitCommand({
    cwd: worktreePath,
    args: ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"],
    reject: false,
  })
  if (upstreamRef.exitCode !== 0) {
    return {
      ahead: null,
      behind: null,
      remote: null,
    }
  }

  const distance = await runGitCommand({
    cwd: worktreePath,
    args: ["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    reject: false,
  })

  if (distance.exitCode !== 0) {
    return {
      ahead: null,
      behind: null,
      remote: upstreamRef.stdout.trim(),
    }
  }

  const [behindRaw, aheadRaw] = distance.stdout.trim().split(/\s+/)
  const behind = Number.parseInt(behindRaw ?? "", 10)
  const ahead = Number.parseInt(aheadRaw ?? "", 10)
  return {
    ahead: Number.isNaN(ahead) ? null : ahead,
    behind: Number.isNaN(behind) ? null : behind,
    remote: upstreamRef.stdout.trim(),
  }
}

const enrichWorktree = async ({
  repoRoot,
  worktree,
  baseBranch,
  mergedByPrByBranch,
}: {
  readonly repoRoot: string
  readonly worktree: GitWorktree
  readonly baseBranch: string | null
  readonly mergedByPrByBranch: ReadonlyMap<string, boolean | null>
}): Promise<WorktreeStatus> => {
  const [dirty, locked, merged, upstream] = await Promise.all([
    resolveDirty(worktree.path),
    resolveLockState({ repoRoot, branch: worktree.branch }),
    resolveMergedState({ repoRoot, branch: worktree.branch, head: worktree.head, baseBranch, mergedByPrByBranch }),
    resolveUpstreamState(worktree.path),
  ])

  return {
    branch: worktree.branch,
    path: worktree.path,
    head: worktree.head,
    dirty,
    locked,
    merged,
    upstream,
  }
}

export type WorktreeSnapshot = {
  readonly repoRoot: string
  readonly baseBranch: string | null
  readonly worktrees: WorktreeStatus[]
}

type CollectWorktreeSnapshotOptions = {
  readonly noGh?: boolean
}

export const collectWorktreeSnapshot = async (
  repoRoot: string,
  { noGh = false }: CollectWorktreeSnapshotOptions = {},
): Promise<WorktreeSnapshot> => {
  const [baseBranch, worktrees, enableGh] = await Promise.all([
    resolveBaseBranch(repoRoot),
    listGitWorktrees(repoRoot),
    resolveEnableGh(repoRoot),
  ])
  const mergedByPrByBranch = await resolveMergedByPrBatch({
    repoRoot,
    baseBranch,
    branches: worktrees.map((worktree) => worktree.branch),
    enabled: enableGh && noGh !== true,
  })
  const enriched = await Promise.all(
    worktrees.map(async (worktree) => {
      return enrichWorktree({ repoRoot, worktree, baseBranch, mergedByPrByBranch })
    }),
  )

  return {
    repoRoot,
    baseBranch,
    worktrees: enriched,
  }
}
