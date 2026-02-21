import { describe, expect, it } from "vitest"
import { createEarlyRepoCommandHandlers, createMiscCommandHandlers, dispatchCommandHandler } from "./handler-groups"

describe("handler-groups", () => {
  it("dispatches matching command handler", async () => {
    const handlers = createEarlyRepoCommandHandlers({
      initHandler: async () => 10,
      listHandler: async () => 20,
      statusHandler: async () => 30,
      pathHandler: async () => 40,
    })

    const exitCode = await dispatchCommandHandler({
      command: "status",
      handlers,
    })

    expect(exitCode).toBe(30)
  })

  it("returns undefined when command is not handled", async () => {
    const handlers = createEarlyRepoCommandHandlers({
      initHandler: async () => 10,
      listHandler: async () => 20,
      statusHandler: async () => 30,
      pathHandler: async () => 40,
    })

    const exitCode = await dispatchCommandHandler({
      command: "unknown",
      handlers,
    })

    expect(exitCode).toBeUndefined()
  })

  it("creates misc handlers in expected command order", () => {
    const handlers = createMiscCommandHandlers({
      execHandler: async () => 1,
      invokeHandler: async () => 1,
      copyHandler: async () => 1,
      linkHandler: async () => 1,
      lockHandler: async () => 1,
      unlockHandler: async () => 1,
      cdHandler: async () => 1,
    })

    expect([...handlers.keys()]).toEqual(["exec", "invoke", "copy", "link", "lock", "unlock", "cd"])
  })
})
