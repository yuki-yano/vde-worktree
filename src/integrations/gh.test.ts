import { describe, expect, it, vi } from "vitest"
import {
  GhUnavailableError,
  resolveMergedByPrBatch,
  resolvePrStateByBranchBatch,
  resolvePrStatusByBranchBatch,
} from "./gh"

describe("resolvePrStateByBranchBatch", () => {
  it("returns unknown states when feature is disabled", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: "[]",
      stderr: "",
    }))

    const result = await resolvePrStateByBranchBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo"],
      enabled: false,
      runGh,
    })

    expect(result.get("feature/foo")).toEqual({
      status: "unknown",
      url: null,
    })
    expect(runGh).not.toHaveBeenCalled()
  })

  it("returns empty map when base branch is unknown", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: "[]",
      stderr: "",
    }))

    const result = await resolvePrStateByBranchBatch({
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
      stdout: "[]",
      stderr: "",
    }))

    const result = await resolvePrStateByBranchBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: [null, "main", "main"],
      runGh,
    })

    expect(result.size).toBe(0)
    expect(runGh).not.toHaveBeenCalled()
  })

  it("resolves none/open/merged/closed_unmerged and urls from PR records", async () => {
    const runGh = vi.fn(async () => ({
      exitCode: 0,
      stdout: JSON.stringify([
        {
          headRefName: "feature/open",
          state: "OPEN",
          mergedAt: null,
          updatedAt: "2026-02-10T10:00:00Z",
          url: "https://github.com/example/repo/pull/101",
        },
        {
          headRefName: "feature/merged",
          state: "MERGED",
          mergedAt: "2026-02-10T00:00:00Z",
          updatedAt: "2026-02-10T11:00:00Z",
          url: "https://github.com/example/repo/pull/102",
        },
        {
          headRefName: "feature/closed",
          state: "CLOSED",
          mergedAt: null,
          updatedAt: "2026-02-10T12:00:00Z",
          url: "https://github.com/example/repo/pull/103",
        },
      ]),
      stderr: "",
    }))

    const result = await resolvePrStateByBranchBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["main", "feature/open", "feature/merged", "feature/closed", "feature/none", null],
      runGh,
    })

    expect(result.get("feature/open")).toEqual({
      status: "open",
      url: "https://github.com/example/repo/pull/101",
    })
    expect(result.get("feature/merged")).toEqual({
      status: "merged",
      url: "https://github.com/example/repo/pull/102",
    })
    expect(result.get("feature/closed")).toEqual({
      status: "closed_unmerged",
      url: "https://github.com/example/repo/pull/103",
    })
    expect(result.get("feature/none")).toEqual({
      status: "none",
      url: null,
    })
    expect(result.has("main")).toBe(false)
    expect(runGh).toHaveBeenCalledWith({
      cwd: "/repo",
      args: [
        "pr",
        "list",
        "--state",
        "all",
        "--base",
        "main",
        "--search",
        "head:feature/open OR head:feature/merged OR head:feature/closed OR head:feature/none",
        "--limit",
        "1000",
        "--json",
        "headRefName,state,mergedAt,updatedAt,url",
      ],
    })
  })

  it("prefers latest updated PR when branch has multiple records", async () => {
    const result = await resolvePrStateByBranchBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo"],
      runGh: async () => ({
        exitCode: 0,
        stdout: JSON.stringify([
          {
            headRefName: "feature/foo",
            state: "MERGED",
            mergedAt: "2026-02-10T00:00:00Z",
            updatedAt: "2026-02-10T00:00:00Z",
            url: "https://github.com/example/repo/pull/201",
          },
          {
            headRefName: "feature/foo",
            state: "OPEN",
            mergedAt: null,
            updatedAt: "2026-02-11T00:00:00Z",
            url: "https://github.com/example/repo/pull/202",
          },
        ]),
        stderr: "",
      }),
    })

    expect(result.get("feature/foo")).toEqual({
      status: "open",
      url: "https://github.com/example/repo/pull/202",
    })
  })

  it("returns unknown states when gh command fails", async () => {
    const result = await resolvePrStateByBranchBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo", "feature/bar"],
      runGh: async () => ({
        exitCode: 1,
        stdout: "",
        stderr: "not logged in",
      }),
    })

    expect(result.get("feature/foo")).toEqual({
      status: "unknown",
      url: null,
    })
    expect(result.get("feature/bar")).toEqual({
      status: "unknown",
      url: null,
    })
  })

  it("returns unknown states on invalid JSON", async () => {
    const result = await resolvePrStateByBranchBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo", "feature/bar"],
      runGh: async () => ({
        exitCode: 0,
        stdout: "invalid-json",
        stderr: "",
      }),
    })

    expect(result.get("feature/foo")).toEqual({
      status: "unknown",
      url: null,
    })
    expect(result.get("feature/bar")).toEqual({
      status: "unknown",
      url: null,
    })
  })

  it("returns unknown states when gh runner raises typed unavailable error", async () => {
    const result = await resolvePrStateByBranchBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo"],
      runGh: async () => {
        throw new GhUnavailableError()
      },
    })

    expect(result.get("feature/foo")).toEqual({
      status: "unknown",
      url: null,
    })
  })
})

describe("resolvePrStatusByBranchBatch", () => {
  it("maps pr state map to status-only map", async () => {
    const result = await resolvePrStatusByBranchBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/foo"],
      runGh: async () => ({
        exitCode: 0,
        stdout: JSON.stringify([
          {
            headRefName: "feature/foo",
            state: "OPEN",
            mergedAt: null,
            updatedAt: "2026-02-10T00:00:00Z",
            url: "https://github.com/example/repo/pull/300",
          },
        ]),
        stderr: "",
      }),
    })

    expect(result.get("feature/foo")).toBe("open")
  })
})

describe("resolveMergedByPrBatch", () => {
  it("maps pr status to merged boolean/null", async () => {
    const result = await resolveMergedByPrBatch({
      repoRoot: "/repo",
      baseBranch: "main",
      branches: ["feature/none", "feature/open", "feature/merged", "feature/closed", "feature/unknown"],
      runGh: async () => ({
        exitCode: 0,
        stdout: JSON.stringify([
          {
            headRefName: "feature/open",
            state: "OPEN",
            mergedAt: null,
            updatedAt: "2026-02-10T00:00:00Z",
            url: "https://github.com/example/repo/pull/401",
          },
          {
            headRefName: "feature/merged",
            state: "MERGED",
            mergedAt: "2026-02-10T00:00:00Z",
            updatedAt: "2026-02-10T00:00:00Z",
            url: "https://github.com/example/repo/pull/402",
          },
          {
            headRefName: "feature/closed",
            state: "CLOSED",
            mergedAt: null,
            updatedAt: "2026-02-10T00:00:00Z",
            url: "https://github.com/example/repo/pull/403",
          },
          {
            headRefName: "feature/unknown",
            state: "SOMETHING_NEW",
            mergedAt: null,
            updatedAt: "2026-02-10T00:00:00Z",
            url: "https://github.com/example/repo/pull/404",
          },
        ]),
        stderr: "",
      }),
    })

    expect(result.get("feature/none")).toBe(false)
    expect(result.get("feature/open")).toBe(false)
    expect(result.get("feature/merged")).toBe(true)
    expect(result.get("feature/closed")).toBe(false)
    expect(result.get("feature/unknown")).toBeNull()
  })
})
