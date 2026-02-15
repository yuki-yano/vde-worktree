import { mkdir, mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, describe, expect, it } from "vitest"
import {
  deleteWorktreeMergeLifecycle,
  moveWorktreeMergeLifecycle,
  readWorktreeMergeLifecycle,
  upsertWorktreeMergeLifecycle,
} from "./worktree-merge-lifecycle"
import { getStateDirectoryPath } from "./paths"

const tempDirs = new Set<string>()

const createRepoRoot = async (): Promise<string> => {
  const repoRoot = await mkdtemp(join(tmpdir(), "vde-worktree-merge-lifecycle-"))
  tempDirs.add(repoRoot)
  await mkdir(getStateDirectoryPath(repoRoot), { recursive: true })
  return repoRoot
}

afterEach(async () => {
  await Promise.all(
    [...tempDirs].map(async (dir) => {
      await rm(dir, { recursive: true, force: true })
    }),
  )
  tempDirs.clear()
})

describe("worktree merge lifecycle", () => {
  it("creates lifecycle record with created head", async () => {
    const repoRoot = await createRepoRoot()
    const record = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "main",
      createdHead: "abc123",
    })

    expect(record.branch).toBe("feature/a")
    expect(record.baseBranch).toBe("main")
    expect(record.createdHead).toBe("abc123")

    const read = await readWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
    })
    expect(read.exists).toBe(true)
    expect(read.valid).toBe(true)
    expect(read.record?.createdHead).toBe("abc123")
  })

  it("keeps created head when base branch changes", async () => {
    const repoRoot = await createRepoRoot()
    const first = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "main",
      createdHead: "abc123",
    })
    const second = await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "trunk",
      createdHead: "def456",
    })

    expect(second.createdHead).toBe(first.createdHead)
    expect(second.baseBranch).toBe("trunk")
  })

  it("moves lifecycle record on branch rename", async () => {
    const repoRoot = await createRepoRoot()
    await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/old",
      baseBranch: "main",
      createdHead: "abc123",
    })

    const moved = await moveWorktreeMergeLifecycle({
      repoRoot,
      fromBranch: "feature/old",
      toBranch: "feature/new",
      baseBranch: "main",
      createdHead: "zzz999",
    })

    expect(moved.branch).toBe("feature/new")
    expect(moved.createdHead).toBe("abc123")
    const oldRecord = await readWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/old",
    })
    const newRecord = await readWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/new",
    })
    expect(oldRecord.exists).toBe(false)
    expect(newRecord.record?.createdHead).toBe("abc123")
  })

  it("deletes lifecycle record", async () => {
    const repoRoot = await createRepoRoot()
    await upsertWorktreeMergeLifecycle({
      repoRoot,
      branch: "feature/a",
      baseBranch: "main",
      createdHead: "abc123",
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
