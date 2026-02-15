import { constants as fsConstants } from "node:fs"
import { access, mkdir, readFile, rename, rm, writeFile } from "node:fs/promises"
import { dirname, join } from "node:path"
import { branchToWorktreeId, getStateDirectoryPath } from "./paths"

export type WorktreeMergeLifecycleRecord = {
  readonly schemaVersion: 1
  readonly branch: string
  readonly worktreeId: string
  readonly baseBranch: string
  readonly createdHead: string
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

const parseLifecycle = (content: string): ParsedLifecycle => {
  try {
    const parsed = JSON.parse(content) as Partial<WorktreeMergeLifecycleRecord>
    if (
      parsed.schemaVersion !== 1 ||
      typeof parsed.branch !== "string" ||
      typeof parsed.worktreeId !== "string" ||
      typeof parsed.baseBranch !== "string" ||
      typeof parsed.createdHead !== "string" ||
      parsed.createdHead.length === 0 ||
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
      record: parsed as WorktreeMergeLifecycleRecord,
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
  await mkdir(dirname(filePath), { recursive: true })
  const tmpPath = `${filePath}.tmp-${String(process.pid)}-${String(Date.now())}`
  await writeFile(tmpPath, `${JSON.stringify(payload)}\n`, "utf8")
  await rename(tmpPath, filePath)
}

export const readWorktreeMergeLifecycle = async ({
  repoRoot,
  branch,
}: {
  readonly repoRoot: string
  readonly branch: string
}): Promise<ParsedLifecycle & { path: string; exists: boolean }> => {
  const path = lifecycleFilePath(repoRoot, branch)
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
    const parsed = parseLifecycle(content)
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

export const upsertWorktreeMergeLifecycle = async ({
  repoRoot,
  branch,
  baseBranch,
  createdHead,
}: {
  readonly repoRoot: string
  readonly branch: string
  readonly baseBranch: string
  readonly createdHead: string
}): Promise<WorktreeMergeLifecycleRecord> => {
  if ((await hasStateDirectory(repoRoot)) !== true) {
    const now = new Date().toISOString()
    return {
      schemaVersion: 1,
      branch,
      worktreeId: branchToWorktreeId(branch),
      baseBranch,
      createdHead,
      createdAt: now,
      updatedAt: now,
    }
  }

  const current = await readWorktreeMergeLifecycle({ repoRoot, branch })
  if (current.valid && current.record !== null && current.record.baseBranch === baseBranch) {
    return current.record
  }

  const now = new Date().toISOString()
  const next: WorktreeMergeLifecycleRecord = {
    schemaVersion: 1,
    branch,
    worktreeId: branchToWorktreeId(branch),
    baseBranch,
    createdHead: current.record?.createdHead ?? createdHead,
    createdAt: current.record?.createdAt ?? now,
    updatedAt: now,
  }
  await writeJsonAtomically({
    filePath: current.path,
    payload: next,
  })
  return next
}

export const moveWorktreeMergeLifecycle = async ({
  repoRoot,
  fromBranch,
  toBranch,
  baseBranch,
  createdHead,
}: {
  readonly repoRoot: string
  readonly fromBranch: string
  readonly toBranch: string
  readonly baseBranch: string
  readonly createdHead: string
}): Promise<WorktreeMergeLifecycleRecord> => {
  if ((await hasStateDirectory(repoRoot)) !== true) {
    const now = new Date().toISOString()
    return {
      schemaVersion: 1,
      branch: toBranch,
      worktreeId: branchToWorktreeId(toBranch),
      baseBranch,
      createdHead,
      createdAt: now,
      updatedAt: now,
    }
  }

  const source = await readWorktreeMergeLifecycle({ repoRoot, branch: fromBranch })
  const targetPath = lifecycleFilePath(repoRoot, toBranch)
  const now = new Date().toISOString()
  const next: WorktreeMergeLifecycleRecord = {
    schemaVersion: 1,
    branch: toBranch,
    worktreeId: branchToWorktreeId(toBranch),
    baseBranch,
    createdHead: source.record?.createdHead ?? createdHead,
    createdAt: source.record?.createdAt ?? now,
    updatedAt: now,
  }

  await writeJsonAtomically({
    filePath: targetPath,
    payload: next,
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
