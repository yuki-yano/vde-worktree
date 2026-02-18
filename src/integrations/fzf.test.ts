import { describe, expect, it } from "vitest"
import { selectPathWithFzf } from "./fzf"

describe("selectPathWithFzf", () => {
  it("returns selected path", async () => {
    const result = await selectPathWithFzf({
      candidates: ["/repo/.worktree/a", "/repo/.worktree/b"],
      isInteractive: () => true,
      checkFzfAvailability: async () => true,
      runFzf: async ({ input, args }) => {
        expect(input).toBe("/repo/.worktree/a\n/repo/.worktree/b")
        expect(args).toContain("--prompt=worktree> ")
        return { stdout: "/repo/.worktree/b\n" }
      },
    })

    expect(result).toEqual({
      status: "selected",
      path: "/repo/.worktree/b",
    })
  })

  it("returns cancelled when fzf exits with 130", async () => {
    const result = await selectPathWithFzf({
      candidates: ["/repo/.worktree/a"],
      isInteractive: () => true,
      checkFzfAvailability: async () => true,
      runFzf: async () => {
        const error = new Error("cancelled") as Error & { exitCode: number }
        error.exitCode = 130
        throw error
      },
    })

    expect(result).toEqual({ status: "cancelled" })
  })

  it("matches selected line when fzf strips ANSI sequences", async () => {
    const candidate = "\u001b[35m  feature/demo\u001b[39m\t/repo/.worktree/demo\tpreview"
    const result = await selectPathWithFzf({
      candidates: [candidate],
      isInteractive: () => true,
      checkFzfAvailability: async () => true,
      runFzf: async () => {
        return { stdout: "  feature/demo\t/repo/.worktree/demo\tpreview\n" }
      },
    })

    expect(result).toEqual({
      status: "selected",
      path: "  feature/demo\t/repo/.worktree/demo\tpreview",
    })
  })

  it("throws when terminal is not interactive", async () => {
    await expect(
      selectPathWithFzf({
        candidates: ["/repo/.worktree/a"],
        isInteractive: () => false,
        checkFzfAvailability: async () => true,
      }),
    ).rejects.toThrow("interactive terminal")
  })

  it("throws when fzf is unavailable", async () => {
    await expect(
      selectPathWithFzf({
        candidates: ["/repo/.worktree/a"],
        isInteractive: () => true,
        checkFzfAvailability: async () => false,
      }),
    ).rejects.toThrow("fzf is required")
  })

  it("rejects reserved fzf options from --fzf-arg", async () => {
    await expect(
      selectPathWithFzf({
        candidates: ["/repo/.worktree/a"],
        isInteractive: () => true,
        checkFzfAvailability: async () => true,
        fzfExtraArgs: ["--prompt=hack> "],
        runFzf: async () => ({ stdout: "/repo/.worktree/a" }),
      }),
    ).rejects.toThrow("cannot override reserved fzf option")
  })

  it("uses tmux popup args when surface=auto and tmux is available", async () => {
    const result = await selectPathWithFzf({
      candidates: ["/repo/.worktree/a"],
      isInteractive: () => true,
      env: {
        ...process.env,
        TMUX: "/tmp/tmux-1000/default,1234,0",
      },
      surface: "auto",
      tmuxPopupOpts: "90%,80%",
      checkFzfAvailability: async () => true,
      checkFzfTmuxSupport: async () => true,
      runFzf: async ({ args }) => {
        expect(args).toContain("--tmux=90%,80%")
        return { stdout: "/repo/.worktree/a\n" }
      },
    })

    expect(result).toEqual({
      status: "selected",
      path: "/repo/.worktree/a",
    })
  })

  it("falls back to inline when tmux arg is unsupported at runtime", async () => {
    const calls: string[][] = []
    const result = await selectPathWithFzf({
      candidates: ["/repo/.worktree/a"],
      isInteractive: () => true,
      surface: "tmux-popup",
      checkFzfAvailability: async () => true,
      runFzf: async ({ args }) => {
        calls.push(args)
        if (calls.length === 1) {
          throw Object.assign(new Error("unknown option --tmux"), {
            stderr: "unknown option: --tmux",
          })
        }
        return { stdout: "/repo/.worktree/a\n" }
      },
    })

    expect(calls[0]?.some((arg) => arg.startsWith("--tmux="))).toBe(true)
    expect(calls[1]?.some((arg) => arg.startsWith("--tmux="))).toBe(false)
    expect(result).toEqual({
      status: "selected",
      path: "/repo/.worktree/a",
    })
  })
})
