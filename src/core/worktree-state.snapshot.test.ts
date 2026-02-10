import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

vi.mock("../git/exec", () => {
  return {
    runGitCommand: vi.fn(),
    doesGitRefExist: vi.fn(),
  }
})

vi.mock("../git/worktree", () => {
  return {
    listGitWorktrees: vi.fn(),
  }
})

vi.mock("../integrations/gh", () => {
  return {
    resolveMergedByPr: vi.fn(),
  }
})

import { doesGitRefExist, runGitCommand } from "../git/exec"
import { listGitWorktrees, type GitWorktree } from "../git/worktree"
import { resolveMergedByPr } from "../integrations/gh"
import { branchToWorktreeId, getLocksDirectoryPath } from "./paths"
import { collectWorktreeSnapshot } from "./worktree-state"

const mockedRunGitCommand = vi.mocked(runGitCommand)
const mockedDoesGitRefExist = vi.mocked(doesGitRefExist)
const mockedListGitWorktrees = vi.mocked(listGitWorktrees)
const mockedResolveMergedByPr = vi.mocked(resolveMergedByPr)

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
  mockedDoesGitRefExist.mockReset()
  mockedListGitWorktrees.mockReset()
  mockedResolveMergedByPr.mockReset()
})

describe("collectWorktreeSnapshot", () => {
  it("uses explicit base branch and resolves lock/merged/upstream states", async () => {
    const repoRoot = await createRepoRoot()
    const branch = "feature/a"
    const worktreePath = join(repoRoot, ".worktree", "feature%2Fa")
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

    mockedResolveMergedByPr.mockResolvedValueOnce(null)
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === repoRoot && args.join(" ") === "config --get vde-worktree.baseBranch") {
        return gitResult({ stdout: "trunk\n" })
      }
      if (cwd === repoRoot && args.join(" ") === "config --bool --get vde-worktree.enableGh") {
        return gitResult({ stdout: "false\n" })
      }
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

    const snapshot = await collectWorktreeSnapshot(repoRoot)
    expect(snapshot.baseBranch).toBe("trunk")
    expect(snapshot.repoRoot).toBe(repoRoot)
    expect(mockedDoesGitRefExist).not.toHaveBeenCalled()
    expect(mockedResolveMergedByPr).toHaveBeenCalledWith({
      repoRoot,
      branch: "feature/a",
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
    const featurePath = join(repoRoot, ".worktree", "feature%2Fb")

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

    mockedDoesGitRefExist.mockResolvedValueOnce(false).mockResolvedValueOnce(true)
    mockedResolveMergedByPr.mockResolvedValueOnce(true)
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === repoRoot && args.join(" ") === "config --get vde-worktree.baseBranch") {
        return gitResult({ exitCode: 1 })
      }
      if (cwd === repoRoot && args.join(" ") === "config --bool --get vde-worktree.enableGh") {
        return gitResult({ exitCode: 1 })
      }
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

    const snapshot = await collectWorktreeSnapshot(repoRoot)
    expect(snapshot.baseBranch).toBe("master")
    expect(mockedDoesGitRefExist).toHaveBeenNthCalledWith(1, repoRoot, "refs/heads/main")
    expect(mockedDoesGitRefExist).toHaveBeenNthCalledWith(2, repoRoot, "refs/heads/master")
    expect(mockedResolveMergedByPr).toHaveBeenCalledTimes(1)
    expect(mockedResolveMergedByPr).toHaveBeenCalledWith({
      repoRoot,
      branch: "feature/b",
      enabled: true,
    })

    expect(snapshot.worktrees[0]).toEqual({
      branch: null,
      path: detachedPath,
      head: "h1",
      dirty: false,
      locked: { value: false, reason: null, owner: null },
      merged: { byAncestry: null, byPR: null, overall: null },
      upstream: { ahead: null, behind: null, remote: null },
    })
    expect(snapshot.worktrees[1]).toEqual({
      branch: "feature/b",
      path: featurePath,
      head: "h2",
      dirty: false,
      locked: { value: false, reason: null, owner: null },
      merged: { byAncestry: true, byPR: true, overall: true },
      upstream: { ahead: null, behind: null, remote: null },
    })
  })

  it("marks lock metadata as invalid and handles unparsable upstream distance", async () => {
    const repoRoot = await createRepoRoot()
    const branch = "feature/c"
    const worktreePath = join(repoRoot, ".worktree", "feature%2Fc")
    const lockPath = join(getLocksDirectoryPath(repoRoot), `${branchToWorktreeId(branch)}.json`)

    await mkdir(lockPath, { recursive: true })

    mockedListGitWorktrees.mockResolvedValueOnce([
      {
        path: worktreePath,
        head: "h3",
        branch,
      } satisfies GitWorktree,
    ])
    mockedDoesGitRefExist.mockResolvedValueOnce(true)
    mockedResolveMergedByPr.mockResolvedValueOnce(false)
    mockedRunGitCommand.mockImplementation(async ({ cwd, args }) => {
      if (cwd === repoRoot && args.join(" ") === "config --get vde-worktree.baseBranch") {
        return gitResult({ exitCode: 1 })
      }
      if (cwd === repoRoot && args.join(" ") === "config --bool --get vde-worktree.enableGh") {
        return gitResult({ stdout: "off\n" })
      }
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

    const snapshot = await collectWorktreeSnapshot(repoRoot)
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
      upstream: {
        ahead: null,
        behind: null,
        remote: "origin/feature/c",
      },
    })
  })
})
