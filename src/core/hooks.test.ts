import { chmod, mkdir, readdir, readFile, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { afterEach, describe, expect, it } from "vitest"
import { cleanupRepoFixtures, createRepoFixture } from "../test-utils/repo-fixture"
import { invokeHook, runPostHook, runPreHook, type HookExecutionContext } from "./hooks"

const createRepoRoot = async (): Promise<string> => {
  return createRepoFixture({
    prefix: "vde-worktree-hooks-",
    setup: async (repoRoot) => {
      await mkdir(join(repoRoot, ".vde", "worktree", "hooks"), { recursive: true })
    },
  })
}

const writeHook = async ({
  repoRoot,
  name,
  body,
  executable = true,
}: {
  readonly repoRoot: string
  readonly name: string
  readonly body: string
  readonly executable?: boolean
}): Promise<void> => {
  const path = join(repoRoot, ".vde", "worktree", "hooks", name)
  await writeFile(path, body, "utf8")
  await chmod(path, executable ? 0o755 : 0o644)
}

const buildContext = ({
  repoRoot,
  stderr,
  strictPostHooks,
  timeoutMs,
  worktreePath,
  extraEnv,
}: {
  readonly repoRoot: string
  readonly stderr: (line: string) => void
  readonly strictPostHooks?: boolean
  readonly timeoutMs?: number
  readonly worktreePath?: string
  readonly extraEnv?: Record<string, string>
}): HookExecutionContext => {
  return {
    repoRoot,
    action: "switch",
    branch: "feature/hooks",
    enabled: true,
    stderr,
    strictPostHooks,
    timeoutMs,
    worktreePath,
    extraEnv,
  }
}

afterEach(cleanupRepoFixtures)

describe("hooks", () => {
  it("returns immediately when hooks are disabled", async () => {
    const repoRoot = await createRepoRoot()
    const stderr: string[] = []

    await expect(
      runPreHook({
        name: "switch",
        context: {
          repoRoot,
          action: "switch",
          branch: "feature/hooks",
          enabled: false,
          stderr: (line) => stderr.push(line),
        },
      }),
    ).resolves.toBeUndefined()
    expect(stderr).toEqual([])
  })

  it("ignores missing pre hook when optional", async () => {
    const repoRoot = await createRepoRoot()
    const stderr: string[] = []
    const context = buildContext({
      repoRoot,
      stderr: (line) => stderr.push(line),
    })

    await expect(
      runPreHook({
        name: "switch",
        context,
      }),
    ).resolves.toBeUndefined()
    expect(stderr).toEqual([])
  })

  it("fails when invoke target hook does not exist", async () => {
    const repoRoot = await createRepoRoot()
    const context = buildContext({
      repoRoot,
      stderr: () => undefined,
    })

    await expect(
      invokeHook({
        hookName: "post-missing",
        args: [],
        context,
      }),
    ).rejects.toMatchObject({
      code: "HOOK_NOT_FOUND",
    })
  })

  it("fails when hook exists but is not executable", async () => {
    const repoRoot = await createRepoRoot()
    await writeHook({
      repoRoot,
      name: "pre-switch",
      body: "#!/usr/bin/env bash\nexit 0\n",
      executable: false,
    })
    const context = buildContext({
      repoRoot,
      stderr: () => undefined,
    })

    await expect(
      runPreHook({
        name: "switch",
        context,
      }),
    ).rejects.toMatchObject({
      code: "HOOK_NOT_EXECUTABLE",
    })
  })

  it("logs and continues for post hook failure when strict mode is disabled", async () => {
    const repoRoot = await createRepoRoot()
    await writeHook({
      repoRoot,
      name: "post-switch",
      body: "#!/usr/bin/env bash\nexit 3\n",
    })

    const stderr: string[] = []
    const context = buildContext({
      repoRoot,
      stderr: (line) => stderr.push(line),
    })

    await expect(
      runPostHook({
        name: "switch",
        context,
      }),
    ).resolves.toBeUndefined()

    expect(stderr).toContain("Hook failed: post-switch (exitCode=3)")
    const logsDir = join(repoRoot, ".vde", "worktree", "logs")
    const files = await readdir(logsDir)
    expect(files.length).toBe(1)
    const log = await readFile(join(logsDir, files[0] as string), "utf8")
    expect(log).toContain("hook=post-switch")
    expect(log).toContain("phase=post")
    expect(log).toContain("exitCode=3")
  })

  it("throws on post hook failure when strict mode is enabled", async () => {
    const repoRoot = await createRepoRoot()
    await writeHook({
      repoRoot,
      name: "post-switch",
      body: "#!/usr/bin/env bash\nexit 2\n",
    })
    const context = buildContext({
      repoRoot,
      stderr: () => undefined,
      strictPostHooks: true,
    })

    await expect(
      runPostHook({
        name: "switch",
        context,
      }),
    ).rejects.toMatchObject({
      code: "HOOK_FAILED",
    })
  })

  it("passes context and extra env to hook process", async () => {
    const repoRoot = await createRepoRoot()
    await writeHook({
      repoRoot,
      name: "post-switch",
      body: '#!/usr/bin/env bash\nset -eu\necho "${WT_ACTION}:${WT_BRANCH}:${EXTRA_FLAG}" > hook-output.txt\n',
    })

    const context = buildContext({
      repoRoot,
      worktreePath: repoRoot,
      stderr: () => undefined,
      extraEnv: {
        EXTRA_FLAG: "ok",
      },
    })

    await runPostHook({
      name: "switch",
      context,
    })

    const output = await readFile(join(repoRoot, "hook-output.txt"), "utf8")
    expect(output.trim()).toBe("switch:feature/hooks:ok")
  })
})
