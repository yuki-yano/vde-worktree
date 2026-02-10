import { describe, expect, it } from "vitest"
import { loadPackageVersion } from "./package-version"

describe("loadPackageVersion", () => {
  it("loads version from first candidate path", () => {
    const requireFn = ((id: string) => {
      if (id === "../package.json") {
        return { version: "1.0.0" }
      }
      throw new Error("unexpected path")
    }) as unknown as NodeJS.Require

    expect(loadPackageVersion(requireFn)).toBe("1.0.0")
  })

  it("falls back to second candidate path when first is missing", () => {
    const requireFn = ((id: string) => {
      if (id === "../package.json") {
        const error = new Error("not found") as Error & { code?: string }
        error.code = "MODULE_NOT_FOUND"
        throw error
      }
      if (id === "../../package.json") {
        return { version: "2.0.0" }
      }
      throw new Error("unexpected path")
    }) as unknown as NodeJS.Require

    expect(loadPackageVersion(requireFn)).toBe("2.0.0")
  })

  it("throws when both candidates are missing", () => {
    const requireFn = ((id: string) => {
      const error = new Error(`not found: ${id}`) as Error & { code?: string }
      error.code = "MODULE_NOT_FOUND"
      throw error
    }) as unknown as NodeJS.Require

    expect(() => loadPackageVersion(requireFn)).toThrow()
  })
})
