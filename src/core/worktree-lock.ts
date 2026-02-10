import { constants as fsConstants } from "node:fs"
import { access, readFile, rename, rm, writeFile } from "node:fs/promises"
import { hostname } from "node:os"
import { join } from "node:path"
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

const parseLock = (content: string): ParsedLock => {
  try {
    const parsed = JSON.parse(content) as Partial<WorktreeLockRecord>
    if (
      parsed.schemaVersion !== 1 ||
      typeof parsed.branch !== "string" ||
      typeof parsed.worktreeId !== "string" ||
      typeof parsed.reason !== "string" ||
      typeof parsed.owner !== "string" ||
      typeof parsed.host !== "string" ||
      typeof parsed.pid !== "number" ||
      typeof parsed.createdAt !== "string" ||
      typeof parsed.updatedAt !== "string"
    ) {
      return {
        valid: false,
        record: null,
      }
    }

    return {
      valid: true,
      record: parsed as WorktreeLockRecord,
    }
  } catch {
    return {
      valid: false,
      record: null,
    }
  }
}

const writeJsonAtomically = async ({
  filePath,
  payload,
}: {
  readonly filePath: string
  readonly payload: Record<string, unknown>
}): Promise<void> => {
  const tmpPath = `${filePath}.tmp-${String(process.pid)}-${String(Date.now())}`
  await writeFile(tmpPath, `${JSON.stringify(payload)}\n`, "utf8")
  await rename(tmpPath, filePath)
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
  try {
    await access(path, fsConstants.F_OK)
  } catch {
    return {
      path,
      exists: false,
      valid: true,
      record: null,
    }
  }

  try {
    const content = await readFile(path, "utf8")
    const parsed = parseLock(content)
    return {
      path,
      exists: true,
      ...parsed,
    }
  } catch {
    return {
      path,
      exists: true,
      valid: false,
      record: null,
    }
  }
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
