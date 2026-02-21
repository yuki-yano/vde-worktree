import { constants as fsConstants } from "node:fs"
import { access, rm } from "node:fs/promises"
import { join } from "node:path"
import { readJsonRecord, writeJsonAtomically } from "./json-storage"
import { branchToWorktreeId, getStateDirectoryPath } from "./paths"

export type WorktreeMergeLifecycleRecord = {
  readonly schemaVersion: 2
  readonly branch: string
  readonly worktreeId: string
  readonly baseBranch: string
  readonly everDiverged: boolean
  readonly lastDivergedHead: string | null
  readonly createdAt: string
  readonly updatedAt: string
}

type ParsedLifecycle = {
  readonly valid: boolean
  readonly record: WorktreeMergeLifecycleRecord | null
}

const lifecycleFilePath = (repoRoot: string, branch: string): string => {
  return join(getStateDirectoryPath(repoRoot), "branches", `${branchToWorktreeId(branch)}.json`)
}

const hasStateDirectory = async (repoRoot: string): Promise<boolean> => {
  try {
    await access(getStateDirectoryPath(repoRoot), fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

const isWorktreeMergeLifecycleRecord = (
  parsed: Partial<WorktreeMergeLifecycleRecord>,
): parsed is WorktreeMergeLifecycleRecord => {
  const isLastDivergedHeadValid =
    parsed.lastDivergedHead === null ||
    (typeof parsed.lastDivergedHead === "string" && parsed.lastDivergedHead.length > 0)

  return (
    typeof parsed.branch === "string" &&
    typeof parsed.worktreeId === "string" &&
    typeof parsed.baseBranch === "string" &&
    typeof parsed.everDiverged === "boolean" &&
    isLastDivergedHeadValid &&
    typeof parsed.createdAt === "string" &&
    typeof parsed.updatedAt === "string"
  )
}

export const readWorktreeMergeLifecycle = async ({
  repoRoot,
  branch,
}: {
  readonly repoRoot: string
  readonly branch: string
}): Promise<ParsedLifecycle & { path: string; exists: boolean }> => {
  const path = lifecycleFilePath(repoRoot, branch)
  return readJsonRecord<WorktreeMergeLifecycleRecord>({
    path,
    schemaVersion: 2,
    validate: isWorktreeMergeLifecycleRecord,
  })
}

export const upsertWorktreeMergeLifecycle = async ({
  repoRoot,
  branch,
  baseBranch,
  observedDivergedHead,
}: {
  readonly repoRoot: string
  readonly branch: string
  readonly baseBranch: string
  readonly observedDivergedHead: string | null
}): Promise<WorktreeMergeLifecycleRecord> => {
  const normalizedObservedHead =
    typeof observedDivergedHead === "string" && observedDivergedHead.length > 0 ? observedDivergedHead : null

  if ((await hasStateDirectory(repoRoot)) !== true) {
    const now = new Date().toISOString()
    return {
      schemaVersion: 2,
      branch,
      worktreeId: branchToWorktreeId(branch),
      baseBranch,
      everDiverged: normalizedObservedHead !== null,
      lastDivergedHead: normalizedObservedHead,
      createdAt: now,
      updatedAt: now,
    }
  }

  const current = await readWorktreeMergeLifecycle({ repoRoot, branch })
  if (
    current.valid &&
    current.record !== null &&
    current.record.baseBranch === baseBranch &&
    normalizedObservedHead === null
  ) {
    return current.record
  }

  const now = new Date().toISOString()
  const everDiverged = current.record?.everDiverged === true || normalizedObservedHead !== null
  const lastDivergedHead = normalizedObservedHead ?? current.record?.lastDivergedHead ?? null
  const next: WorktreeMergeLifecycleRecord = {
    schemaVersion: 2,
    branch,
    worktreeId: branchToWorktreeId(branch),
    baseBranch,
    everDiverged,
    lastDivergedHead,
    createdAt: current.record?.createdAt ?? now,
    updatedAt: now,
  }
  await writeJsonAtomically({
    filePath: current.path,
    payload: next,
    ensureDir: true,
  })
  return next
}

export const moveWorktreeMergeLifecycle = async ({
  repoRoot,
  fromBranch,
  toBranch,
  baseBranch,
  observedDivergedHead,
}: {
  readonly repoRoot: string
  readonly fromBranch: string
  readonly toBranch: string
  readonly baseBranch: string
  readonly observedDivergedHead: string | null
}): Promise<WorktreeMergeLifecycleRecord> => {
  const normalizedObservedHead =
    typeof observedDivergedHead === "string" && observedDivergedHead.length > 0 ? observedDivergedHead : null

  if ((await hasStateDirectory(repoRoot)) !== true) {
    const now = new Date().toISOString()
    return {
      schemaVersion: 2,
      branch: toBranch,
      worktreeId: branchToWorktreeId(toBranch),
      baseBranch,
      everDiverged: normalizedObservedHead !== null,
      lastDivergedHead: normalizedObservedHead,
      createdAt: now,
      updatedAt: now,
    }
  }

  const source = await readWorktreeMergeLifecycle({ repoRoot, branch: fromBranch })
  const targetPath = lifecycleFilePath(repoRoot, toBranch)
  const now = new Date().toISOString()
  const everDiverged = source.record?.everDiverged === true || normalizedObservedHead !== null
  const lastDivergedHead = normalizedObservedHead ?? source.record?.lastDivergedHead ?? null
  const next: WorktreeMergeLifecycleRecord = {
    schemaVersion: 2,
    branch: toBranch,
    worktreeId: branchToWorktreeId(toBranch),
    baseBranch,
    everDiverged,
    lastDivergedHead,
    createdAt: source.record?.createdAt ?? now,
    updatedAt: now,
  }

  await writeJsonAtomically({
    filePath: targetPath,
    payload: next,
    ensureDir: true,
  })

  if (source.path !== targetPath) {
    await rm(source.path, { force: true })
  }
  return next
}

export const deleteWorktreeMergeLifecycle = async ({
  repoRoot,
  branch,
}: {
  readonly repoRoot: string
  readonly branch: string
}): Promise<void> => {
  const path = lifecycleFilePath(repoRoot, branch)
  await rm(path, { force: true })
}
