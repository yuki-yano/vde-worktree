import { access, chmod, lstat, mkdtemp, mkdir, readFile, realpath, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { execa } from "execa"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { SelectPathWithFzfInput, SelectPathWithFzfResult } from "../integrations/fzf"
import { createCli } from "./index"

const runGit = async (cwd: string, args: readonly string[]): Promise<string> => {
  const result = await execa("git", [...args], { cwd, reject: false })
  if ((result.exitCode ?? 0) !== 0) {
    throw new Error(`git failed in ${cwd}: git ${args.join(" ")}\n${result.stderr}`)
  }
  return result.stdout
}

const setupRepo = async (): Promise<string> => {
  const repoRoot = await mkdtemp(join(tmpdir(), "vde-worktree-test-"))
  await runGit(repoRoot, ["init", "-b", "main"])
  await runGit(repoRoot, ["config", "user.name", "test-user"])
  await runGit(repoRoot, ["config", "user.email", "test@example.com"])
  await writeFile(join(repoRoot, "README.md"), "# test\n", "utf8")
  await runGit(repoRoot, ["add", "."])
  await runGit(repoRoot, ["commit", "-m", "initial"])
  return realpath(repoRoot)
}

const expectSingleStdoutLine = (stdout: readonly string[]): string => {
  expect(stdout.length).toBe(1)
  return stdout[0] as string
}

const writeExecutableHook = async ({
  repoRoot,
  hookName,
  body,
}: {
  readonly repoRoot: string
  readonly hookName: string
  readonly body: string
}): Promise<void> => {
  const hookPath = join(repoRoot, ".vde", "worktree", "hooks", hookName)
  await writeFile(hookPath, body, "utf8")
  await chmod(hookPath, 0o755)
}

describe("createCli", () => {
  const tempDirs = new Set<string>()
  const envBackup = new Map<string, string | undefined>()

  afterEach(async () => {
    for (const key of ["WT_WORKTREE_PATH"]) {
      if (envBackup.has(key)) {
        const value = envBackup.get(key)
        if (value === undefined) {
          delete process.env[key]
        } else {
          process.env[key] = value
        }
      }
    }
    envBackup.clear()

    await Promise.all(
      [...tempDirs].map(async (dir) => {
        await rm(dir, { recursive: true, force: true })
      }),
    )
    tempDirs.clear()
  })

  it("prints version", async () => {
    const stdout: string[] = []
    const stderr: string[] = []
    const cli = createCli({
      version: "1.2.3",
      stdout: (line) => stdout.push(line),
      stderr: (line) => stderr.push(line),
    })

    const exitCode = await cli.run(["--version"])

    expect(exitCode).toBe(0)
    expect(stdout).toEqual(["1.2.3"])
    expect(stderr).toEqual([])
  })

  it("prints rich general help with command index", async () => {
    const stdout: string[] = []
    const cli = createCli({
      version: "1.2.3",
      stdout: (line) => stdout.push(line),
    })

    const exitCode = await cli.run(["--help"])
    expect(exitCode).toBe(0)

    const text = expectSingleStdoutLine(stdout)
    expect(text).toContain("Usage:")
    expect(text).toContain("Commands:")
    expect(text).toContain("switch")
    expect(text).toContain("help <command>")
  })

  it("prints command-specific help via help subcommand", async () => {
    const stdout: string[] = []
    const cli = createCli({
      stdout: (line) => stdout.push(line),
    })

    const exitCode = await cli.run(["help", "del"])
    expect(exitCode).toBe(0)

    const text = expectSingleStdoutLine(stdout)
    expect(text).toContain("Command: del")
    expect(text).toContain("Usage:")
    expect(text).toContain("--force-unmerged")
    expect(text).toContain("--allow-unsafe")
  })

  it("prints command-specific help via --help on command", async () => {
    const stdout: string[] = []
    const cli = createCli({
      stdout: (line) => stdout.push(line),
    })

    const exitCode = await cli.run(["exec", "--help"])
    expect(exitCode).toBe(0)

    const text = expectSingleStdoutLine(stdout)
    expect(text).toContain("Command: exec")
    expect(text).toContain("vw exec <branch> -- <cmd...>")
  })

  it("init creates required directories and keeps exclude idempotent", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []

    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["init"])).toBe(0)

    const exclude = await readFile(join(repoRoot, ".git", "info", "exclude"), "utf8")
    const markerCount = exclude.split("# vde-worktree (managed)").length - 1
    expect(markerCount).toBe(1)

    await readFile(join(repoRoot, ".vde", "worktree", "hooks", "post-new"), "utf8")
    await readFile(join(repoRoot, ".vde", "worktree", "hooks", "post-switch"), "utf8")
    await access(join(repoRoot, ".vde", "worktree", "logs"))
    expect(stderr).toEqual([])
  })

  it("list --json resolves repoRoot from subdirectory", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const workingDir = join(repoRoot, "sub", "dir")
    await mkdir(workingDir, { recursive: true })

    const stdout: string[] = []
    const cli = createCli({
      cwd: workingDir,
      stdout: (line) => stdout.push(line),
    })

    const exitCode = await cli.run(["list", "--json"])
    expect(exitCode).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      schemaVersion: number
      command: string
      repoRoot: string | null
      worktrees: Array<{ branch: string | null; path: string }>
    }

    expect(payload.schemaVersion).toBe(1)
    expect(payload.command).toBe("list")
    expect(payload.repoRoot).toBe(repoRoot)
    expect(payload.worktrees.some((worktree) => worktree.branch === "main" && worktree.path === repoRoot)).toBe(true)
  })

  it("switch creates and then reuses worktree path", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const stderr: string[] = []

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
      stderr: (line) => stderr.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/foo"])).toBe(0)
    expect(await cli.run(["switch", "feature/foo"])).toBe(0)

    const encodedPath = join(repoRoot, ".worktree", "feature%2Ffoo")
    expect(stdout).toEqual([encodedPath, encodedPath])
    expect(stderr).toEqual([])

    const worktreeList = await runGit(repoRoot, ["worktree", "list", "--porcelain"])
    const branchMatches = worktreeList.match(/branch refs\/heads\/feature\/foo/g)
    expect(branchMatches?.length ?? 0).toBe(1)
  })

  it("path --json returns absolute worktree path", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/foo"])).toBe(0)
    stdout.length = 0

    expect(await cli.run(["path", "feature/foo", "--json"])).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as { status: string; path: string; branch: string }

    expect(payload.status).toBe("ok")
    expect(payload.branch).toBe("feature/foo")
    expect(payload.path).toBe(join(repoRoot, ".worktree", "feature%2Ffoo"))
  })

  it("status --json without branch shows current worktree", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const workingDir = join(repoRoot, "nested")
    await mkdir(workingDir, { recursive: true })

    const stdout: string[] = []
    const cli = createCli({
      cwd: workingDir,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["status", "--json"])).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      command: string
      worktree: { branch: string | null; path: string }
    }

    expect(payload.command).toBe("status")
    expect(payload.worktree.branch).toBe("main")
    expect(payload.worktree.path).toBe(repoRoot)
  })

  it("new without branch creates wip branch", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["new"])).toBe(0)
    expect(stdout.length).toBe(1)
    expect(stdout[0]).toMatch(/\.worktree\/wip-\d{6}$/)
  })

  it("cd selects from existing worktree paths via fzf", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []

    const selectPathWithFzf = vi.fn<(input: SelectPathWithFzfInput) => Promise<SelectPathWithFzfResult>>(async () => ({
      status: "selected" as const,
      path: join(repoRoot, ".worktree", "feature%2Ffoo"),
    }))

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
      selectPathWithFzf,
      isInteractive: () => true,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/foo"])).toBe(0)
    stdout.length = 0

    expect(await cli.run(["cd"])).toBe(0)
    expect(stdout).toEqual([join(repoRoot, ".worktree", "feature%2Ffoo")])
    expect(selectPathWithFzf).toHaveBeenCalledTimes(1)
    const firstCall = selectPathWithFzf.mock.calls[0]?.[0] ?? null
    expect(firstCall).not.toBeNull()
    const candidates = (firstCall as SelectPathWithFzfInput).candidates
    expect(candidates).toContain(repoRoot)
    expect(candidates).toContain(join(repoRoot, ".worktree", "feature%2Ffoo"))
  })

  it("fails with exit code 4 when --no-hooks is used without --allow-unsafe", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []

    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    const exitCode = await cli.run(["new", "feature/no-hooks", "--no-hooks"])

    expect(exitCode).toBe(4)
    expect(stderr.some((line) => line.includes("UNSAFE_FLAG_REQUIRED"))).toBe(true)
  })

  it("lock and unlock update lock state", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/lock"])).toBe(0)
    stdout.length = 0

    expect(await cli.run(["lock", "feature/lock", "--owner", "alice", "--reason", "in progress", "--json"])).toBe(0)
    const lockPayload = JSON.parse(expectSingleStdoutLine(stdout)) as { locked: { value: boolean; reason: string } }
    expect(lockPayload.locked.value).toBe(true)
    expect(lockPayload.locked.reason).toBe("in progress")

    stdout.length = 0
    expect(await cli.run(["unlock", "feature/lock", "--owner", "alice", "--json"])).toBe(0)
    const unlockPayload = JSON.parse(expectSingleStdoutLine(stdout)) as { locked: { value: boolean } }
    expect(unlockPayload.locked.value).toBe(false)
  })

  it("exec returns child failure as 21", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/exec"])).toBe(0)
    stdout.length = 0

    const exitCode = await cli.run(["exec", "feature/exec", "--", "node", "-e", "process.exit(2)"])
    expect(exitCode).toBe(21)

    expect(await cli.run(["exec", "feature/exec", "--json", "--", "node", "-e", "process.exit(2)"])).toBe(21)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      status: string
      code: string
      details: { childExitCode: number }
    }
    expect(payload.status).toBe("error")
    expect(payload.code).toBe("CHILD_PROCESS_FAILED")
    expect(payload.details.childExitCode).toBe(2)
  })

  it("invoke executes existing hook", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const marker = join(repoRoot, "hook-invoked.txt")
    const cli = createCli({ cwd: repoRoot })

    expect(await cli.run(["init"])).toBe(0)
    await writeExecutableHook({
      repoRoot,
      hookName: "post-switch",
      body: `#!/usr/bin/env bash
set -eu
echo invoked > "${marker}"
`,
    })

    expect(await cli.run(["invoke", "post-switch"])).toBe(0)
    const content = await readFile(marker, "utf8")
    expect(content.trim()).toBe("invoked")
  })

  it("copy and link helper commands place files into WT_WORKTREE_PATH", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    await writeFile(join(repoRoot, ".envrc"), "export FOO=bar\n", "utf8")
    const cli = createCli({ cwd: repoRoot })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/files"])).toBe(0)
    const targetPath = join(repoRoot, ".worktree", "feature%2Ffiles")

    envBackup.set("WT_WORKTREE_PATH", process.env.WT_WORKTREE_PATH)
    process.env.WT_WORKTREE_PATH = targetPath

    expect(await cli.run(["copy", ".envrc"])).toBe(0)
    const copied = await readFile(join(targetPath, ".envrc"), "utf8")
    expect(copied).toContain("FOO=bar")

    expect(await cli.run(["link", ".envrc"])).toBe(0)
    const stats = await lstat(join(targetPath, ".envrc"))
    expect(stats.isSymbolicLink()).toBe(true)
  })

  it("mv renames current worktree branch and path", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const rootCli = createCli({ cwd: repoRoot })
    expect(await rootCli.run(["init"])).toBe(0)
    expect(await rootCli.run(["switch", "feature/mv"])).toBe(0)

    const currentPath = join(repoRoot, ".worktree", "feature%2Fmv")
    const stdout: string[] = []
    const cli = createCli({
      cwd: currentPath,
      stdout: (line) => stdout.push(line),
    })
    expect(await cli.run(["mv", "feature/moved"])).toBe(0)

    const newPath = join(repoRoot, ".worktree", "feature%2Fmoved")
    expect(stdout).toEqual([newPath])
    const worktreeList = await runGit(repoRoot, ["worktree", "list", "--porcelain"])
    expect(worktreeList.includes(`worktree ${newPath}`)).toBe(true)
    expect(worktreeList.includes("branch refs/heads/feature/moved")).toBe(true)
  })

  it("del rejects unmerged by default and succeeds with force", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/del"])).toBe(0)

    expect(await cli.run(["del", "feature/del"])).toBe(4)
    expect(stderr.some((line) => line.includes("WORKTREE"))).toBe(true)

    expect(await cli.run(["del", "feature/del", "--force-unmerged", "--allow-unpushed", "--allow-unsafe"])).toBe(0)
    const list = await runGit(repoRoot, ["worktree", "list", "--porcelain"])
    expect(list.includes("feature/del")).toBe(false)
  })

  it("gone dry-run then apply removes merged candidates", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/gone"])).toBe(0)
    const featurePath = join(repoRoot, ".worktree", "feature%2Fgone")
    await writeFile(join(featurePath, "feature.txt"), "x\n", "utf8")
    await runGit(featurePath, ["add", "feature.txt"])
    await runGit(featurePath, ["commit", "-m", "feature commit"])
    await runGit(featurePath, ["branch", "--set-upstream-to", "main", "feature/gone"])
    await runGit(repoRoot, ["merge", "--no-ff", "feature/gone", "-m", "merge feature/gone"])

    stdout.length = 0
    expect(await cli.run(["gone", "--json"])).toBe(0)
    const dryPayload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      candidates: string[]
      dryRun: boolean
    }
    expect(dryPayload.dryRun).toBe(true)
    expect(dryPayload.candidates).toContain("feature/gone")

    stdout.length = 0
    expect(await cli.run(["gone", "--apply", "--json"])).toBe(0)
    const applyPayload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      deleted: string[]
      dryRun: boolean
    }
    expect(applyPayload.dryRun).toBe(false)
    expect(applyPayload.deleted).toContain("feature/gone")
  })

  it("get creates tracked local branch worktree from remote branch", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const remoteRoot = await mkdtemp(join(tmpdir(), "vde-worktree-remote-"))
    tempDirs.add(remoteRoot)
    await runGit(remoteRoot, ["init", "--bare"])
    await runGit(repoRoot, ["remote", "add", "origin", remoteRoot])
    await runGit(repoRoot, ["push", "-u", "origin", "main"])
    await runGit(repoRoot, ["checkout", "-b", "feature/get"])
    await writeFile(join(repoRoot, "get.txt"), "get\n", "utf8")
    await runGit(repoRoot, ["add", "get.txt"])
    await runGit(repoRoot, ["commit", "-m", "feature get"])
    await runGit(repoRoot, ["push", "-u", "origin", "feature/get"])
    await runGit(repoRoot, ["checkout", "main"])
    await runGit(repoRoot, ["branch", "-D", "feature/get"])

    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })
    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["get", "origin/feature/get"])).toBe(0)
    expect(stdout).toEqual([join(repoRoot, ".worktree", "feature%2Fget")])
  })

  it("extract moves current primary branch into .worktree", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    await runGit(repoRoot, ["checkout", "-b", "feature/extract"])
    await writeFile(join(repoRoot, "extract.txt"), "extract\n", "utf8")
    await runGit(repoRoot, ["add", "extract.txt"])
    await runGit(repoRoot, ["commit", "-m", "extract commit"])

    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })
    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["extract", "--current"])).toBe(0)
    expect(stdout).toEqual([join(repoRoot, ".worktree", "feature%2Fextract")])

    const head = await runGit(repoRoot, ["branch", "--show-current"])
    expect(head.trim()).toBe("main")
  })

  it("use requires explicit non-TTY allow flags and then checks out branch", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    await runGit(repoRoot, ["checkout", "-b", "feature/use"])
    await runGit(repoRoot, ["checkout", "main"])

    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["use", "feature/use"])).toBe(4)
    expect(stderr.some((line) => line.includes("UNSAFE_FLAG_REQUIRED"))).toBe(true)

    expect(await cli.run(["use", "feature/use", "--allow-agent", "--allow-unsafe"])).toBe(0)
    const head = await runGit(repoRoot, ["branch", "--show-current"])
    expect(head.trim()).toBe("feature/use")
  })
})
