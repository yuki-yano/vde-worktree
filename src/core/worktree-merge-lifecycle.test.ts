import { mkdir, writeFile } from "node:fs/promises"
import { dirname, join } from "node:path"
import { afterEach, describe, expect, it } from "vitest"
import { cleanupRepoFixtures, createRepoFixture } from "../test-utils/repo-fixture"
import {
  deleteWorktreeMergeLifecycle,
  moveWorktreeMergeLifecycle,
  readWorktreeMergeLifecycle,
  upsertWorktreeMergeLifecycle,
} from "./worktree-merge-lifecycle"
import { branchToWorktreeId, getStateDirectoryPath } from "./paths"

const createRepoRoot = async (): Promise<string> => {
  return createRepoFixture({
    prefix: "vde-worktree-merge-lifecycle-",
    setup: async (repoRoot) => {
      await mkdir(getStateDirectoryPath(repoRoot), { recursive: true })
    },
  })
}

afterEach(cleanupRepoFixtures)

describe("worktree merge lifecycle", () => {
  it("creates lifecycle record without divergence evidence", async () => {
    const repoRoot = await createRepoRoot()
    const record = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "main",
      observedDivergedHead: null,
    })

    expect(record.branch).toBe("feature/a")
    expect(record.baseBranch).toBe("main")
    expect(record.everDiverged).toBe(false)
    expect(record.lastDivergedHead).toBeNull()

    const read = await readWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
    })
    expect(read.exists).toBe(true)
    expect(read.valid).toBe(true)
    expect(read.record?.everDiverged).toBe(false)
    expect(read.record?.lastDivergedHead).toBeNull()
  })

  it("records latest divergence head and keeps evidence across non-diverged updates", async () => {
    const repoRoot = await createRepoRoot()
    const first = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "main",
      observedDivergedHead: "abc123",
    })
    const second = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "trunk",
      observedDivergedHead: null,
    })
    const third = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "trunk",
      observedDivergedHead: "def456",
    })

    expect(second.createdAt).toBe(first.createdAt)
    expect(second.everDiverged).toBe(true)
    expect(second.lastDivergedHead).toBe("abc123")
    expect(second.baseBranch).toBe("trunk")
    expect(third.everDiverged).toBe(true)
    expect(third.lastDivergedHead).toBe("def456")
  })

  it("ignores legacy schema records and rewrites with schema version 2", async () => {
    const repoRoot = await createRepoRoot()
    const branch = "feature/legacy"
    const filePath = join(getStateDirectoryPath(repoRoot), "branches", `${branchToWorktreeId(branch)}.json`)

    await mkdir(dirname(filePath), { recursive: true })
    await writeFile(
      filePath,
      `${JSON.stringify({
        schemaVersion: 1,
        branch,
        worktreeId: branchToWorktreeId(branch),
        baseBranch: "main",
        createdHead: "legacy-head",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      })}\n`,
      "utf8",
    )

    const record = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch,
      baseBranch: "main",
      observedDivergedHead: null,
    })

    expect(record.schemaVersion).toBe(2)
    expect(record.everDiverged).toBe(false)
    expect(record.lastDivergedHead).toBeNull()
  })

  it("moves lifecycle record on branch rename", async () => {
    const repoRoot = await createRepoRoot()
    await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/old",
      baseBranch: "main",
      observedDivergedHead: "abc123",
    })

    const moved = await moveWorktreeMergeLifecycle({
      repoRoot,
      fromBranch: "feature/old",
      toBranch: "feature/new",
      baseBranch: "main",
      observedDivergedHead: null,
    })

    expect(moved.branch).toBe("feature/new")
    expect(moved.lastDivergedHead).toBe("abc123")
    expect(moved.everDiverged).toBe(true)
    const oldRecord = await readWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/old",
    })
    const newRecord = await readWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/new",
    })
    expect(oldRecord.exists).toBe(false)
    expect(newRecord.record?.lastDivergedHead).toBe("abc123")
  })

  it("deletes lifecycle record", async () => {
    const repoRoot = await createRepoRoot()
    await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "main",
      observedDivergedHead: "abc123",
    })

    const before = await readWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
    })
    expect(before.exists).toBe(true)
    await deleteWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
    })
    const after = await readWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
    })
    expect(after.exists).toBe(false)
  })
})
