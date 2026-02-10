import { describe, expect, it, vi } from "vitest"

vi.mock("../git/exec", () => {
  return {
    runGitCommand: vi.fn(),
  }
})

import { runGitCommand } from "../git/exec"
import {
  branchToWorktreeId,
  branchToWorktreePath,
  ensurePathInsideRepo,
  getHooksDirectoryPath,
  getLocksDirectoryPath,
  getLogsDirectoryPath,
  getStateDirectoryPath,
  getWorktreeMetaRootPath,
  getWorktreeRootPath,
  resolvePathFromCwd,
  resolveRepoContext,
  resolveRepoRelativePath,
} from "./paths"

const mockedRunGitCommand = vi.mocked(runGitCommand)

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

describe("paths", () => {
  it("resolves repo context from git common dir", async () => {
    mockedRunGitCommand.mockReset()
    mockedRunGitCommand.mockImplementation(async ({ args }) => {
      if (args.join(" ") === "rev-parse --show-toplevel") {
        return gitResult({
          stdout: "/repo/.worktree/feature\n",
        })
      }
      if (args.join(" ") === "rev-parse --path-format=absolute --git-common-dir") {
        return gitResult({
          stdout: "/repo/.git\n",
        })
      }
      throw new Error(`unexpected git args: ${args.join(" ")}`)
    })

    const context = await resolveRepoContext("/repo/.worktree/feature")
    expect(context).toEqual({
      repoRoot: "/repo",
      currentWorktreeRoot: "/repo/.worktree/feature",
      gitCommonDir: "/repo/.git",
    })
  })

  it("falls back to current worktree root when git common dir resolution fails", async () => {
    mockedRunGitCommand.mockReset()
    mockedRunGitCommand.mockImplementation(async ({ args }) => {
      if (args.join(" ") === "rev-parse --show-toplevel") {
        return gitResult({
          stdout: "/repo/.worktree/feature\n",
        })
      }
      if (args.join(" ") === "rev-parse --path-format=absolute --git-common-dir") {
        return gitResult({
          stdout: "",
          exitCode: 1,
        })
      }
      throw new Error(`unexpected git args: ${args.join(" ")}`)
    })

    const context = await resolveRepoContext("/repo/.worktree/feature")
    expect(context).toEqual({
      repoRoot: "/repo/.worktree/feature",
      currentWorktreeRoot: "/repo/.worktree/feature",
      gitCommonDir: "/repo/.worktree/feature/.git",
    })
  })

  it("throws NOT_GIT_REPOSITORY when not in a git repository", async () => {
    mockedRunGitCommand.mockReset()
    mockedRunGitCommand.mockResolvedValueOnce(
      gitResult({
        exitCode: 128,
      }),
    )

    await expect(resolveRepoContext("/tmp/outside")).rejects.toMatchObject({
      code: "NOT_GIT_REPOSITORY",
    })
  })

  it("returns expected managed directories", () => {
    expect(getWorktreeRootPath("/repo")).toBe("/repo/.worktree")
    expect(getWorktreeMetaRootPath("/repo")).toBe("/repo/.vde/worktree")
    expect(getHooksDirectoryPath("/repo")).toBe("/repo/.vde/worktree/hooks")
    expect(getLogsDirectoryPath("/repo")).toBe("/repo/.vde/worktree/logs")
    expect(getLocksDirectoryPath("/repo")).toBe("/repo/.vde/worktree/locks")
    expect(getStateDirectoryPath("/repo")).toBe("/repo/.vde/worktree/state")
    expect(branchToWorktreeId("feature/a b")).toBe("feature%2Fa%20b")
    expect(branchToWorktreePath("/repo", "feature/a b")).toBe("/repo/.worktree/feature%2Fa%20b")
  })

  it("ensures path is inside repository", () => {
    expect(ensurePathInsideRepo({ repoRoot: "/repo", path: "/repo" })).toBe("/repo")
    expect(ensurePathInsideRepo({ repoRoot: "/repo", path: "/repo/sub/file.txt" })).toBe("/repo/sub/file.txt")

    expect(() =>
      ensurePathInsideRepo({
        repoRoot: "/repo",
        path: "/outside/file.txt",
      }),
    ).toThrow("outside repository root")
  })

  it("resolves relative repo paths and blocks absolute/traversal paths", () => {
    expect(
      resolveRepoRelativePath({
        repoRoot: "/repo",
        relativePath: "./dir/../file.txt",
      }),
    ).toBe("/repo/file.txt")

    expect(() =>
      resolveRepoRelativePath({
        repoRoot: "/repo",
        relativePath: "/etc/passwd",
      }),
    ).toThrow("Absolute path is not allowed")

    expect(() =>
      resolveRepoRelativePath({
        repoRoot: "/repo",
        relativePath: "../../outside",
      }),
    ).toThrow("outside repository root")
  })

  it("resolves path from cwd", () => {
    expect(resolvePathFromCwd({ cwd: "/repo", path: "a/b" })).toBe("/repo/a/b")
    expect(resolvePathFromCwd({ cwd: "/repo", path: "/tmp/a" })).toBe("/tmp/a")
  })
})
