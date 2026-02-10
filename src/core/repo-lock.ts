import { constants as fsConstants } from "node:fs"
import { access, open, readFile, rm } from "node:fs/promises"
import { hostname } from "node:os"
import { join } from "node:path"
import { DEFAULT_LOCK_TIMEOUT_MS, DEFAULT_STALE_LOCK_TTL_SECONDS } from "./constants"
import { createCliError } from "./errors"
import { getStateDirectoryPath } from "./paths"

type RepoLockFileSchema = {
  readonly schemaVersion: 1
  readonly owner: string
  readonly command: string
  readonly pid: number
  readonly host: string
  readonly startedAt: string
}

type AcquireRepoLockOptions = {
  readonly repoRoot: string
  readonly command: string
  readonly timeoutMs?: number
  readonly staleLockTTLSeconds?: number
}

type RepoLockHandle = {
  release: () => Promise<void>
}

const sleep = async (ms: number): Promise<void> => {
  await new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })
}

const isProcessAlive = (pid: number): boolean => {
  if (pid <= 0 || Number.isFinite(pid) !== true) {
    return false
  }

  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code === "ESRCH") {
      return false
    }
    return true
  }
}

const safeParseLockFile = (content: string): RepoLockFileSchema | null => {
  try {
    const parsed = JSON.parse(content) as Partial<RepoLockFileSchema>
    if (parsed.schemaVersion !== 1) {
      return null
    }
    if (typeof parsed.command !== "string" || typeof parsed.owner !== "string") {
      return null
    }
    if (typeof parsed.pid !== "number" || typeof parsed.host !== "string" || typeof parsed.startedAt !== "string") {
      return null
    }
    return parsed as RepoLockFileSchema
  } catch {
    return null
  }
}

const lockFilePath = async (repoRoot: string): Promise<string> => {
  const stateDir = getStateDirectoryPath(repoRoot)
  try {
    await access(stateDir, fsConstants.F_OK)
    return join(stateDir, "repo.lock")
  } catch {
    return join(repoRoot, ".git", "vde-worktree.init.lock")
  }
}

const buildLockPayload = (command: string): RepoLockFileSchema => {
  return {
    schemaVersion: 1,
    owner: "vde-worktree",
    command,
    pid: process.pid,
    host: hostname(),
    startedAt: new Date().toISOString(),
  }
}

const canRecoverStaleLock = ({
  lock,
  staleLockTTLSeconds,
}: {
  readonly lock: RepoLockFileSchema | null
  readonly staleLockTTLSeconds: number
}): boolean => {
  if (lock === null) {
    return true
  }

  const startedAtMs = Date.parse(lock.startedAt)
  if (Number.isFinite(startedAtMs) !== true) {
    return true
  }

  const staleAtMs = startedAtMs + staleLockTTLSeconds * 1000
  if (staleAtMs > Date.now()) {
    return false
  }

  if (lock.host === hostname() && isProcessAlive(lock.pid)) {
    return false
  }

  return true
}

const writeNewLockFile = async (path: string, payload: RepoLockFileSchema): Promise<boolean> => {
  try {
    const handle = await open(path, "wx")
    await handle.writeFile(`${JSON.stringify(payload)}\n`, "utf8")
    await handle.close()
    return true
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code === "EEXIST") {
      return false
    }
    throw error
  }
}

export const acquireRepoLock = async ({
  repoRoot,
  command,
  timeoutMs = DEFAULT_LOCK_TIMEOUT_MS,
  staleLockTTLSeconds = DEFAULT_STALE_LOCK_TTL_SECONDS,
}: AcquireRepoLockOptions): Promise<RepoLockHandle> => {
  const path = await lockFilePath(repoRoot)
  const startAt = Date.now()
  const payload = buildLockPayload(command)

  while (Date.now() - startAt <= timeoutMs) {
    const created = await writeNewLockFile(path, payload)
    if (created) {
      return {
        release: async (): Promise<void> => {
          try {
            await rm(path, { force: true })
          } catch {
            return
          }
        },
      }
    }

    let lockContent = ""
    try {
      lockContent = await readFile(path, "utf8")
    } catch {
      await sleep(100)
      continue
    }

    const parsed = safeParseLockFile(lockContent)
    if (canRecoverStaleLock({ lock: parsed, staleLockTTLSeconds })) {
      try {
        await rm(path, { force: true })
      } catch {
        throw createCliError("REPO_LOCK_STALE_RECOVERY_FAILED", {
          message: "Failed to recover stale repo lock",
          details: { path },
        })
      }
      continue
    }

    await sleep(100)
  }

  throw createCliError("REPO_LOCK_TIMEOUT", {
    message: "Timed out while acquiring repo lock",
    details: { path, timeoutMs },
  })
}

export const withRepoLock = async <T>(options: AcquireRepoLockOptions, task: () => Promise<T>): Promise<T> => {
  const handle = await acquireRepoLock(options)
  try {
    return await task()
  } finally {
    await handle.release()
  }
}

export const readNumberFromEnvOrDefault = ({
  rawValue,
  defaultValue,
}: {
  readonly rawValue: unknown
  readonly defaultValue: number
}): number => {
  if (typeof rawValue !== "number" || Number.isFinite(rawValue) !== true) {
    return defaultValue
  }
  return rawValue
}
