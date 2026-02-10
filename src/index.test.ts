import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const createCliMock = vi.fn()

vi.mock("./cli/index", () => {
  return {
    createCli: createCliMock,
  }
})

const flushMain = async (): Promise<void> => {
  await Promise.resolve()
  await new Promise<void>((resolve) => {
    setTimeout(resolve, 0)
  })
}

describe("index entrypoint", () => {
  const envBackup = new Map<string, string | undefined>()

  beforeEach(() => {
    vi.resetModules()
    createCliMock.mockReset()
    for (const key of ["VDE_WORKTREE_DEBUG", "VDE_DEBUG"] as const) {
      envBackup.set(key, process.env[key])
      delete process.env[key]
    }
  })

  afterEach(() => {
    for (const [key, value] of envBackup.entries()) {
      if (value === undefined) {
        delete process.env[key]
      } else {
        process.env[key] = value
      }
    }
    envBackup.clear()
    vi.restoreAllMocks()
  })

  it("exits with returned non-zero code", async () => {
    const exitSpy = vi.spyOn(process, "exit").mockImplementation((() => undefined) as never)
    createCliMock.mockReturnValue({
      run: vi.fn(async () => 7),
    })

    await import("./index")
    await flushMain()

    expect(exitSpy).toHaveBeenCalledWith(7)
  })

  it("prints error message and exits 1 when cli throws Error", async () => {
    const exitSpy = vi.spyOn(process, "exit").mockImplementation((() => undefined) as never)
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined)
    createCliMock.mockReturnValue({
      run: vi.fn(async () => {
        throw new Error("boom")
      }),
    })

    await import("./index")
    await flushMain()

    expect(errorSpy).toHaveBeenCalledWith("Error:", "boom")
    expect(exitSpy).toHaveBeenCalledWith(1)
  })

  it("prints stack trace in debug mode", async () => {
    process.env.VDE_WORKTREE_DEBUG = "true"
    const exitSpy = vi.spyOn(process, "exit").mockImplementation((() => undefined) as never)
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined)
    const error = new Error("boom")
    error.stack = "custom-stack"
    createCliMock.mockReturnValue({
      run: vi.fn(async () => {
        throw error
      }),
    })

    await import("./index")
    await flushMain()

    expect(errorSpy).toHaveBeenNthCalledWith(1, "Error:", "boom")
    expect(errorSpy).toHaveBeenNthCalledWith(2, "custom-stack")
    expect(exitSpy).toHaveBeenCalledWith(1)
  })

  it("prints fallback message for non-Error throws", async () => {
    const exitSpy = vi.spyOn(process, "exit").mockImplementation((() => undefined) as never)
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined)
    createCliMock.mockReturnValue({
      run: vi.fn(async () => {
        throw "failed"
      }),
    })

    await import("./index")
    await flushMain()

    expect(errorSpy).toHaveBeenCalledWith("An unexpected error occurred:", "failed")
    expect(exitSpy).toHaveBeenCalledWith(1)
  })
})
