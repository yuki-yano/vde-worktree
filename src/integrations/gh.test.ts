import { describe, expect, it, vi } from "vitest"
import { resolveMergedByPrBatch } from "./gh"

describe("resolveMergedByPrBatch", () => {
  it("returns null when feature is disabled", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: "[]",
      stderr: "",
    }))

    const result = await resolveMergedByPrBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo"],
      enabled: false,
      runGh,
    })

    expect(result.size).toBe(0)
    expect(runGh).not.toHaveBeenCalled()
  })

  it("returns empty map when base branch is unknown", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: '[{"mergedAt":"2026-02-10T00:00:00Z"}]',
      stderr: "",
    }))

    const result = await resolveMergedByPrBatch({
      repoRoot: "/repo",
      baseBranch: null,
      branches: ["feature/foo"],
      runGh,
    })

    expect(result.size).toBe(0)
    expect(runGh).not.toHaveBeenCalled()
  })

  it("returns empty map when branch candidates are empty", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: '[{"mergedAt":"2026-02-10T00:00:00Z"}]',
      stderr: "",
    }))

    const result = await resolveMergedByPrBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: [null, "main", "main"],
      runGh,
    })

    expect(result.size).toBe(0)
    expect(runGh).not.toHaveBeenCalled()
  })

  it("returns merged state for each non-base branch", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout:
        '[{"headRefName":"feature/foo","mergedAt":"2026-02-10T00:00:00Z"},{"headRefName":"feature/bar","mergedAt":null}]',
      stderr: "",
    }))

    const result = await resolveMergedByPrBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["main", "feature/foo", "feature/bar", "feature/foo", null],
      runGh,
    })

    expect(result.get("feature/foo")).toBe(true)
    expect(result.get("feature/bar")).toBe(false)
    expect(result.has("main")).toBe(false)
    expect(runGh).toHaveBeenCalledWith({
      cwd: "/repo",
      args: [
        "pr",
        "list",
        "--state",
        "merged",
        "--base",
        "main",
        "--search",
        "head:feature/foo OR head:feature/bar",
        "--limit",
        "1000",
        "--json",
        "headRefName,mergedAt",
      ],
    })
  })

  it("returns false for branches when no merged PR exists", async () => {
    const result = await resolveMergedByPrBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo", "feature/bar"],
      runGh: async () => ({
        exitCode: 0,
        stdout: "[]",
        stderr: "",
      }),
    })

    expect(result.get("feature/foo")).toBe(false)
    expect(result.get("feature/bar")).toBe(false)
  })

  it("returns null states when gh command fails", async () => {
    const result = await resolveMergedByPrBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo", "feature/bar"],
      runGh: async () => ({
        exitCode: 1,
        stdout: "",
        stderr: "not logged in",
      }),
    })

    expect(result.get("feature/foo")).toBeNull()
    expect(result.get("feature/bar")).toBeNull()
  })

  it("returns null states on invalid JSON", async () => {
    const result = await resolveMergedByPrBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo", "feature/bar"],
      runGh: async () => ({
        exitCode: 0,
        stdout: "invalid-json",
        stderr: "",
      }),
    })

    expect(result.get("feature/foo")).toBeNull()
    expect(result.get("feature/bar")).toBeNull()
  })
})
