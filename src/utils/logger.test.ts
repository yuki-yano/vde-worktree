import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { createLogger, LogLevel } from "./logger"

const ENV_KEYS = ["VDE_WORKTREE_DEBUG", "VDE_DEBUG", "VDE_WORKTREE_VERBOSE", "VDE_VERBOSE"] as const

const envBackup = new Map<string, string | undefined>()

beforeEach(() => {
  for (const key of ENV_KEYS) {
    envBackup.set(key, process.env[key])
    delete process.env[key]
  }
})

afterEach(() => {
  for (const key of ENV_KEYS) {
    const value = envBackup.get(key)
    if (value === undefined) {
      delete process.env[key]
    } else {
      process.env[key] = value
    }
  }
  envBackup.clear()
  vi.restoreAllMocks()
})

describe("createLogger", () => {
  it("resolves default level from environment variables", () => {
    expect(createLogger().level).toBe(LogLevel.WARN)

    process.env.VDE_VERBOSE = "true"
    expect(createLogger().level).toBe(LogLevel.INFO)

    process.env.VDE_DEBUG = "true"
    expect(createLogger().level).toBe(LogLevel.DEBUG)
  })

  it("applies level filtering for warn/info/debug and always prints success", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined)
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => undefined)
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => undefined)
    const logger = createLogger({ level: LogLevel.WARN, prefix: "[vw]" })

    logger.error("failed")
    logger.warn("watch out")
    logger.info("info message")
    logger.debug("debug message")
    logger.success("done")

    expect(errorSpy).toHaveBeenCalledTimes(1)
    expect(warnSpy).toHaveBeenCalledTimes(1)
    expect(logSpy).toHaveBeenCalledTimes(1)
    expect(String(errorSpy.mock.calls[0]?.[0])).toContain("[vw] Error: failed")
    expect(String(warnSpy.mock.calls[0]?.[0])).toContain("[vw] watch out")
    expect(String(logSpy.mock.calls[0]?.[0])).toContain("[vw] done")
  })

  it("prints stack trace only in debug mode", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined)
    const logger = createLogger({ level: LogLevel.ERROR, prefix: "[vw]" })
    const error = new Error("boom")
    error.stack = "mock-stack"

    logger.error("failed", error)
    expect(errorSpy).toHaveBeenCalledTimes(1)

    process.env.VDE_WORKTREE_DEBUG = "true"
    const debugLogger = createLogger({ level: LogLevel.ERROR, prefix: "[vw]" })
    debugLogger.error("failed", error)
    expect(errorSpy).toHaveBeenCalledTimes(3)
    expect(String(errorSpy.mock.calls[2]?.[0])).toContain("mock-stack")
  })

  it("inherits prefix and level in child logger", () => {
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => undefined)
    const parent = createLogger({ level: LogLevel.DEBUG, prefix: "[root]" })
    const child = parent.createChild("[child]")

    child.debug("trace")

    expect(child.level).toBe(LogLevel.DEBUG)
    expect(child.prefix).toBe("[root] [child]")
    expect(logSpy).toHaveBeenCalledTimes(1)
    expect(String(logSpy.mock.calls[0]?.[0])).toContain("[root] [child] [DEBUG] trace")
  })
})
