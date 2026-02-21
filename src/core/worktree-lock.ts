import { rm } from "node:fs/promises"
import { hostname } from "node:os"
import { join } from "node:path"
import { readJsonRecord, writeJsonAtomically } from "./json-storage"
import { branchToWorktreeId, getLocksDirectoryPath } from "./paths"

export type WorktreeLockRecord = {
  readonly schemaVersion: 1
  readonly branch: string
  readonly worktreeId: string
  readonly reason: string
  readonly owner: string
  readonly host: string
  readonly pid: number
  readonly createdAt: string
  readonly updatedAt: string
}

type ParsedLock = {
  readonly valid: boolean
  readonly record: WorktreeLockRecord | null
}

export const isWorktreeLockRecord = (parsed: Partial<WorktreeLockRecord>): parsed is WorktreeLockRecord => {
  return (
    typeof parsed.branch === "string" &&
    typeof parsed.worktreeId === "string" &&
    typeof parsed.reason === "string" &&
    typeof parsed.owner === "string" &&
    typeof parsed.host === "string" &&
    typeof parsed.pid === "number" &&
    typeof parsed.createdAt === "string" &&
    typeof parsed.updatedAt === "string"
  )
}

const lockFilePath = (repoRoot: string, branch: string): string => {
  return join(getLocksDirectoryPath(repoRoot), `${branchToWorktreeId(branch)}.json`)
}

export const readWorktreeLock = async ({
  repoRoot,
  branch,
}: {
  readonly repoRoot: string
  readonly branch: string
}): Promise<ParsedLock & { path: string; exists: boolean }> => {
  const path = lockFilePath(repoRoot, branch)
  return readJsonRecord<WorktreeLockRecord>({
    path,
    schemaVersion: 1,
    validate: isWorktreeLockRecord,
  })
}

export const upsertWorktreeLock = async ({
  repoRoot,
  branch,
  reason,
  owner,
}: {
  readonly repoRoot: string
  readonly branch: string
  readonly reason: string
  readonly owner: string
}): Promise<WorktreeLockRecord> => {
  const { path, record } = await readWorktreeLock({ repoRoot, branch })
  const now = new Date().toISOString()
  const next: WorktreeLockRecord = {
    schemaVersion: 1,
    branch,
    worktreeId: branchToWorktreeId(branch),
    reason,
    owner,
    host: hostname(),
    pid: process.pid,
    createdAt: record?.createdAt ?? now,
    updatedAt: now,
  }
  await writeJsonAtomically({
    filePath: path,
    payload: next,
  })
  return next
}

export const deleteWorktreeLock = async ({
  repoRoot,
  branch,
}: {
  readonly repoRoot: string
  readonly branch: string
}): Promise<void> => {
  const path = lockFilePath(repoRoot, branch)
  await rm(path, { force: true })
}
