import { describe, expect, it } from "vitest"
import { resolveMergedOverall } from "./worktree-state"

describe("resolveMergedOverall", () => {
  it("returns true when either ancestry or PR merge is true", () => {
    expect(
      resolveMergedOverall({
        byAncestry: false,
        byPR: true,
      }),
    ).toBe(true)

    expect(
      resolveMergedOverall({
        byAncestry: true,
        byPR: false,
      }),
    ).toBe(true)
  })

  it("returns false only when both ancestry and PR merge are false", () => {
    expect(
      resolveMergedOverall({
        byAncestry: false,
        byPR: false,
      }),
    ).toBe(false)
  })

  it("falls back to known value when the other side is unknown", () => {
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

    expect(
      resolveMergedOverall({
        byAncestry: null,
        byPR: false,
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
