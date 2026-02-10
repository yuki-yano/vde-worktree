import { beforeEach, describe, expect, it, vi } from "vitest"
import { execa } from "execa"
import { doesGitRefExist, runGitCommand } from "./exec"

vi.mock("execa", () => {
  return {
    execa: vi.fn(),
  }
})

const mockedExeca = vi.mocked(execa)

beforeEach(() => {
  mockedExeca.mockReset()
})

describe("runGitCommand", () => {
  it("returns stdout/stderr/exitCode when command succeeds", async () => {
    mockedExeca.mockResolvedValueOnce({
      stdout: "ok",
      stderr: "",
      exitCode: 0,
    } as Awaited<ReturnType<typeof execa>>)

    const result = await runGitCommand({
      cwd: "/repo",
      args: ["status", "--porcelain"],
    })

    expect(result).toEqual({
      stdout: "ok",
      stderr: "",
      exitCode: 0,
    })
    expect(mockedExeca).toHaveBeenCalledWith("git", ["status", "--porcelain"], {
      cwd: "/repo",
      reject: true,
    })
  })

  it("wraps execa errors as GIT_COMMAND_FAILED", async () => {
    const execaError = Object.assign(new Error("git failed"), {
      stderr: "fatal: bad revision",
      stdout: "",
      shortMessage: "fatal: bad revision",
      exitCode: 128,
    })
    mockedExeca.mockRejectedValueOnce(execaError)

    await expect(
      runGitCommand({
        cwd: "/repo",
        args: ["rev-parse", "--verify", "missing"],
      }),
    ).rejects.toMatchObject({
      code: "GIT_COMMAND_FAILED",
      details: {
        cwd: "/repo",
        exitCode: 128,
        command: ["git", "rev-parse", "--verify", "missing"],
        shortMessage: "fatal: bad revision",
      },
    })
  })
})

describe("doesGitRefExist", () => {
  it("returns true only when show-ref exits with 0", async () => {
    mockedExeca
      .mockResolvedValueOnce({
        stdout: "",
        stderr: "",
        exitCode: 0,
      } as Awaited<ReturnType<typeof execa>>)
      .mockResolvedValueOnce({
        stdout: "",
        stderr: "",
        exitCode: 1,
      } as Awaited<ReturnType<typeof execa>>)

    await expect(doesGitRefExist("/repo", "refs/heads/main")).resolves.toBe(true)
    await expect(doesGitRefExist("/repo", "refs/heads/missing")).resolves.toBe(false)

    expect(mockedExeca).toHaveBeenNthCalledWith(1, "git", ["show-ref", "--verify", "--quiet", "refs/heads/main"], {
      cwd: "/repo",
      reject: false,
    })
    expect(mockedExeca).toHaveBeenNthCalledWith(
      2,
      "git",
      ["show-ref", "--verify", "--quiet", "refs/heads/missing"],
      {
        cwd: "/repo",
        reject: false,
      },
    )
  })
})
