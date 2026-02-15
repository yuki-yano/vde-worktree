import { describe, expect, it, vi } from "vitest"
import { resolveMergedByPr } from "./gh"

describe("resolveMergedByPr", () => {
  it("returns null when feature is disabled", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: "[]",
      stderr: "",
    }))

    const result = await resolveMergedByPr({
      repoRoot: "/repo",
      branch: "feature/foo",
      baseBranch: "main",
      enabled: false,
      runGh,
    })

    expect(result).toBeNull()
    expect(runGh).not.toHaveBeenCalled()
  })

  it("returns null when base branch is unknown", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: '[{"mergedAt":"2026-02-10T00:00:00Z"}]',
      stderr: "",
    }))

    const result = await resolveMergedByPr({
      repoRoot: "/repo",
      branch: "feature/foo",
      baseBranch: null,
      runGh,
    })

    expect(result).toBeNull()
    expect(runGh).not.toHaveBeenCalled()
  })

  it("returns true when merged PR exists", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: '[{"mergedAt":"2026-02-10T00:00:00Z"}]',
      stderr: "",
    }))

    const result = await resolveMergedByPr({
      repoRoot: "/repo",
      branch: "feature/foo",
      baseBranch: "main",
      runGh,
    })

    expect(result).toBe(true)
    expect(runGh).toHaveBeenCalledWith({
      cwd: "/repo",
      args: [
        "pr",
        "list",
        "--state",
        "merged",
        "--head",
        "feature/foo",
        "--base",
        "main",
        "--limit",
        "1",
        "--json",
        "mergedAt",
      ],
    })
  })

  it("returns false when no merged PR exists", async () => {
    const result = await resolveMergedByPr({
      repoRoot: "/repo",
      branch: "feature/foo",
      baseBranch: "main",
      runGh: async () => ({
        exitCode: 0,
        stdout: "[]",
        stderr: "",
      }),
    })

    expect(result).toBe(false)
  })

  it("returns null when gh command fails", async () => {
    const result = await resolveMergedByPr({
      repoRoot: "/repo",
      branch: "feature/foo",
      baseBranch: "main",
      runGh: async () => ({
        exitCode: 1,
        stdout: "",
        stderr: "not logged in",
      }),
    })

    expect(result).toBeNull()
  })

  it("returns null on invalid JSON", async () => {
    const result = await resolveMergedByPr({
      repoRoot: "/repo",
      branch: "feature/foo",
      baseBranch: "main",
      runGh: async () => ({
        exitCode: 0,
        stdout: "invalid-json",
        stderr: "",
      }),
    })

    expect(result).toBeNull()
  })
})
