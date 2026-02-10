import { constants as fsConstants } from "node:fs"
import { access, chmod, mkdtemp, mkdir, readFile, rm, symlink, writeFile } from "node:fs/promises"
import { hostname } from "node:os"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { afterEach, describe, expect, it } from "vitest"
import { acquireRepoLock, readNumberFromEnvOrDefault, withRepoLock } from "./repo-lock"

const tempDirs = new Set<string>()

const createRepoRoot = async ({ withStateDir = true }: { readonly withStateDir?: boolean } = {}): Promise<string> => {
  const repoRoot = await mkdtemp(join(tmpdir(), "vde-worktree-repo-lock-"))
  tempDirs.add(repoRoot)
  await mkdir(join(repoRoot, ".git"), { recursive: true })
  if (withStateDir) {
    await mkdir(join(repoRoot, ".vde", "worktree", "state"), { recursive: true })
  }
  return repoRoot
}

const exists = async (path: string): Promise<boolean> => {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

afterEach(async () => {
  await Promise.all(
    [...tempDirs].map(async (dir) => {
      await rm(dir, { recursive: true, force: true })
    }),
  )
  tempDirs.clear()
})

describe("acquireRepoLock", () => {
  it("creates lock in state directory when initialized and removes it on release", async () => {
    const repoRoot = await createRepoRoot()
    const lockPath = join(repoRoot, ".vde", "worktree", "state", "repo.lock")

    const handle = await acquireRepoLock({
      repoRoot,
      command: "switch",
      timeoutMs: 200,
    })

    expect(await exists(lockPath)).toBe(true)
    const content = await readFile(lockPath, "utf8")
    expect(content).toContain('"command":"switch"')

    await handle.release()
    expect(await exists(lockPath)).toBe(false)
  })

  it("falls back to .git lock file before initialization", async () => {
    const repoRoot = await createRepoRoot({ withStateDir: false })
    const lockPath = join(repoRoot, ".git", "vde-worktree.init.lock")

    const handle = await acquireRepoLock({
      repoRoot,
      command: "init",
      timeoutMs: 200,
    })

    expect(await exists(lockPath)).toBe(true)
    await handle.release()
    expect(await exists(lockPath)).toBe(false)
  })

  it("recovers stale lock file and acquires lock", async () => {
    const repoRoot = await createRepoRoot()
    const lockPath = join(repoRoot, ".vde", "worktree", "state", "repo.lock")
    const staleStartedAt = new Date(Date.now() - 60_000).toISOString()

    await writeFile(
      lockPath,
      `${JSON.stringify({
        schemaVersion: 1,
        owner: "vde-worktree",
        command: "old-command",
        pid: 999_999,
        host: hostname(),
        startedAt: staleStartedAt,
      })}\n`,
      "utf8",
    )

    const handle = await acquireRepoLock({
      repoRoot,
      command: "new-command",
      timeoutMs: 300,
      staleLockTTLSeconds: 1,
    })

    const content = await readFile(lockPath, "utf8")
    expect(content).toContain('"command":"new-command"')
    await handle.release()
  })

  it("times out when lock is active and cannot be recovered", async () => {
    const repoRoot = await createRepoRoot()
    const lockPath = join(repoRoot, ".vde", "worktree", "state", "repo.lock")

    await writeFile(
      lockPath,
      `${JSON.stringify({
        schemaVersion: 1,
        owner: "vde-worktree",
        command: "running-command",
        pid: process.pid,
        host: hostname(),
        startedAt: new Date().toISOString(),
      })}\n`,
      "utf8",
    )

    await expect(
      acquireRepoLock({
        repoRoot,
        command: "blocked-command",
        timeoutMs: 150,
        staleLockTTLSeconds: 0,
      }),
    ).rejects.toMatchObject({
      code: "REPO_LOCK_TIMEOUT",
    })
  })

  it("recovers from invalid lock metadata", async () => {
    const repoRoot = await createRepoRoot()
    const lockPath = join(repoRoot, ".vde", "worktree", "state", "repo.lock")
    await writeFile(lockPath, "not-json", "utf8")

    const handle = await acquireRepoLock({
      repoRoot,
      command: "repair-lock",
      timeoutMs: 300,
      staleLockTTLSeconds: 1,
    })

    const content = await readFile(lockPath, "utf8")
    expect(content).toContain('"command":"repair-lock"')
    await handle.release()
  })

  it("times out when lock file path cannot be read after existing lock is detected", async () => {
    const repoRoot = await createRepoRoot()
    const lockPath = join(repoRoot, ".vde", "worktree", "state", "repo.lock")
    await symlink("/path/that/does/not/exist", lockPath)

    await expect(
      acquireRepoLock({
        repoRoot,
        command: "blocked-read",
        timeoutMs: 160,
      }),
    ).rejects.toMatchObject({
      code: "REPO_LOCK_TIMEOUT",
    })
  })

  it("throws stale recovery error when stale lock cannot be removed", async () => {
    const repoRoot = await createRepoRoot()
    const stateDir = join(repoRoot, ".vde", "worktree", "state")
    const lockPath = join(stateDir, "repo.lock")

    await writeFile(
      lockPath,
      `${JSON.stringify({
        schemaVersion: 1,
        owner: "vde-worktree",
        command: "old-command",
        pid: 999_999,
        host: hostname(),
        startedAt: new Date(Date.now() - 60_000).toISOString(),
      })}\n`,
      "utf8",
    )
    await chmod(stateDir, 0o500)
    try {
      await expect(
        acquireRepoLock({
          repoRoot,
          command: "new-command",
          timeoutMs: 300,
          staleLockTTLSeconds: 1,
        }),
      ).rejects.toMatchObject({
        code: "REPO_LOCK_STALE_RECOVERY_FAILED",
      })
    } finally {
      await chmod(stateDir, 0o700)
    }
  })

  it("does not recover lock before stale ttl expires", async () => {
    const repoRoot = await createRepoRoot()
    const lockPath = join(repoRoot, ".vde", "worktree", "state", "repo.lock")

    await writeFile(
      lockPath,
      `${JSON.stringify({
        schemaVersion: 1,
        owner: "vde-worktree",
        command: "recent-command",
        pid: 999_999,
        host: "other-host",
        startedAt: new Date().toISOString(),
      })}\n`,
      "utf8",
    )

    await expect(
      acquireRepoLock({
        repoRoot,
        command: "blocked-command",
        timeoutMs: 160,
        staleLockTTLSeconds: 600,
      }),
    ).rejects.toMatchObject({
      code: "REPO_LOCK_TIMEOUT",
    })
  })
})

describe("withRepoLock", () => {
  it("always releases lock even when task throws", async () => {
    const repoRoot = await createRepoRoot()
    const lockPath = join(repoRoot, ".vde", "worktree", "state", "repo.lock")

    await expect(
      withRepoLock(
        {
          repoRoot,
          command: "del",
          timeoutMs: 200,
        },
        async () => {
          expect(await exists(lockPath)).toBe(true)
          throw new Error("boom")
        },
      ),
    ).rejects.toThrow("boom")

    expect(await exists(lockPath)).toBe(false)
  })
})

describe("readNumberFromEnvOrDefault", () => {
  it("returns raw value only when finite number", () => {
    expect(
      readNumberFromEnvOrDefault({
        rawValue: 123,
        defaultValue: 10,
      }),
    ).toBe(123)

    expect(
      readNumberFromEnvOrDefault({
        rawValue: Number.NaN,
        defaultValue: 10,
      }),
    ).toBe(10)

    expect(
      readNumberFromEnvOrDefault({
        rawValue: "123",
        defaultValue: 10,
      }),
    ).toBe(10)
  })
})
