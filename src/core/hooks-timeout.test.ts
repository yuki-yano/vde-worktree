import { chmod, mkdtemp, mkdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, describe, expect, it, vi } from "vitest"

vi.mock("execa", () => {
  return {
    execa: vi.fn(),
  }
})

import { execa } from "execa"
import { runPostHook, runPreHook } from "./hooks"

const tempDirs = new Set<string>()
const mockedExeca = vi.mocked(execa)

const createRepoRoot = async (): Promise<string> => {
  const repoRoot = await mkdtemp(join(tmpdir(), "vde-worktree-hooks-timeout-"))
  tempDirs.add(repoRoot)
  await mkdir(join(repoRoot, ".vde", "worktree", "hooks"), { recursive: true })
  return repoRoot
}

afterEach(async () => {
  mockedExeca.mockReset()
  await Promise.all(
    [...tempDirs].map(async (dir) => {
      await rm(dir, { recursive: true, force: true })
    }),
  )
  tempDirs.clear()
})

describe("runPreHook timeout handling", () => {
  it("maps timeout errors to HOOK_TIMEOUT", async () => {
    const repoRoot = await createRepoRoot()
    const hookPath = join(repoRoot, ".vde", "worktree", "hooks", "pre-switch")
    await writeFile(hookPath, "#!/usr/bin/env bash\nexit 0\n", "utf8")
    await chmod(hookPath, 0o755)

    mockedExeca.mockRejectedValueOnce(
      Object.assign(new Error("timeout"), {
        code: "ETIMEDOUT",
        stderr: "timeout",
      }),
    )

    await expect(
      runPreHook({
        name: "switch",
        context: {
          repoRoot,
          action: "switch",
          branch: "feature/hooks",
          enabled: true,
          stderr: () => undefined,
          timeoutMs: 10,
        },
      }),
    ).rejects.toMatchObject({
      code: "HOOK_TIMEOUT",
    })
  })

  it("swallows post-hook execution errors when strictPostHooks is disabled", async () => {
    const repoRoot = await createRepoRoot()
    const hookPath = join(repoRoot, ".vde", "worktree", "hooks", "post-switch")
    await writeFile(hookPath, "#!/usr/bin/env bash\nexit 0\n", "utf8")
    await chmod(hookPath, 0o755)
    const stderr: string[] = []

    mockedExeca.mockRejectedValueOnce(new Error("spawn failed"))

    await expect(
      runPostHook({
        name: "switch",
        context: {
          repoRoot,
          action: "switch",
          branch: "feature/hooks",
          enabled: true,
          strictPostHooks: false,
          stderr: (line) => stderr.push(line),
        },
      }),
    ).resolves.toBeUndefined()
    expect(stderr).toEqual(["Hook failed: post-switch"])
  })

  it("throws HOOK_FAILED for post-hook execution errors when strictPostHooks is enabled", async () => {
    const repoRoot = await createRepoRoot()
    const hookPath = join(repoRoot, ".vde", "worktree", "hooks", "post-switch")
    await writeFile(hookPath, "#!/usr/bin/env bash\nexit 0\n", "utf8")
    await chmod(hookPath, 0o755)

    mockedExeca.mockRejectedValueOnce(
      Object.assign(new Error("spawn failed"), {
        stderr: "permission denied",
      }),
    )

    await expect(
      runPostHook({
        name: "switch",
        context: {
          repoRoot,
          action: "switch",
          branch: "feature/hooks",
          enabled: true,
          strictPostHooks: true,
          stderr: () => undefined,
        },
      }),
    ).rejects.toMatchObject({
      code: "HOOK_FAILED",
      details: {
        hook: "post-switch",
        stderr: "permission denied",
      },
    })
  })
})
