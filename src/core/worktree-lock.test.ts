import { constants as fsConstants } from "node:fs"
import { access, mkdir, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { afterEach, describe, expect, it } from "vitest"
import { cleanupRepoFixtures, createRepoFixture } from "../test-utils/repo-fixture"
import { branchToWorktreeId, getLocksDirectoryPath } from "./paths"
import { deleteWorktreeLock, readWorktreeLock, upsertWorktreeLock } from "./worktree-lock"

const createRepoRoot = async (): Promise<string> => {
  return createRepoFixture({
    prefix: "vde-worktree-lock-",
    setup: async (repoRoot) => {
      await mkdir(getLocksDirectoryPath(repoRoot), { recursive: true })
    },
  })
}

const fileExists = async (path: string): Promise<boolean> => {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

afterEach(cleanupRepoFixtures)

describe("worktree lock", () => {
  it("returns not exists when lock file is absent", async () => {
    const repoRoot = await createRepoRoot()
    const result = await readWorktreeLock({
      repoRoot,
      branch: "feature/a",
    })

    expect(result.exists).toBe(false)
    expect(result.valid).toBe(true)
    expect(result.record).toBeNull()
    expect(result.path).toContain(`.vde/worktree/locks/${branchToWorktreeId("feature/a")}.json`)
  })

  it("upserts lock and preserves createdAt on update", async () => {
    const repoRoot = await createRepoRoot()

    const first = await upsertWorktreeLock({
      repoRoot,
      branch: "feature/a",
      reason: "in progress",
      owner: "codex",
    })
    const second = await upsertWorktreeLock({
      repoRoot,
      branch: "feature/a",
      reason: "reviewing",
      owner: "bot",
    })

    expect(second.createdAt).toBe(first.createdAt)
    expect(Date.parse(second.updatedAt)).toBeGreaterThanOrEqual(Date.parse(first.updatedAt))

    const parsed = await readWorktreeLock({
      repoRoot,
      branch: "feature/a",
    })
    expect(parsed.exists).toBe(true)
    expect(parsed.valid).toBe(true)
    expect(parsed.record).toMatchObject({
      schemaVersion: 1,
      branch: "feature/a",
      reason: "reviewing",
      owner: "bot",
    })
  })

  it("marks malformed JSON metadata as invalid", async () => {
    const repoRoot = await createRepoRoot()
    const path = join(getLocksDirectoryPath(repoRoot), `${branchToWorktreeId("feature/a")}.json`)

    await writeFile(path, "{not-json", "utf8")
    const malformed = await readWorktreeLock({
      repoRoot,
      branch: "feature/a",
    })
    expect(malformed.exists).toBe(true)
    expect(malformed.valid).toBe(false)
    expect(malformed.record).toBeNull()

    await writeFile(
      path,
      `${JSON.stringify({
        schemaVersion: 1,
        branch: "feature/a",
        worktreeId: branchToWorktreeId("feature/a"),
        reason: "",
        owner: "codex",
        host: "localhost",
        pid: process.pid,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      })}\n`,
      "utf8",
    )
    const validRecord = await readWorktreeLock({
      repoRoot,
      branch: "feature/a",
    })
    expect(validRecord.exists).toBe(true)
    expect(validRecord.valid).toBe(true)
    expect(validRecord.record?.reason).toBe("")
  })

  it("marks lock metadata as invalid when file cannot be read as text", async () => {
    const repoRoot = await createRepoRoot()
    const path = join(getLocksDirectoryPath(repoRoot), `${branchToWorktreeId("feature/a")}.json`)
    await mkdir(path, { recursive: true })

    const result = await readWorktreeLock({
      repoRoot,
      branch: "feature/a",
    })
    expect(result.exists).toBe(true)
    expect(result.valid).toBe(false)
    expect(result.record).toBeNull()
  })

  it("deletes lock file safely", async () => {
    const repoRoot = await createRepoRoot()
    const lock = await upsertWorktreeLock({
      repoRoot,
      branch: "feature/a",
      reason: "delete me",
      owner: "codex",
    })
    const lockPath = join(getLocksDirectoryPath(repoRoot), `${branchToWorktreeId("feature/a")}.json`)

    expect(lock.branch).toBe("feature/a")
    expect(await fileExists(lockPath)).toBe(true)
    await deleteWorktreeLock({
      repoRoot,
      branch: "feature/a",
    })
    expect(await fileExists(lockPath)).toBe(false)

    await expect(
      deleteWorktreeLock({
        repoRoot,
        branch: "feature/a",
      }),
    ).resolves.toBeUndefined()
  })
})
