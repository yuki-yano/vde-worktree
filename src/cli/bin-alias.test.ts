import { access, readFile } from "node:fs/promises"
import { join } from "node:path"
import { describe, expect, it } from "vitest"

describe("bin alias", () => {
  it("provides vw alias in package bin definitions", async () => {
    const packageJsonPath = join(process.cwd(), "package.json")
    const packageJson = JSON.parse(await readFile(packageJsonPath, "utf8")) as {
      bin?: Record<string, string>
    }

    expect(packageJson.bin?.["vde-worktree"]).toBe("./bin/vde-worktree")
    expect(packageJson.bin?.vw).toBe("./bin/vw")
    await access(join(process.cwd(), "bin", "vw"))
  })
})
