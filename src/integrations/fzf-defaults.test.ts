import { beforeEach, describe, expect, it, vi } from "vitest"

vi.mock("execa", () => {
  return {
    execa: vi.fn(),
  }
})

import { execa } from "execa"
import { selectPathWithFzf } from "./fzf"

const mockedExeca = vi.mocked(execa)

const execaResult = ({
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
  } as Awaited<ReturnType<typeof execa>>
}

beforeEach(() => {
  mockedExeca.mockReset()
})

describe("selectPathWithFzf defaults", () => {
  it("uses default fzf availability check and run, and sanitizes candidates", async () => {
    mockedExeca.mockResolvedValueOnce(execaResult({ stdout: "0.48.1" })).mockResolvedValueOnce(
      execaResult({
        stdout: "a b",
      }),
    )

    const env = { ...process.env, TEST_FLAG: "1" }
    const result = await selectPathWithFzf({
      candidates: ["a\nb", "\t", "a b"],
      isInteractive: () => true,
      prompt: "pick> ",
      fzfExtraArgs: ["--ansi"],
      cwd: "/repo",
      env,
    })

    expect(result).toEqual({
      status: "selected",
      path: "a b",
    })
    expect(mockedExeca).toHaveBeenNthCalledWith(1, "fzf", ["--version"], {
      timeout: 5000,
    })
    expect(mockedExeca).toHaveBeenNthCalledWith(
      2,
      "fzf",
      ["--prompt=pick> ", "--layout=reverse", "--height=80%", "--border", "--ansi"],
      {
        input: "a b\na b",
        cwd: "/repo",
        env,
        stderr: "inherit",
      },
    )
  })

  it("returns cancelled when fzf returns empty output", async () => {
    mockedExeca.mockResolvedValueOnce(execaResult({ stdout: "0.48.1" })).mockResolvedValueOnce(
      execaResult({
        stdout: "\n",
      }),
    )

    await expect(
      selectPathWithFzf({
        candidates: ["feature/a"],
        isInteractive: () => true,
      }),
    ).resolves.toEqual({
      status: "cancelled",
    })
  })

  it("returns cancelled when fzf exits with 130", async () => {
    mockedExeca.mockResolvedValueOnce(execaResult({ stdout: "0.48.1" })).mockRejectedValueOnce(
      Object.assign(new Error("cancel"), {
        exitCode: 130,
      }),
    )

    await expect(
      selectPathWithFzf({
        candidates: ["feature/a"],
        isInteractive: () => true,
      }),
    ).resolves.toEqual({
      status: "cancelled",
    })
  })

  it("throws when fzf returns a value not in the candidate list", async () => {
    mockedExeca.mockResolvedValueOnce(execaResult({ stdout: "0.48.1" })).mockResolvedValueOnce(
      execaResult({
        stdout: "unknown",
      }),
    )

    await expect(
      selectPathWithFzf({
        candidates: ["feature/a"],
        isInteractive: () => true,
      }),
    ).rejects.toThrow("not in the candidate list")
  })

  it("throws when all candidates are empty after sanitization", async () => {
    mockedExeca.mockResolvedValueOnce(execaResult({ stdout: "0.48.1" }))

    await expect(
      selectPathWithFzf({
        candidates: ["\n", "\t", "\r"],
        isInteractive: () => true,
      }),
    ).rejects.toThrow("All candidates are empty after sanitization")
  })

  it("maps missing/timeout fzf binary to dependency error", async () => {
    mockedExeca
      .mockRejectedValueOnce(
        Object.assign(new Error("missing"), {
          code: "ENOENT",
        }),
      )
      .mockRejectedValueOnce(
        Object.assign(new Error("timeout"), {
          code: "ERR_EXECA_TIMEOUT",
        }),
      )
      .mockRejectedValueOnce(
        Object.assign(new Error("timeout"), {
          timedOut: true,
        }),
      )

    await expect(
      selectPathWithFzf({
        candidates: ["feature/a"],
        isInteractive: () => true,
      }),
    ).rejects.toThrow("fzf is required")

    await expect(
      selectPathWithFzf({
        candidates: ["feature/a"],
        isInteractive: () => true,
      }),
    ).rejects.toThrow("fzf is required")

    await expect(
      selectPathWithFzf({
        candidates: ["feature/a"],
        isInteractive: () => true,
      }),
    ).rejects.toThrow("fzf is required")
  })

  it("rethrows unexpected availability check errors", async () => {
    mockedExeca.mockRejectedValueOnce(
      Object.assign(new Error("permission denied"), {
        code: "EACCES",
      }),
    )

    await expect(
      selectPathWithFzf({
        candidates: ["feature/a"],
        isInteractive: () => true,
      }),
    ).rejects.toThrow("permission denied")
  })
})
