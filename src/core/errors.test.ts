import { describe, expect, it } from "vitest"
import { createCliError, ensureCliError, type ErrorCode } from "./errors"

describe("errors", () => {
  it("creates CliError with mapped exit code and details", () => {
    const error = createCliError("HOOK_TIMEOUT", {
      message: "hook timeout",
      details: { hook: "post-switch" },
    })

    expect(error.code).toBe("HOOK_TIMEOUT")
    expect(error.exitCode).toBe(10)
    expect(error.details).toEqual({ hook: "post-switch" })
    expect(error.message).toBe("hook timeout")
  })

  it("returns the same object when ensureCliError receives CliError", () => {
    const original = createCliError("NOT_GIT_REPOSITORY", {
      message: "not git",
    })

    const resolved = ensureCliError(original)
    expect(resolved).toBe(original)
  })

  it("wraps regular Error as INTERNAL_ERROR", () => {
    const input = new Error("boom")
    const resolved = ensureCliError(input)

    expect(resolved.code).toBe("INTERNAL_ERROR")
    expect(resolved.exitCode).toBe(30)
    expect(resolved.message).toBe("boom")
    expect(resolved.cause).toBe(input)
  })

  it("wraps non-Error values as INTERNAL_ERROR with stringified detail", () => {
    const resolved = ensureCliError({ foo: "bar" })

    expect(resolved.code).toBe("INTERNAL_ERROR")
    expect(resolved.exitCode).toBe(30)
    expect(resolved.message).toBe("An unexpected error occurred")
    expect(resolved.details.value).toBe("[object Object]")
  })

  it("maintains exit-code mapping for representative safety errors", () => {
    const cases: ReadonlyArray<[ErrorCode, number]> = [
      ["UNSAFE_FLAG_REQUIRED", 4],
      ["WORKTREE_NOT_FOUND", 4],
      ["INVALID_CONFIG", 3],
      ["DEPENDENCY_MISSING", 5],
      ["GIT_COMMAND_FAILED", 20],
    ]

    for (const [code, exitCode] of cases) {
      const error = createCliError(code, { message: code })
      expect(error.exitCode).toBe(exitCode)
    }
  })
})
