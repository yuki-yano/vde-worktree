import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

vi.mock("../git/exec", () => {
  return {
    runGitCommand: vi.fn(),
  }
})

vi.mock("../git/worktree", () => {
  return {
    listGitWorktrees: vi.fn(),
  }
})

vi.mock("../integrations/gh", () => {
  return {
    resolvePrStateByBranchBatch: vi.fn(),
  }
})

import { runGitCommand } from "../git/exec"
import { listGitWorktrees, type GitWorktree } from "../git/worktree"
import { resolvePrStateByBranchBatch } from "../integrations/gh"
import { branchToWorktreeId, getLocksDirectoryPath, getStateDirectoryPath } from "./paths"
import { collectWorktreeSnapshot } from "./worktree-state"

const mockedRunGitCommand = vi.mocked(runGitCommand)
const mockedListGitWorktrees = vi.mocked(listGitWorktrees)
const mockedResolvePrStateByBranchBatch = vi.mocked(resolvePrStateByBranchBatch)

const tempDirs = new Set<string>()

const createRepoRoot = async (): Promise<string> => {
  const repoRoot = await mkdtemp(join(tmpdir(), "vde-worktree-state-"))
  tempDirs.add(repoRoot)
  await mkdir(getLocksDirectoryPath(repoRoot), { recursive: true })
  return repoRoot
}

const gitResult = ({
  stdout = "",
  stderr = "",
  exitCode = 0,
}: {
  readonly stdout?: string
  readonly stderr?: string
  readonly exitCode?: number
}) => {
  return {
    stdout,
    stderr,
    exitCode,
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

beforeEach(() => {
  mockedRunGitCommand.mockReset()
  mockedListGitWorktrees.mockReset()
  mockedResolvePrStateByBranchBatch.mockReset()
})

describe("collectWorktreeSnapshot", () => {
  it("uses explicit base branch and resolves lock/merged/upstream states", async () => {
    const repoRoot = await createRepoRoot()
    const branch = "feature/a"
    const worktreePath = join(repoRoot, ".worktree", "feature", "a")
    const lockPath = join(getLocksDirectoryPath(repoRoot), `${branchToWorktreeId(branch)}.json`)

    await writeFile(
      lockPath,
      `${JSON.stringify({
        schemaVersion: 1,
        branch,
        worktreeId: branchToWorktreeId(branch),
        reason: "active task",
        owner: "codex",
      })}\n`,
      "utf8",
    )

    mockedListGitWorktrees.mockResolvedValueOnce([
      {
        path: worktreePath,
        head: "abc123",
        branch,
      } satisfies GitWorktree,
    ])

    mockedResolvePrStateByBranchBatch.mockResolvedValueOnce(
      new Map([
        [
          "feature/a",
          {
            status: "unknown",
            url: null,
          },
        ],
      ]),
    )
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === repoRoot && args.join(" ") === "merge-base --is-ancestor feature/a trunk") {
        return gitResult({ exitCode: 1 })
      }
      if (cwd === worktreePath && args.join(" ") === "status --porcelain") {
        return gitResult({ stdout: " M README.md\n" })
      }
      if (cwd === worktreePath && args.join(" ") === "rev-parse --abbrev-ref --symbolic-full-name @{upstream}") {
        return gitResult({ stdout: "origin/feature/a\n" })
      }
      if (cwd === worktreePath && args.join(" ") === "rev-list --left-right --count @{upstream}...HEAD") {
        return gitResult({ stdout: "2 5\n" })
      }
      throw new Error(`unexpected git command: cwd=${cwd} args=${args.join(" ")}`)
    })

    const snapshot = await collectWorktreeSnapshot(repoRoot, {
      baseBranch: "trunk",
      ghEnabled: false,
    })
    expect(snapshot.baseBranch).toBe("trunk")
    expect(snapshot.repoRoot).toBe(repoRoot)
    expect(mockedResolvePrStateByBranchBatch).toHaveBeenCalledWith({
      repoRoot,
      baseBranch: "trunk",
      branches: ["feature/a"],
      enabled: false,
    })
    expect(snapshot.worktrees).toEqual([
      {
        branch: "feature/a",
        path: worktreePath,
        head: "abc123",
        dirty: true,
        locked: {
          value: true,
          reason: "active task",
          owner: "codex",
        },
        merged: {
          byAncestry: false,
          byPR: null,
          overall: false,
        },
        pr: {
          status: "unknown",
          url: null,
        },
        upstream: {
          ahead: 5,
          behind: 2,
          remote: "origin/feature/a",
        },
      },
    ])
  })

  it("falls back to main/master detection and handles detached worktree", async () => {
    const repoRoot = await createRepoRoot()
    const detachedPath = join(repoRoot, ".worktree", "detached")
    const featurePath = join(repoRoot, ".worktree", "feature", "b")

    mockedListGitWorktrees.mockResolvedValueOnce([
      {
        path: detachedPath,
        head: "h1",
        branch: null,
      } satisfies GitWorktree,
      {
        path: featurePath,
        head: "h2",
        branch: "feature/b",
      } satisfies GitWorktree,
    ])

    mockedResolvePrStateByBranchBatch.mockResolvedValueOnce(
      new Map([
        [
          "feature/b",
          {
            status: "merged",
            url: "https://github.com/example/repo/pull/42",
          },
        ],
      ]),
    )
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === detachedPath && args.join(" ") === "status --porcelain") {
        return gitResult({ stdout: "" })
      }
      if (cwd === featurePath && args.join(" ") === "status --porcelain") {
        return gitResult({ stdout: "" })
      }
      if (cwd === repoRoot && args.join(" ") === "merge-base --is-ancestor feature/b master") {
        return gitResult({ exitCode: 0 })
      }
      if (cwd === detachedPath && args.join(" ") === "rev-parse --abbrev-ref --symbolic-full-name @{upstream}") {
        return gitResult({ exitCode: 1 })
      }
      if (cwd === featurePath && args.join(" ") === "rev-parse --abbrev-ref --symbolic-full-name @{upstream}") {
        return gitResult({ exitCode: 1 })
      }
      throw new Error(`unexpected git command: cwd=${cwd} args=${args.join(" ")}`)
    })

    const snapshot = await collectWorktreeSnapshot(repoRoot, {
      baseBranch: "master",
    })
    expect(snapshot.baseBranch).toBe("master")
    expect(mockedResolvePrStateByBranchBatch).toHaveBeenCalledTimes(1)
    expect(mockedResolvePrStateByBranchBatch).toHaveBeenCalledWith({
      repoRoot,
      baseBranch: "master",
      branches: [null, "feature/b"],
      enabled: true,
    })

    expect(snapshot.worktrees[0]).toEqual({
      branch: null,
      path: detachedPath,
      head: "h1",
      dirty: false,
      locked: { value: false, reason: null, owner: null },
      merged: { byAncestry: null, byPR: null, overall: null },
      pr: { status: null, url: null },
      upstream: { ahead: null, behind: null, remote: null },
    })
    expect(snapshot.worktrees[1]).toEqual({
      branch: "feature/b",
      path: featurePath,
      head: "h2",
      dirty: false,
      locked: { value: false, reason: null, owner: null },
      merged: { byAncestry: true, byPR: true, overall: true },
      pr: { status: "merged", url: "https://github.com/example/repo/pull/42" },
      upstream: { ahead: null, behind: null, remote: null },
    })
  })

  it("marks lock metadata as invalid and handles unparsable upstream distance", async () => {
    const repoRoot = await createRepoRoot()
    const branch = "feature/c"
    const worktreePath = join(repoRoot, ".worktree", "feature", "c")
    const lockPath = join(getLocksDirectoryPath(repoRoot), `${branchToWorktreeId(branch)}.json`)

    await mkdir(lockPath, { recursive: true })

    mockedListGitWorktrees.mockResolvedValueOnce([
      {
        path: worktreePath,
        head: "h3",
        branch,
      } satisfies GitWorktree,
    ])
    mockedResolvePrStateByBranchBatch.mockResolvedValueOnce(
      new Map([
        [
          "feature/c",
          {
            status: "closed_unmerged",
            url: "https://github.com/example/repo/pull/43",
          },
        ],
      ]),
    )
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === worktreePath && args.join(" ") === "status --porcelain") {
        return gitResult({ stdout: "" })
      }
      if (cwd === repoRoot && args.join(" ") === "merge-base --is-ancestor feature/c main") {
        return gitResult({ exitCode: 2 })
      }
      if (cwd === worktreePath && args.join(" ") === "rev-parse --abbrev-ref --symbolic-full-name @{upstream}") {
        return gitResult({ stdout: "origin/feature/c\n" })
      }
      if (cwd === worktreePath && args.join(" ") === "rev-list --left-right --count @{upstream}...HEAD") {
        return gitResult({ stdout: "NaN x\n" })
      }
      throw new Error(`unexpected git command: cwd=${cwd} args=${args.join(" ")}`)
    })

    const snapshot = await collectWorktreeSnapshot(repoRoot, {
      baseBranch: "main",
      ghEnabled: false,
    })
    expect(snapshot.baseBranch).toBe("main")
    expect(snapshot.worktrees[0]).toEqual({
      branch: "feature/c",
      path: worktreePath,
      head: "h3",
      dirty: false,
      locked: {
        value: true,
        reason: "invalid lock metadata",
        owner: null,
      },
      merged: {
        byAncestry: null,
        byPR: false,
        overall: false,
      },
      pr: {
        status: "closed_unmerged",
        url: "https://github.com/example/repo/pull/43",
      },
      upstream: {
        ahead: null,
        behind: null,
        remote: "origin/feature/c",
      },
    })
  })

  it("disables gh lookup when noGh option is true", async () => {
    const repoRoot = await createRepoRoot()
    const branch = "feature/no-gh"
    const worktreePath = join(repoRoot, ".worktree", "feature", "no-gh")

    mockedListGitWorktrees.mockResolvedValueOnce([
      {
        path: worktreePath,
        head: "h4",
        branch,
      } satisfies GitWorktree,
    ])

    mockedResolvePrStateByBranchBatch.mockResolvedValueOnce(
      new Map([
        [
          "feature/no-gh",
          {
            status: "unknown",
            url: null,
          },
        ],
      ]),
    )
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === repoRoot && args.join(" ") === "merge-base --is-ancestor feature/no-gh main") {
        return gitResult({ exitCode: 1 })
      }
      if (cwd === worktreePath && args.join(" ") === "status --porcelain") {
        return gitResult({ stdout: "" })
      }
      if (cwd === worktreePath && args.join(" ") === "rev-parse --abbrev-ref --symbolic-full-name @{upstream}") {
        return gitResult({ exitCode: 1 })
      }
      throw new Error(`unexpected git command: cwd=${cwd} args=${args.join(" ")}`)
    })

    const snapshot = await collectWorktreeSnapshot(repoRoot, { baseBranch: "main", ghEnabled: true, noGh: true })
    expect(snapshot.baseBranch).toBe("main")
    expect(mockedResolvePrStateByBranchBatch).toHaveBeenCalledWith({
      repoRoot,
      baseBranch: "main",
      branches: ["feature/no-gh"],
      enabled: false,
    })
    expect(snapshot.worktrees[0]?.merged.byPR).toBeNull()
    expect(snapshot.worktrees[0]?.pr.status).toBe("unknown")
    expect(snapshot.worktrees[0]?.pr.url).toBeNull()
  })

  it("keeps branch unmerged after rebase when no divergence has been observed", async () => {
    const repoRoot = await createRepoRoot()
    await mkdir(getStateDirectoryPath(repoRoot), { recursive: true })
    const branch = "feature/rebase"
    const worktreePath = join(repoRoot, ".worktree", "feature", "rebase")

    mockedListGitWorktrees
      .mockResolvedValueOnce([
        {
          path: worktreePath,
          head: "h1",
          branch,
        } satisfies GitWorktree,
      ])
      .mockResolvedValueOnce([
        {
          path: worktreePath,
          head: "h2",
          branch,
        } satisfies GitWorktree,
      ])

    mockedResolvePrStateByBranchBatch
      .mockResolvedValueOnce(new Map([["feature/rebase", { status: "unknown", url: null }]]))
      .mockResolvedValueOnce(new Map([["feature/rebase", { status: "unknown", url: null }]]))
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === repoRoot && args.join(" ") === "merge-base --is-ancestor feature/rebase main") {
        return gitResult({ exitCode: 0 })
      }
      if (cwd === repoRoot && args.join(" ") === "reflog show --format=%H%x09%gs feature/rebase") {
        return gitResult({ stdout: "" })
      }
      if (cwd === worktreePath && args.join(" ") === "status --porcelain") {
        return gitResult({ stdout: "" })
      }
      if (cwd === worktreePath && args.join(" ") === "rev-parse --abbrev-ref --symbolic-full-name @{upstream}") {
        return gitResult({ exitCode: 1 })
      }
      throw new Error(`unexpected git command: cwd=${cwd} args=${args.join(" ")}`)
    })

    const beforeRebaseSnapshot = await collectWorktreeSnapshot(repoRoot, { baseBranch: "main", ghEnabled: false })
    const afterRebaseSnapshot = await collectWorktreeSnapshot(repoRoot, { baseBranch: "main", ghEnabled: false })

    expect(beforeRebaseSnapshot.worktrees[0]?.merged.overall).toBe(false)
    expect(afterRebaseSnapshot.worktrees[0]?.merged.overall).toBe(false)
  })

  it("marks lifecycle merged when previously diverged head is contained in base", async () => {
    const repoRoot = await createRepoRoot()
    await mkdir(getStateDirectoryPath(repoRoot), { recursive: true })
    const branch = "feature/integrated"
    const worktreePath = join(repoRoot, ".worktree", "feature", "integrated")

    mockedListGitWorktrees
      .mockResolvedValueOnce([
        {
          path: worktreePath,
          head: "diverge123",
          branch,
        } satisfies GitWorktree,
      ])
      .mockResolvedValueOnce([
        {
          path: worktreePath,
          head: "diverge123",
          branch,
        } satisfies GitWorktree,
      ])

    mockedResolvePrStateByBranchBatch
      .mockResolvedValueOnce(new Map([["feature/integrated", { status: "unknown", url: null }]]))
      .mockResolvedValueOnce(new Map([["feature/integrated", { status: "unknown", url: null }]]))

    let branchAncestryChecks = 0
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === repoRoot && args.join(" ") === "merge-base --is-ancestor feature/integrated main") {
        branchAncestryChecks += 1
        return gitResult({ exitCode: branchAncestryChecks === 1 ? 1 : 0 })
      }
      if (cwd === repoRoot && args.join(" ") === "merge-base --is-ancestor diverge123 main") {
        return gitResult({ exitCode: 0 })
      }
      if (cwd === worktreePath && args.join(" ") === "status --porcelain") {
        return gitResult({ stdout: "" })
      }
      if (cwd === worktreePath && args.join(" ") === "rev-parse --abbrev-ref --symbolic-full-name @{upstream}") {
        return gitResult({ exitCode: 1 })
      }
      throw new Error(`unexpected git command: cwd=${cwd} args=${args.join(" ")}`)
    })

    const beforeMergeSnapshot = await collectWorktreeSnapshot(repoRoot, { baseBranch: "main", ghEnabled: false })
    const afterMergeSnapshot = await collectWorktreeSnapshot(repoRoot, { baseBranch: "main", ghEnabled: false })

    expect(beforeMergeSnapshot.worktrees[0]?.merged.overall).toBe(false)
    expect(afterMergeSnapshot.worktrees[0]?.merged.overall).toBe(true)
  })
})
