import { describe, expect, it } from "vitest"
import { resolveMergedOverall } from "./worktree-state"

describe("resolveMergedOverall", () => {
  it("returns true when PR merge is true", () => {
    expect(
      resolveMergedOverall({
        byAncestry: false,
        byPR: true,
        byLifecycle: false,
      }),
    ).toBe(true)
  })

  it("returns true when lifecycle merge is true", () => {
    expect(
      resolveMergedOverall({
        byAncestry: true,
        byPR: null,
        byLifecycle: true,
      }),
    ).toBe(true)
  })

  it("returns false when ancestry is false", () => {
    expect(
      resolveMergedOverall({
        byAncestry: false,
        byPR: null,
        byLifecycle: null,
      }),
    ).toBe(false)
  })

  it("returns false when ancestry is true but no lifecycle merge evidence exists", () => {
    expect(
      resolveMergedOverall({
        byAncestry: true,
        byPR: null,
        byLifecycle: false,
      }),
    ).toBe(false)
  })

  it("returns false when PR explicitly indicates no merge", () => {
    expect(
      resolveMergedOverall({
        byAncestry: null,
        byPR: false,
        byLifecycle: null,
      }),
    ).toBe(false)
  })

  it("returns false when lifecycle explicitly indicates no merge", () => {
    expect(
      resolveMergedOverall({
        byAncestry: null,
        byPR: null,
        byLifecycle: false,
      }),
    ).toBe(false)
  })

  it("returns null when both are unknown", () => {
    expect(
      resolveMergedOverall({
        byAncestry: null,
        byPR: null,
        byLifecycle: null,
      }),
    ).toBeNull()
  })
})
