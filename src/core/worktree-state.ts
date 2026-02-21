import { join } from "node:path"
import { runGitCommand } from "../git/exec"
import { resolvePrStateByBranchBatch, type PrState, type PrStatus } from "../integrations/gh"
import { type GitWorktree, listGitWorktrees } from "../git/worktree"
import { readJsonRecord } from "./json-storage"
import { branchToWorktreeId, getLocksDirectoryPath } from "./paths"
import { type WorktreeMergeLifecycleRecord, upsertWorktreeMergeLifecycle } from "./worktree-merge-lifecycle"

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

export type WorktreePrState = {
  readonly status: PrStatus | null
  readonly url: string | null
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
  readonly pr: WorktreePrState
  readonly upstream: WorktreeUpstreamState
}

const isLockPayload = (parsed: Partial<LockPayload>): parsed is LockPayload => {
  return (
    typeof parsed.branch === "string" &&
    typeof parsed.worktreeId === "string" &&
    typeof parsed.reason === "string" &&
    parsed.reason.length > 0 &&
    (typeof parsed.owner === "undefined" || typeof parsed.owner === "string")
  )
}

const resolveDirty = async (worktreePath: string): Promise<boolean> => {
  const status = await runGitCommand({
    cwd: worktreePath,
    args: ["status", "--porcelain"],
    reject: false,
  })
  return status.stdout.trim().length > 0
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
  const lock = await readJsonRecord<LockPayload>({
    path: lockPath,
    schemaVersion: 1,
    validate: isLockPayload,
  })
  if (lock.exists !== true) {
    return { value: false, reason: null, owner: null }
  }
  if (lock.valid !== true || lock.record === null) {
    return {
      value: true,
      reason: "invalid lock metadata",
      owner: null,
    }
  }

  return {
    value: true,
    reason: lock.record.reason,
    owner: typeof lock.record.owner === "string" && lock.record.owner.length > 0 ? lock.record.owner : null,
  }
}

const WORK_REFLOG_MESSAGE_PATTERN = /^(commit(?: \([^)]*\))?|cherry-pick|revert|rebase \(pick\)|merge):/

type MergeLifecycleRepository = {
  readonly upsert: (input: {
    readonly branch: string
    readonly baseBranch: string
    readonly observedDivergedHead: string | null
  }) => Promise<WorktreeMergeLifecycleRecord>
}

type MergeProbeRepository = {
  readonly probeAncestry: (input: { readonly branch: string; readonly baseBranch: string }) => Promise<boolean | null>
  readonly probeLifecycleFromReflog: (input: {
    readonly branch: string
    readonly baseBranch: string
  }) => Promise<{ merged: boolean | null; divergedHead: string | null }>
}

const resolveAncestryFromExitCode = (exitCode: number): boolean | null => {
  if (exitCode === 0) {
    return true
  }
  if (exitCode === 1) {
    return false
  }
  return null
}

const resolveMergedByPr = ({
  branch,
  baseBranch,
  prStateByBranch,
}: {
  readonly branch: string
  readonly baseBranch: string | null
  readonly prStateByBranch: ReadonlyMap<string, PrState>
}): boolean | null => {
  const prStatus = branch === baseBranch ? null : (prStateByBranch.get(branch)?.status ?? null)
  if (prStatus === "merged") {
    return true
  }
  if (prStatus === "none" || prStatus === "open" || prStatus === "closed_unmerged") {
    return false
  }
  return null
}

const hasLifecycleDivergedHead = (
  lifecycle: WorktreeMergeLifecycleRecord,
): lifecycle is WorktreeMergeLifecycleRecord & { readonly lastDivergedHead: string } => {
  return lifecycle.everDiverged === true && lifecycle.lastDivergedHead !== null
}

const parseWorkReflogHeads = (
  reflogOutput: string,
): { readonly heads: string[]; readonly latestHead: string | null } => {
  const heads: string[] = []
  let latestHead: string | null = null

  for (const line of reflogOutput.split("\n")) {
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
    if (latestHead === null) {
      latestHead = head
    }
    heads.push(head)
  }

  return {
    heads,
    latestHead,
  }
}

const probeLifecycleFromReflog = async ({
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

  const parsedHeads = parseWorkReflogHeads(reflog.stdout)
  if (parsedHeads.heads.length === 0) {
    return {
      merged: null,
      divergedHead: null,
    }
  }

  for (const head of parsedHeads.heads) {
    const result = await runGitCommand({
      cwd: repoRoot,
      args: ["merge-base", "--is-ancestor", head, baseBranch],
      reject: false,
    })
    const merged = resolveAncestryFromExitCode(result.exitCode)
    if (merged === true) {
      return {
        merged: true,
        divergedHead: head,
      }
    }
    if (merged === null) {
      return {
        merged: null,
        divergedHead: parsedHeads.latestHead,
      }
    }
  }

  return {
    merged: false,
    divergedHead: parsedHeads.latestHead,
  }
}

const createMergeLifecycleRepository = ({ repoRoot }: { readonly repoRoot: string }): MergeLifecycleRepository => {
  return {
    upsert: async ({ branch, baseBranch, observedDivergedHead }): Promise<WorktreeMergeLifecycleRecord> => {
      return upsertWorktreeMergeLifecycle({
        repoRoot,
        branch,
        baseBranch,
        observedDivergedHead,
      })
    },
  }
}

const createMergeProbeRepository = ({ repoRoot }: { readonly repoRoot: string }): MergeProbeRepository => {
  return {
    probeAncestry: async ({ branch, baseBranch }): Promise<boolean | null> => {
      const result = await runGitCommand({
        cwd: repoRoot,
        args: ["merge-base", "--is-ancestor", branch, baseBranch],
        reject: false,
      })
      return resolveAncestryFromExitCode(result.exitCode)
    },
    probeLifecycleFromReflog: async ({
      branch,
      baseBranch,
    }): Promise<{ merged: boolean | null; divergedHead: string | null }> => {
      return probeLifecycleFromReflog({
        repoRoot,
        branch,
        baseBranch,
      })
    },
  }
}

const resolveMergedState = async ({
  repoRoot,
  branch,
  head,
  baseBranch,
  prStateByBranch,
}: {
  readonly repoRoot: string
  readonly branch: string | null
  readonly head: string
  readonly baseBranch: string | null
  readonly prStateByBranch: ReadonlyMap<string, PrState>
}): Promise<WorktreeMergedState> => {
  if (branch === null) {
    return { byAncestry: null, byPR: null, overall: null }
  }

  const mergeProbeRepository = createMergeProbeRepository({ repoRoot })
  const mergeLifecycleRepository = createMergeLifecycleRepository({ repoRoot })

  const byAncestry = baseBranch === null ? null : await mergeProbeRepository.probeAncestry({ branch, baseBranch })

  const byPR = resolveMergedByPr({
    branch,
    baseBranch,
    prStateByBranch,
  })

  let byLifecycle: boolean | null = null
  if (baseBranch !== null) {
    const lifecycle = await mergeLifecycleRepository.upsert({
      branch,
      baseBranch,
      observedDivergedHead: byAncestry === false ? head : null,
    })
    if (byAncestry === false) {
      byLifecycle = false
    } else if (byAncestry === true) {
      if (hasLifecycleDivergedHead(lifecycle)) {
        byLifecycle = await mergeProbeRepository.probeAncestry({
          branch: lifecycle.lastDivergedHead,
          baseBranch,
        })
      } else if (byPR === true) {
        byLifecycle = null
      } else {
        const probe = await mergeProbeRepository.probeLifecycleFromReflog({
          branch,
          baseBranch,
        })
        byLifecycle = probe.merged
        if (probe.divergedHead !== null) {
          await mergeLifecycleRepository.upsert({
            branch,
            baseBranch,
            observedDivergedHead: probe.divergedHead,
          })
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

const resolveWorktreePrState = ({
  branch,
  baseBranch,
  prStateByBranch,
}: {
  readonly branch: string | null
  readonly baseBranch: string | null
  readonly prStateByBranch: ReadonlyMap<string, PrState>
}): WorktreePrState => {
  if (branch === null || branch === baseBranch) {
    return { status: null, url: null }
  }
  const prState = prStateByBranch.get(branch)
  return {
    status: prState?.status ?? null,
    url: prState?.url ?? null,
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
  prStateByBranch,
}: {
  readonly repoRoot: string
  readonly worktree: GitWorktree
  readonly baseBranch: string | null
  readonly prStateByBranch: ReadonlyMap<string, PrState>
}): Promise<WorktreeStatus> => {
  const [dirty, locked, merged, upstream] = await Promise.all([
    resolveDirty(worktree.path),
    resolveLockState({ repoRoot, branch: worktree.branch }),
    resolveMergedState({ repoRoot, branch: worktree.branch, head: worktree.head, baseBranch, prStateByBranch }),
    resolveUpstreamState(worktree.path),
  ])
  const pr = resolveWorktreePrState({
    branch: worktree.branch,
    baseBranch,
    prStateByBranch,
  })

  return {
    branch: worktree.branch,
    path: worktree.path,
    head: worktree.head,
    dirty,
    locked,
    merged,
    pr,
    upstream,
  }
}

export type WorktreeSnapshot = {
  readonly repoRoot: string
  readonly baseBranch: string | null
  readonly worktrees: WorktreeStatus[]
}

type CollectWorktreeSnapshotOptions = {
  readonly baseBranch?: string | null
  readonly ghEnabled?: boolean
  readonly noGh?: boolean
}

export const collectWorktreeSnapshot = async (
  repoRoot: string,
  { baseBranch = null, ghEnabled = true, noGh = false }: CollectWorktreeSnapshotOptions = {},
): Promise<WorktreeSnapshot> => {
  const worktrees = await listGitWorktrees(repoRoot)
  const prStateByBranch = await resolvePrStateByBranchBatch({
    repoRoot,
    baseBranch,
    branches: worktrees.map((worktree) => worktree.branch),
    enabled: ghEnabled && noGh !== true,
  })
  const enriched = await Promise.all(
    worktrees.map(async (worktree) => {
      return enrichWorktree({ repoRoot, worktree, baseBranch, prStateByBranch })
    }),
  )

  return {
    repoRoot,
    baseBranch,
    worktrees: enriched,
  }
}
