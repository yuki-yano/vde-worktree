import { describe, expect, it } from "vitest"
import { resolveMergedOverall } from "./worktree-state"

describe("resolveMergedOverall", () => {
  it("returns true when PR merge is true", () => {
    expect(
      resolveMergedOverall({
        byAncestry: false,
        byPR: true,
      }),
    ).toBe(true)
  })

  it("returns false when PR merge is false", () => {
    expect(
      resolveMergedOverall({
        byAncestry: true,
        byPR: false,
      }),
    ).toBe(false)
  })

  it("falls back to ancestry when PR is unknown", () => {
    expect(
      resolveMergedOverall({
        byAncestry: true,
        byPR: null,
      }),
    ).toBe(true)

    expect(
      resolveMergedOverall({
        byAncestry: false,
        byPR: null,
      }),
    ).toBe(false)
  })

  it("returns null when both are unknown", () => {
    expect(
      resolveMergedOverall({
        byAncestry: null,
        byPR: null,
      }),
    ).toBeNull()
  })
})
