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
})
