import { access, chmod, lstat, mkdtemp, mkdir, readFile, realpath, rm, writeFile } from "node:fs/promises"
import { homedir, tmpdir } from "node:os"
import { join } from "node:path"
import { execa } from "execa"
import stringWidth from "string-width"
import { afterEach, describe, expect, it, vi } from "vitest"
import { branchToWorktreeId } from "../core/paths"
import { FzfDependencyError } from "../integrations/fzf"
import type { SelectPathWithFzfInput, SelectPathWithFzfResult } from "../integrations/fzf"
import { createCli } from "./index"

const runGit = async (cwd: string, args: readonly string[]): Promise<string> => {
  const result = await execa("git", [...args], { cwd, reject: false })
  if ((result.exitCode ?? 0) !== 0) {
    throw new Error(`git failed in ${cwd}: git ${args.join(" ")}\n${result.stderr}`)
  }
  return result.stdout
}

const setupRepo = async ({ baseDir }: { readonly baseDir?: string } = {}): Promise<string> => {
  const repoRoot = await mkdtemp(join(baseDir ?? tmpdir(), "vde-worktree-test-"))
  await runGit(repoRoot, ["init", "-b", "main"])
  await runGit(repoRoot, ["config", "user.name", "test-user"])
  await runGit(repoRoot, ["config", "user.email", "test@example.com"])
  await writeFile(join(repoRoot, "README.md"), "# test\n", "utf8")
  await runGit(repoRoot, ["add", "."])
  await runGit(repoRoot, ["commit", "-m", "initial"])
  if (baseDir === undefined) {
    return realpath(repoRoot)
  }
  return repoRoot
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
    for (const key of ["WT_WORKTREE_PATH", "PATH"]) {
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
    expect(text).toContain("completion")
    expect(text).toContain("help <command>")
    expect(text).toContain("--no-gh")
    expect(text).toContain("--full-path")
  })

  it("prints zsh completion script outside git repository", async () => {
    const cwd = await mkdtemp(join(tmpdir(), "vde-worktree-completion-"))
    tempDirs.add(cwd)
    const stdout: string[] = []
    const stderr: string[] = []
    const cli = createCli({
      cwd,
      stdout: (line) => stdout.push(line),
      stderr: (line) => stderr.push(line),
    })

    const exitCode = await cli.run(["completion", "zsh"])
    expect(exitCode).toBe(0)

    const text = expectSingleStdoutLine(stdout)
    expect(text).toContain("#compdef vw vde-worktree")
    expect(text).toContain("completion")
    expect(text).toContain("_vw_complete_use_branches()")
    expect(text).toContain("_vw_complete_managed_worktree_names()")
    expect(text).not.toContain("_vw_complete_switch_branches()")
    expect(text).not.toContain("_vw_local_branches_raw()")
    expect(text).toContain(`switch)
          _arguments \\
            "1:branch:_vw_complete_worktree_branches_with_meta"`)
    expect(text).toContain(`mv)
          _arguments \\
            "1:new-branch:"`)
    expect(text).toContain(`use)
          _arguments \\
            "1:branch:_vw_complete_use_branches" \\`)
    expect(text).toContain(`absorb)
          _arguments \\
            "1:branch:_vw_complete_worktree_branches_with_meta" \\`)
    expect(text).toContain("--from[Source managed worktree name]:worktree-name:_vw_complete_managed_worktree_names")
    expect(text).toContain(`unabsorb)
          _arguments \\
            "1:branch:_vw_complete_worktree_branches_with_meta" \\`)
    expect(text).toContain("--to[Target managed worktree name]:worktree-name:_vw_complete_managed_worktree_names")
    expect(text).toContain("--allow-shared[Allow checkout when branch is attached by another worktree]")
    expect(text).toContain("list --json 2>/dev/null | command node -e")
    expect(text).toContain("payload?.baseBranch")
    expect(text).toContain("payload?.managedWorktreeRoot")
    expect(stderr).toEqual([])
  })

  it("installs fish completion script to custom path", async () => {
    const cwd = await mkdtemp(join(tmpdir(), "vde-worktree-completion-install-"))
    tempDirs.add(cwd)
    const targetPath = join(cwd, "fish", "completions", "vw.fish")
    const stdout: string[] = []
    const stderr: string[] = []
    const cli = createCli({
      cwd,
      stdout: (line) => stdout.push(line),
      stderr: (line) => stderr.push(line),
    })

    const exitCode = await cli.run(["completion", "fish", "--install", "--path", targetPath])
    expect(exitCode).toBe(0)

    const installed = await readFile(targetPath, "utf8")
    expect(installed).toContain("complete -c $__vw_bin")
    expect(installed).toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from switch" -a "(__vw_worktree_candidates_with_meta)"',
    )
    expect(installed).not.toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from mv" -a "(__vw_local_branches)"',
    )
    expect(installed).toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from absorb" -a "(__vw_worktree_candidates_with_meta)"',
    )
    expect(installed).toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from unabsorb" -a "(__vw_worktree_candidates_with_meta)"',
    )
    expect(installed).toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from absorb" -l from -r -a "(__vw_managed_worktree_names_with_meta)" -d "Source managed worktree name"',
    )
    expect(installed).toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from absorb" -l keep-stash -d "Keep stash entry after absorb"',
    )
    expect(installed).toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from unabsorb" -l to -r -a "(__vw_managed_worktree_names_with_meta)" -d "Target managed worktree name"',
    )
    expect(installed).toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from use" -a "(__vw_use_candidates_with_meta)"',
    )
    expect(installed).toContain(
      'complete -c $__vw_bin -n "__fish_seen_subcommand_from use" -l allow-shared -d "Allow checkout when branch is attached by another worktree"',
    )
    expect(installed).toContain("list --json 2>/dev/null | command node -e")
    expect(installed).toContain("payload?.baseBranch")
    expect(installed).toContain("payload?.managedWorktreeRoot")
    expect(expectSingleStdoutLine(stdout)).toContain(targetPath)
    expect(stderr).toEqual([])
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

    const featurePath = join(repoRoot, ".worktree", "feature", "foo")
    expect(stdout).toEqual([featurePath, featurePath])
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
    expect(payload.path).toBe(join(repoRoot, ".worktree", "feature", "foo"))
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

    const selectPathWithFzf = vi.fn<(input: SelectPathWithFzfInput) => Promise<SelectPathWithFzfResult>>(
      async (input) => {
        const selected =
          input.candidates.find((candidate) =>
            candidate.includes(`\t${join(repoRoot, ".worktree", "feature", "foo")}\t`),
          ) ?? join(repoRoot, ".worktree", "feature", "foo")
        return {
          status: "selected" as const,
          path: selected,
        }
      },
    )

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
    expect(stdout).toEqual([join(repoRoot, ".worktree", "feature", "foo")])
    expect(selectPathWithFzf).toHaveBeenCalledTimes(1)
    const firstCall = selectPathWithFzf.mock.calls[0]?.[0] ?? null
    expect(firstCall).not.toBeNull()
    const candidates = (firstCall as SelectPathWithFzfInput).candidates
    const candidateRows = candidates.map((candidate) => candidate.split("\t"))
    expect(candidateRows.some((parts) => parts[1] === repoRoot)).toBe(true)
    expect(candidateRows.some((parts) => parts[1] === join(repoRoot, ".worktree", "feature", "foo"))).toBe(true)
    const mainRow = candidateRows.find((parts) => parts[1] === repoRoot)
    const selectedRow = candidateRows.find((parts) => parts[1] === join(repoRoot, ".worktree", "feature", "foo"))
    const toPlain = (value: string): string => value.replace(/\u001b\[[0-9;]*m/g, "")
    const mainFirstColumn = toPlain(mainRow?.[0] ?? "")
    const selectedFirstColumn = toPlain(selectedRow?.[0] ?? "")
    const mainStateColumnIndex = mainFirstColumn.search(/\b(CLEAN|DIRTY)\b/)
    const selectedStateColumnIndex = selectedFirstColumn.search(/\b(CLEAN|DIRTY)\b/)
    const mainLockColumnIndex = mainFirstColumn.search(/\b(LOCK|OPEN)\b/)
    const selectedLockColumnIndex = selectedFirstColumn.search(/\b(LOCK|OPEN)\b/)
    expect(mainStateColumnIndex).toBeGreaterThanOrEqual(0)
    expect(selectedStateColumnIndex).toBe(mainStateColumnIndex)
    expect(mainLockColumnIndex).toBeGreaterThanOrEqual(0)
    expect(selectedLockColumnIndex).toBe(mainLockColumnIndex)
    expect(stringWidth(selectedFirstColumn)).toBeGreaterThan(stringWidth("* feature/foo"))
    expect(selectedRow?.[0]).toContain("feature/foo")
    expect(selectedRow?.[0]).toMatch(/(CLEAN|DIRTY)/)
    expect(selectedRow?.[0]).toMatch(/(MERGED|UNMERGED|BASE|UNKNOWN)/)
    expect(selectedRow?.[0]).toContain("|")
    expect(selectedRow?.[2]).toMatch(/STATUS.*\\n/)
    expect(selectedRow?.[2]).toMatch(/Dirty.*CLEAN/)
    expect(selectedRow?.[2]).toMatch(/\\033\[/)
  })

  it("cd allows command-substitution style execution when stderr is TTY", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const mutableStderr = process.stderr as NodeJS.WriteStream & { isTTY?: boolean }
    const hadOwnIsTTY = Object.prototype.hasOwnProperty.call(mutableStderr, "isTTY")
    const previousIsTTY = mutableStderr.isTTY

    Object.defineProperty(mutableStderr, "isTTY", {
      value: true,
      configurable: true,
      writable: true,
    })

    try {
      const selectPathWithFzf = vi.fn<(input: SelectPathWithFzfInput) => Promise<SelectPathWithFzfResult>>(
        async (input) => {
          expect(input.isInteractive?.()).toBe(true)
          expect(input.candidates.some((candidate) => candidate.includes("\u001b["))).toBe(true)
          return {
            status: "selected" as const,
            path: join(repoRoot, ".worktree", "feature", "foo"),
          }
        },
      )

      const cli = createCli({
        cwd: repoRoot,
        stdout: (line) => stdout.push(line),
        selectPathWithFzf,
        isInteractive: () => false,
      })

      expect(await cli.run(["init"])).toBe(0)
      expect(await cli.run(["switch", "feature/foo"])).toBe(0)
      stdout.length = 0

      expect(await cli.run(["cd"])).toBe(0)
      expect(stdout).toEqual([join(repoRoot, ".worktree", "feature", "foo")])
    } finally {
      if (hadOwnIsTTY) {
        Object.defineProperty(mutableStderr, "isTTY", {
          value: previousIsTTY,
          configurable: true,
          writable: true,
        })
      } else {
        Reflect.deleteProperty(mutableStderr as unknown as Record<string, unknown>, "isTTY")
      }
    }
  })

  it("cd shows home-relative path in picker but returns absolute path", async () => {
    const repoRoot = await setupRepo({ baseDir: homedir() })
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const selectedPath = join(repoRoot, ".worktree", "feature", "home")

    const selectPathWithFzf = vi.fn<(input: SelectPathWithFzfInput) => Promise<SelectPathWithFzfResult>>(
      async (input) => {
        const selected = input.candidates.find((candidate) => candidate.includes(`\t${selectedPath}\t`)) ?? selectedPath
        const previewEncoded = selected.split("\t")[2] ?? ""
        const decodedPreview = previewEncoded.replace(/\\033/g, "\u001b")
        const plainPreview = decodedPreview.replace(/\u001b\[[0-9;]*m/g, "")
        expect(plainPreview).toContain("Path   : ~/")
        return {
          status: "selected" as const,
          path: selected,
        }
      },
    )

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
      selectPathWithFzf,
      isInteractive: () => true,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/home"])).toBe(0)
    stdout.length = 0

    expect(await cli.run(["cd"])).toBe(0)
    expect(stdout).toEqual([selectedPath])
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
    const targetPath = join(repoRoot, ".worktree", "feature", "files")

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

    const currentPath = join(repoRoot, ".worktree", "feature", "mv")
    const stdout: string[] = []
    const cli = createCli({
      cwd: currentPath,
      stdout: (line) => stdout.push(line),
    })
    expect(await cli.run(["mv", "feature/moved"])).toBe(0)

    const newPath = join(repoRoot, ".worktree", "feature", "moved")
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

  it("gone dry-run then apply removes overall-merged candidates without upstream tracking", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/gone"])).toBe(0)
    const featurePath = join(repoRoot, ".worktree", "feature", "gone")
    await writeFile(join(featurePath, "feature.txt"), "x\n", "utf8")
    await runGit(featurePath, ["add", "feature.txt"])
    await runGit(featurePath, ["commit", "-m", "feature commit"])
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

  it("gone excludes unmanaged worktrees even when merged", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const managedRoot = await mkdtemp(join(tmpdir(), "vde-worktree-managed-gone-"))
    const unmanagedRoot = await mkdtemp(join(tmpdir(), "vde-worktree-unmanaged-gone-"))
    tempDirs.add(managedRoot)
    tempDirs.add(unmanagedRoot)
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    await writeFile(
      join(repoRoot, ".vde", "worktree", "config.yml"),
      ["paths:", `  worktreeRoot: ${managedRoot}`].join("\n"),
      "utf8",
    )

    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    await runGit(repoRoot, ["branch", "feature/gone-unmanaged", "main"])
    await runGit(repoRoot, ["worktree", "add", unmanagedRoot, "feature/gone-unmanaged"])

    stdout.length = 0
    expect(await cli.run(["gone", "--json"])).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      candidates: string[]
      dryRun: boolean
    }
    expect(payload.dryRun).toBe(true)
    expect(payload.candidates).not.toContain("feature/gone-unmanaged")
  })

  it("adopt dry-run then apply moves unmanaged worktrees into managed root", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const unmanagedRootRaw = await mkdtemp(join(tmpdir(), "vde-worktree-unmanaged-adopt-"))
    tempDirs.add(unmanagedRootRaw)
    const unmanagedRoot = await realpath(unmanagedRootRaw)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    await runGit(repoRoot, ["branch", "feature/adopt", "main"])
    const unmanagedPath = join(unmanagedRoot, "feature-adopt")
    await runGit(repoRoot, ["worktree", "add", unmanagedPath, "feature/adopt"])

    stdout.length = 0
    expect(await cli.run(["adopt", "--json"])).toBe(0)
    const dryPayload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      dryRun: boolean
      candidates: Array<{ branch: string; fromPath: string; toPath: string }>
      moved: Array<{ branch: string; fromPath: string; toPath: string }>
      failed: Array<{ branch: string; fromPath: string; toPath: string; code: string; message: string }>
    }
    expect(dryPayload.dryRun).toBe(true)
    expect(dryPayload.moved).toEqual([])
    expect(dryPayload.failed).toEqual([])
    expect(dryPayload.candidates).toEqual([
      {
        branch: "feature/adopt",
        fromPath: unmanagedPath,
        toPath: join(repoRoot, ".worktree", "feature", "adopt"),
      },
    ])

    const listBeforeApply = await runGit(repoRoot, ["worktree", "list", "--porcelain"])
    expect(listBeforeApply.includes(`worktree ${unmanagedPath}`)).toBe(true)

    stdout.length = 0
    expect(await cli.run(["adopt", "--apply", "--json"])).toBe(0)
    const applyPayload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      dryRun: boolean
      moved: Array<{ branch: string; fromPath: string; toPath: string }>
      failed: Array<{ branch: string; fromPath: string; toPath: string; code: string; message: string }>
    }
    expect(applyPayload.dryRun).toBe(false)
    expect(applyPayload.failed).toEqual([])
    expect(applyPayload.moved).toEqual([
      {
        branch: "feature/adopt",
        fromPath: unmanagedPath,
        toPath: join(repoRoot, ".worktree", "feature", "adopt"),
      },
    ])

    const listAfterApply = await runGit(repoRoot, ["worktree", "list", "--porcelain"])
    expect(listAfterApply.includes(`worktree ${join(repoRoot, ".worktree", "feature", "adopt")}`)).toBe(true)
    expect(listAfterApply.includes(`worktree ${unmanagedPath}`)).toBe(false)
  })

  it("adopt dry-run reports skipped reasons for detached, locked, and target conflicts", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const unmanagedRootRaw = await mkdtemp(join(tmpdir(), "vde-worktree-unmanaged-adopt-skip-"))
    tempDirs.add(unmanagedRootRaw)
    const unmanagedRoot = await realpath(unmanagedRootRaw)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)

    await runGit(repoRoot, ["branch", "feature/adopt-locked", "main"])
    const lockedPath = join(unmanagedRoot, "locked")
    await runGit(repoRoot, ["worktree", "add", lockedPath, "feature/adopt-locked"])
    expect(await cli.run(["lock", "feature/adopt-locked", "--owner", "tester", "--reason", "protect"])).toBe(0)

    const detachedPath = join(unmanagedRoot, "detached")
    await runGit(repoRoot, ["worktree", "add", "--detach", detachedPath, "main"])

    await runGit(repoRoot, ["branch", "feature/adopt-conflict", "main"])
    const conflictPath = join(unmanagedRoot, "conflict")
    await runGit(repoRoot, ["worktree", "add", conflictPath, "feature/adopt-conflict"])
    const conflictTargetPath = join(repoRoot, ".worktree", "feature", "adopt-conflict")
    await mkdir(conflictTargetPath, { recursive: true })
    await writeFile(join(conflictTargetPath, "already.txt"), "x\n", "utf8")

    stdout.length = 0
    expect(await cli.run(["adopt", "--json"])).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      dryRun: boolean
      candidates: Array<{ branch: string; fromPath: string; toPath: string }>
      skipped: Array<{ branch: string | null; fromPath: string; toPath: string | null; reason: string }>
    }
    expect(payload.dryRun).toBe(true)
    expect(payload.candidates).toEqual([])

    const lockedSkip = payload.skipped.find((entry) => entry.branch === "feature/adopt-locked")
    expect(lockedSkip).toBeDefined()
    expect(lockedSkip?.reason).toBe("locked")
    expect(lockedSkip?.fromPath).toBe(lockedPath)

    const detachedSkip = payload.skipped.find((entry) => entry.fromPath === detachedPath)
    expect(detachedSkip).toBeDefined()
    expect(detachedSkip?.branch).toBeNull()
    expect(detachedSkip?.reason).toBe("detached")

    const conflictSkip = payload.skipped.find((entry) => entry.branch === "feature/adopt-conflict")
    expect(conflictSkip).toBeDefined()
    expect(conflictSkip?.reason).toBe("target_exists")
    expect(conflictSkip?.fromPath).toBe(conflictPath)
    expect(conflictSkip?.toPath).toBe(conflictTargetPath)
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
    expect(stdout).toEqual([join(repoRoot, ".worktree", "feature", "get")])
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
    expect(stdout).toEqual([join(repoRoot, ".worktree", "feature", "extract")])

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

  it("use requires --allow-shared when target branch is attached by another worktree", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: () => {},
      stderr: (line) => stderr.push(line),
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/use-shared"])).toBe(0)
    expect(await cli.run(["use", "feature/use-shared", "--allow-agent", "--allow-unsafe"])).toBe(4)
    expect(stderr.some((line) => line.includes("BRANCH_IN_USE"))).toBe(true)
    expect(stderr.some((line) => line.includes("--allow-shared"))).toBe(true)
    expect(stderr.some((line) => line.includes("unsafe"))).toBe(true)
    expect(stderr.some((line) => line.includes("To continue (unsafe), re-run with:"))).toBe(true)
    expect(stderr.some((line) => line.includes("\n  vw use feature/use-shared --allow-shared\n"))).toBe(true)

    stderr.length = 0
    expect(await cli.run(["use", "feature/use-shared", "--allow-shared", "--allow-agent", "--allow-unsafe"])).toBe(0)
    expect(stderr.some((line) => line.includes("warning:"))).toBe(true)
    expect(stderr.some((line) => line.includes("--allow-shared"))).toBe(true)
    expect(stderr.some((line) => line.includes("unsafe"))).toBe(true)
    expect(stderr.some((line) => line.includes("\n  branch: feature/use-shared\n"))).toBe(true)

    const head = await runGit(repoRoot, ["branch", "--show-current"])
    expect(head.trim()).toBe("feature/use-shared")
  })

  it("absorb applies non-primary worktree changes into primary", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const cli = createCli({
      cwd: repoRoot,
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/absorb"])).toBe(0)
    const sourcePath = join(repoRoot, ".worktree", "feature", "absorb")
    await writeFile(join(sourcePath, "README.md"), "# absorb\n", "utf8")
    await writeFile(join(sourcePath, "absorb.txt"), "absorbed\n", "utf8")

    expect(await cli.run(["absorb", "feature/absorb", "--allow-agent", "--allow-unsafe"])).toBe(0)
    const head = await runGit(repoRoot, ["branch", "--show-current"])
    expect(head.trim()).toBe("feature/absorb")
    expect(await readFile(join(repoRoot, "README.md"), "utf8")).toBe("# absorb\n")
    expect(await readFile(join(repoRoot, "absorb.txt"), "utf8")).toBe("absorbed\n")
  })

  it("absorb supports --from and --keep-stash", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/absorb-keep"])).toBe(0)
    stdout.length = 0
    const sourcePath = join(repoRoot, ".worktree", "feature", "absorb-keep")
    const sourceName = "feature/absorb-keep"
    await writeFile(join(sourcePath, "keep.txt"), "keep\n", "utf8")

    expect(
      await cli.run([
        "absorb",
        "feature/absorb-keep",
        "--from",
        sourceName,
        "--keep-stash",
        "--allow-agent",
        "--allow-unsafe",
        "--json",
      ]),
    ).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      branch: string
      sourcePath: string
      stashed: boolean
      stashRef: string | null
    }
    expect(payload.branch).toBe("feature/absorb-keep")
    expect(payload.sourcePath).toBe(sourcePath)
    expect(payload.stashed).toBe(true)
    expect(payload.stashRef).not.toBeNull()

    const stashMessage = await runGit(repoRoot, ["stash", "list", "--max-count=1", "--format=%gs"])
    expect(stashMessage.trim()).toContain("vde-worktree absorb feature/absorb-keep")
  })

  it("absorb applies intended stash even when pre-hook creates another stash", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const cli = createCli({
      cwd: repoRoot,
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/absorb-stash-stable"])).toBe(0)
    const sourcePath = join(repoRoot, ".worktree", "feature", "absorb-stash-stable")
    await writeFile(join(sourcePath, "stable.txt"), "from-source\n", "utf8")

    await writeExecutableHook({
      repoRoot,
      hookName: "pre-absorb",
      body: `#!/usr/bin/env bash
set -eu
echo hook-stash > "$WT_REPO_ROOT/.hook-pre-absorb.tmp"
git -C "$WT_REPO_ROOT" stash push -u -m "hook-pre-absorb" >/dev/null
`,
    })

    expect(await cli.run(["absorb", "feature/absorb-stash-stable", "--allow-agent", "--allow-unsafe"])).toBe(0)
    expect(await readFile(join(repoRoot, "stable.txt"), "utf8")).toBe("from-source\n")
  })

  it("absorb rejects --from when managed worktree name does not resolve", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/absorb-invalid"])).toBe(0)
    const sourcePath = join(repoRoot, ".worktree", "feature", "absorb-invalid")
    await writeFile(join(sourcePath, "invalid.txt"), "invalid\n", "utf8")

    expect(
      await cli.run([
        "absorb",
        "feature/absorb-invalid",
        "--from",
        ".worktree/feature/absorb-invalid",
        "--allow-agent",
        "--allow-unsafe",
      ]),
    ).toBe(4)
    expect(stderr.some((line) => line.includes("source worktree not found"))).toBe(true)
  })

  it("absorb auto-restores source changes when pre-hook fails", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const cli = createCli({
      cwd: repoRoot,
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/absorb-hook-fail"])).toBe(0)
    const sourcePath = join(repoRoot, ".worktree", "feature", "absorb-hook-fail")
    await writeFile(join(sourcePath, "restore-source.txt"), "restore\n", "utf8")

    await writeExecutableHook({
      repoRoot,
      hookName: "pre-absorb",
      body: `#!/usr/bin/env bash
set -eu
exit 1
`,
    })

    expect(await cli.run(["absorb", "feature/absorb-hook-fail", "--allow-agent", "--allow-unsafe"])).toBe(10)
    expect(await readFile(join(sourcePath, "restore-source.txt"), "utf8")).toBe("restore\n")
    const sourceStatus = await runGit(sourcePath, ["status", "--porcelain"])
    expect(sourceStatus).toContain("restore-source.txt")
  })

  it("unabsorb applies primary changes into non-primary worktree", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const cli = createCli({
      cwd: repoRoot,
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/unabsorb"])).toBe(0)
    expect(await cli.run(["use", "feature/unabsorb", "--allow-shared", "--allow-agent", "--allow-unsafe"])).toBe(0)

    await writeFile(join(repoRoot, "unabsorb.txt"), "unabsorbed\n", "utf8")
    expect(await cli.run(["unabsorb", "feature/unabsorb", "--allow-agent", "--allow-unsafe"])).toBe(0)

    const targetPath = join(repoRoot, ".worktree", "feature", "unabsorb")
    expect(await readFile(join(targetPath, "unabsorb.txt"), "utf8")).toBe("unabsorbed\n")
    const primaryStatus = await runGit(repoRoot, ["status", "--porcelain"])
    expect(primaryStatus.trim()).toBe("")
  })

  it("unabsorb supports --to and --keep-stash", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/unabsorb-keep"])).toBe(0)
    expect(await cli.run(["use", "feature/unabsorb-keep", "--allow-shared", "--allow-agent", "--allow-unsafe"])).toBe(0)

    await writeFile(join(repoRoot, "unabsorb-keep.txt"), "keep\n", "utf8")
    stdout.length = 0

    expect(
      await cli.run([
        "unabsorb",
        "feature/unabsorb-keep",
        "--to",
        "feature/unabsorb-keep",
        "--keep-stash",
        "--allow-agent",
        "--allow-unsafe",
        "--json",
      ]),
    ).toBe(0)

    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      branch: string
      path: string
      stashed: boolean
      stashRef: string | null
    }
    expect(payload.branch).toBe("feature/unabsorb-keep")
    expect(payload.path).toBe(join(repoRoot, ".worktree", "feature", "unabsorb-keep"))
    expect(payload.stashed).toBe(true)
    expect(payload.stashRef).not.toBeNull()

    const stashMessage = await runGit(repoRoot, ["stash", "list", "--max-count=1", "--format=%gs"])
    expect(stashMessage.trim()).toContain("vde-worktree unabsorb feature/unabsorb-keep")
  })

  it("unabsorb applies intended stash even when pre-hook creates another stash", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const cli = createCli({
      cwd: repoRoot,
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/unabsorb-stash-stable"])).toBe(0)
    expect(
      await cli.run(["use", "feature/unabsorb-stash-stable", "--allow-shared", "--allow-agent", "--allow-unsafe"]),
    ).toBe(0)
    await writeFile(join(repoRoot, "stable-unabsorb.txt"), "from-primary\n", "utf8")

    await writeExecutableHook({
      repoRoot,
      hookName: "pre-unabsorb",
      body: `#!/usr/bin/env bash
set -eu
echo hook-stash > "$WT_REPO_ROOT/.hook-pre-unabsorb.tmp"
git -C "$WT_REPO_ROOT" stash push -u -m "hook-pre-unabsorb" >/dev/null
`,
    })

    expect(await cli.run(["unabsorb", "feature/unabsorb-stash-stable", "--allow-agent", "--allow-unsafe"])).toBe(0)
    const targetPath = join(repoRoot, ".worktree", "feature", "unabsorb-stash-stable")
    expect(await readFile(join(targetPath, "stable-unabsorb.txt"), "utf8")).toBe("from-primary\n")
  })

  it("unabsorb rejects --to when managed worktree name does not resolve", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/unabsorb-invalid"])).toBe(0)
    expect(
      await cli.run(["use", "feature/unabsorb-invalid", "--allow-shared", "--allow-agent", "--allow-unsafe"]),
    ).toBe(0)

    await writeFile(join(repoRoot, "invalid-to.txt"), "invalid\n", "utf8")
    expect(
      await cli.run([
        "unabsorb",
        "feature/unabsorb-invalid",
        "--to",
        ".worktree/feature/unabsorb-invalid",
        "--allow-agent",
        "--allow-unsafe",
      ]),
    ).toBe(4)
    expect(stderr.some((line) => line.includes("target worktree not found"))).toBe(true)
  })

  it("unabsorb auto-restores primary changes when pre-hook fails", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const cli = createCli({
      cwd: repoRoot,
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/unabsorb-hook-fail"])).toBe(0)
    expect(
      await cli.run(["use", "feature/unabsorb-hook-fail", "--allow-shared", "--allow-agent", "--allow-unsafe"]),
    ).toBe(0)
    await writeFile(join(repoRoot, "restore-primary.txt"), "restore\n", "utf8")

    await writeExecutableHook({
      repoRoot,
      hookName: "pre-unabsorb",
      body: `#!/usr/bin/env bash
set -eu
exit 1
`,
    })

    expect(await cli.run(["unabsorb", "feature/unabsorb-hook-fail", "--allow-agent", "--allow-unsafe"])).toBe(10)
    expect(await readFile(join(repoRoot, "restore-primary.txt"), "utf8")).toBe("restore\n")
    const primaryStatus = await runGit(repoRoot, ["status", "--porcelain"])
    expect(primaryStatus).toContain("restore-primary.txt")
  })

  it("prints general help when command is omitted", async () => {
    const stdout: string[] = []
    const cli = createCli({
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run([])).toBe(0)
    const helpText = expectSingleStdoutLine(stdout)
    expect(helpText).toContain("Usage:")
    expect(helpText).toContain("Commands:")
  })

  it("returns JSON error for unknown help target", async () => {
    const stdout: string[] = []
    const cli = createCli({
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["help", "not-found-command", "--json"])).toBe(3)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      status: string
      command: string
      code: string
      repoRoot: string | null
    }
    expect(payload.status).toBe("error")
    expect(payload.command).toBe("help")
    expect(payload.code).toBe("INVALID_ARGUMENT")
    expect(payload.repoRoot).toBeNull()
  })

  it("validates malformed options before command execution", async () => {
    const stderr: string[] = []
    const cli = createCli({
      stderr: (line) => stderr.push(line),
    })

    const cases: ReadonlyArray<{ args: string[]; expectedMessage: string }> = [
      { args: ["--unknown"], expectedMessage: "Unknown option: --unknown" },
      { args: ["list", "--hook-timeout-ms="], expectedMessage: "Missing value for option: --hook-timeout-ms" },
      { args: ["list", "--hook-timeout-ms"], expectedMessage: "Missing value for option: --hook-timeout-ms" },
      {
        args: ["list", "--hook-timeout-ms", "--json"],
        expectedMessage: "Missing value for option: --hook-timeout-ms",
      },
      { args: ["-x"], expectedMessage: "Unknown option: -x" },
      { args: ["-xv"], expectedMessage: "Unknown option: -x" },
    ]

    for (const testCase of cases) {
      stderr.length = 0
      expect(await cli.run(testCase.args)).toBe(3)
      expect(stderr.some((line) => line.includes(testCase.expectedMessage))).toBe(true)
    }
  })

  it("accepts boolean negation, inline value options, and grouped short options", async () => {
    const stderr: string[] = []
    const cli = createCli({
      stderr: (line) => stderr.push(line),
    })

    stderr.length = 0
    expect(await cli.run(["-vh"])).toBe(0)

    stderr.length = 0
    const noGhExitCode = await cli.run(["help", "--no-gh"])
    expect(noGhExitCode).not.toBe(3)
    expect(stderr.some((line) => line.includes("Unknown option"))).toBe(false)

    stderr.length = 0
    const inlineValueExitCode = await cli.run(["help", "--hook-timeout-ms=100"])
    expect(inlineValueExitCode).not.toBe(3)
    expect(stderr.some((line) => line.includes("Missing value for option"))).toBe(false)
  })

  it("init --json returns initialization metadata", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init", "--json"])).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      status: string
      initialized: boolean
      alreadyInitialized: boolean
    }
    expect(payload.status).toBe("ok")
    expect(payload.initialized).toBe(true)
    expect(payload.alreadyInitialized).toBe(false)
  })

  it("list without --json prints branch, flags, and path", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/list"])).toBe(0)
    const featurePath = join(repoRoot, ".worktree", "feature", "list")
    await writeFile(join(featurePath, "feature-list.txt"), "list\n", "utf8")
    await runGit(featurePath, ["add", "feature-list.txt"])
    await runGit(featurePath, ["commit", "-m", "feature list commit"])
    stdout.length = 0

    expect(await cli.run(["list"])).toBe(0)
    expect(stdout.length).toBeGreaterThan(0)

    const text = stdout.join("\n")
    expect(text).toContain("branch")
    expect(text).toContain("dirty")
    expect(text).toContain("merged")
    expect(text).toContain("pr")
    expect(text).toContain("locked")
    expect(text).toContain("ahead")
    expect(text).toContain("behind")
    expect(text).toContain("path")
    expect(stdout.some((line) => line.startsWith("┌") || line.startsWith("+"))).toBe(true)

    const mainLine = stdout.find((line) => line.includes("* main"))
    expect(mainLine).toBeDefined()
    const mainCells = (mainLine as string)
      .split(/[│|]/)
      .map((cell) => cell.trim())
      .filter((cell) => cell.length > 0)
    expect(mainCells[0]).toBe("* main")
    expect(mainCells[1]).toBe("clean")
    expect(mainCells[2]).toBe("-")
    expect(mainCells[3]).toBe("-")
    expect(mainCells[4]).toBe("-")
    expect(mainCells[5]).toBe("0")
    expect(mainCells[6]).toBe("0")
    expect(mainCells[7]).toContain(repoRoot)

    const featureLine = stdout.find((line) => line.includes("feature/list"))
    expect(featureLine).toBeDefined()
    const featureCells = (featureLine as string)
      .split(/[│|]/)
      .map((cell) => cell.trim())
      .filter((cell) => cell.length > 0)
    expect(featureCells[2]).not.toBe("-")
    expect(["merged", "unmerged", "unknown"]).toContain(featureCells[2])
    expect(["none", "open", "merged", "closed_unmerged", "unknown"]).toContain(featureCells[3] ?? "")
    expect(featureCells[5]).toBe("1")
    expect(featureCells[6]).toBe("0")
  })

  it("list reflects config.yml columns and list --json includes managedWorktreeRoot", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const managedRoot = await mkdtemp(join(tmpdir(), "vde-worktree-managed-root-"))
    tempDirs.add(managedRoot)
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    await writeFile(
      join(repoRoot, ".vde", "worktree", "config.yml"),
      ["paths:", `  worktreeRoot: ${managedRoot}`, "list:", "  table:", "    columns: [branch, locked]"].join("\n"),
      "utf8",
    )

    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    const exclude = await readFile(join(repoRoot, ".git", "info", "exclude"), "utf8")
    expect(exclude.includes("# vde-worktree (managed)")).toBe(false)
    expect(await cli.run(["switch", "feature/config-columns"])).toBe(0)
    const switchedPath = expectSingleStdoutLine(stdout.slice(-1))
    expect(switchedPath.startsWith(managedRoot)).toBe(true)

    stdout.length = 0
    expect(await cli.run(["list"])).toBe(0)
    const listText = stdout.join("\n")
    expect(listText).toContain("branch")
    expect(listText).toContain("locked")
    expect(listText).not.toContain("│ path")

    stdout.length = 0
    expect(await cli.run(["list", "--full-path"])).toBe(0)
    expect(stdout.join("\n")).not.toContain("│ path")

    stdout.length = 0
    expect(await cli.run(["list", "--json"])).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      managedWorktreeRoot: string
      worktrees: Array<{ branch: string | null; path: string }>
    }
    expect(payload.managedWorktreeRoot).toBe(managedRoot)
    const managedFeature = payload.worktrees.find((worktree) => worktree.branch === "feature/config-columns")
    expect(managedFeature).toBeDefined()
    const resolvedManagedRoot = await realpath(payload.managedWorktreeRoot)
    const resolvedManagedFeaturePath = await realpath((managedFeature as { path: string }).path)
    expect(resolvedManagedFeaturePath.startsWith(resolvedManagedRoot)).toBe(true)
  })

  it("list --no-gh skips gh command invocation", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/no-gh"])).toBe(0)

    const shimDir = await mkdtemp(join(tmpdir(), "vde-worktree-gh-shim-"))
    tempDirs.add(shimDir)
    const ghPath = join(shimDir, "gh")
    const ghLogPath = join(shimDir, "gh.log")
    await writeFile(
      ghPath,
      `#!/bin/sh
echo "$*" >> "${ghLogPath}"
echo '[]'
`,
      "utf8",
    )
    await chmod(ghPath, 0o755)

    envBackup.set("PATH", process.env.PATH)
    process.env.PATH = `${shimDir}:${process.env.PATH ?? ""}`

    stdout.length = 0
    expect(await cli.run(["list", "--json"])).toBe(0)
    expect(await readFile(ghLogPath, "utf8")).toContain("pr list")

    await rm(ghLogPath, { force: true })

    stdout.length = 0
    expect(await cli.run(["list", "--json", "--no-gh"])).toBe(0)
    await expect(access(ghLogPath)).rejects.toThrow()
  })

  it("list --json includes pr.url when gh returns PR metadata", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/pr-url"])).toBe(0)

    const shimDir = await mkdtemp(join(tmpdir(), "vde-worktree-gh-shim-pr-url-"))
    tempDirs.add(shimDir)
    const ghPath = join(shimDir, "gh")
    await writeFile(
      ghPath,
      `#!/bin/sh
echo '[{"headRefName":"feature/pr-url","state":"OPEN","mergedAt":null,"updatedAt":"2026-02-17T00:00:00Z","url":"https://github.com/example/repo/pull/987"}]'
`,
      "utf8",
    )
    await chmod(ghPath, 0o755)

    envBackup.set("PATH", process.env.PATH)
    process.env.PATH = `${shimDir}:${process.env.PATH ?? ""}`

    stdout.length = 0
    expect(await cli.run(["list", "--json"])).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      worktrees: Array<{
        branch: string | null
        pr: {
          status: string | null
          url: string | null
        }
      }>
    }
    const target = payload.worktrees.find((worktree) => worktree.branch === "feature/pr-url")
    expect(target).toBeDefined()
    expect(target?.pr.status).toBe("open")
    expect(target?.pr.url).toBe("https://github.com/example/repo/pull/987")
  })

  it("list truncates long path in narrow tty and --full-path disables truncation", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/truncate-path"])).toBe(0)

    const mutableStdout = process.stdout as NodeJS.WriteStream & { isTTY?: boolean; columns?: number }
    const hadOwnIsTTY = Object.prototype.hasOwnProperty.call(mutableStdout, "isTTY")
    const previousIsTTY = mutableStdout.isTTY
    const hadOwnColumns = Object.prototype.hasOwnProperty.call(mutableStdout, "columns")
    const previousColumns = mutableStdout.columns

    Object.defineProperty(mutableStdout, "isTTY", {
      value: true,
      configurable: true,
      writable: true,
    })
    Object.defineProperty(mutableStdout, "columns", {
      value: 90,
      configurable: true,
      writable: true,
    })

    try {
      stdout.length = 0
      expect(await cli.run(["list"])).toBe(0)
      const truncatedText = stdout.join("\n")
      expect(truncatedText).toContain("feature/truncate-path")
      expect(truncatedText).toContain("…")

      stdout.length = 0
      expect(await cli.run(["list", "--full-path"])).toBe(0)
      const fullPathText = stdout.join("\n")
      expect(fullPathText).toContain(join(repoRoot, ".worktree", "feature", "truncate-path"))
      expect(fullPathText).not.toContain("…")
    } finally {
      if (hadOwnIsTTY) {
        Object.defineProperty(mutableStdout, "isTTY", {
          value: previousIsTTY,
          configurable: true,
          writable: true,
        })
      } else {
        Reflect.deleteProperty(mutableStdout as unknown as Record<string, unknown>, "isTTY")
      }

      if (hadOwnColumns) {
        Object.defineProperty(mutableStdout, "columns", {
          value: previousColumns,
          configurable: true,
          writable: true,
        })
      } else {
        Reflect.deleteProperty(mutableStdout as unknown as Record<string, unknown>, "columns")
      }
    }
  })

  it("list applies catppuccin colors in interactive mode", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
      isInteractive: () => true,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/list-colored"])).toBe(0)
    stdout.length = 0

    expect(await cli.run(["list"])).toBe(0)
    const text = stdout.join("\n")
    expect(text).toContain("feature/list-colored")
    expect(text).toMatch(/\u001b\[38;2;/)
  })

  it("returns INVALID_CONFIG when config.yml schema is invalid", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    await writeFile(join(repoRoot, ".vde", "worktree", "config.yml"), "list:\n  table:\n    unknown: true\n", "utf8")

    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
    })

    expect(await cli.run(["list"])).toBe(3)
    expect(stderr.some((line) => line.includes("INVALID_CONFIG"))).toBe(true)
  })

  it("returns INVALID_REMOTE_BRANCH_FORMAT for malformed get target", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
    })

    expect(await cli.run(["get", "invalid-format"])).toBe(3)
    expect(stderr.some((line) => line.includes("INVALID_REMOTE_BRANCH_FORMAT"))).toBe(true)
  })

  it("returns INVALID_ARGUMENT for malformed invoke hook name", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
    })

    expect(await cli.run(["invoke", "post_invalid"])).toBe(3)
    expect(stderr.some((line) => line.includes("hookName must be pre-* or post-*"))).toBe(true)
  })

  it("returns INVALID_ARGUMENT when extract uses --current and --from together", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stderr: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stderr: (line) => stderr.push(line),
    })

    expect(await cli.run(["extract", "--current", "--from", "."])).toBe(3)
    expect(stderr.some((line) => line.includes("extract cannot use --current and --from together"))).toBe(true)
  })

  it("exec validates arguments after -- and supports --json success payload", async () => {
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
    expect(await cli.run(["switch", "feature/exec-ok"])).toBe(0)

    stderr.length = 0
    expect(await cli.run(["exec", "feature/exec-ok", "--"])).toBe(3)
    expect(stderr.some((line) => line.includes("exec requires arguments after --"))).toBe(true)

    stdout.length = 0
    expect(await cli.run(["exec", "feature/exec-ok", "--json", "--", "node", "-e", "process.exit(0)"])).toBe(0)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as {
      status: string
      childExitCode: number
      branch: string
    }
    expect(payload.status).toBe("ok")
    expect(payload.childExitCode).toBe(0)
    expect(payload.branch).toBe("feature/exec-ok")
  })

  it("handles lock/unlock conflicts for invalid metadata and owner mismatch", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/lock-conflict"])).toBe(0)

    const lockPath = join(repoRoot, ".vde", "worktree", "locks", `${branchToWorktreeId("feature/lock-conflict")}.json`)
    await writeFile(lockPath, "{invalid", "utf8")

    stdout.length = 0
    expect(await cli.run(["lock", "feature/lock-conflict", "--json"])).toBe(4)
    expect((JSON.parse(expectSingleStdoutLine(stdout)) as { code: string }).code).toBe("LOCK_CONFLICT")

    stdout.length = 0
    expect(await cli.run(["unlock", "feature/lock-conflict", "--json"])).toBe(4)
    expect((JSON.parse(expectSingleStdoutLine(stdout)) as { code: string }).code).toBe("LOCK_CONFLICT")

    stdout.length = 0
    expect(await cli.run(["unlock", "feature/lock-conflict", "--force", "--json"])).toBe(0)
    expect((JSON.parse(expectSingleStdoutLine(stdout)) as { locked: { value: boolean } }).locked.value).toBe(false)

    stdout.length = 0
    expect(await cli.run(["lock", "feature/lock-conflict", "--owner", "alice", "--json"])).toBe(0)
    stdout.length = 0
    expect(await cli.run(["lock", "feature/lock-conflict", "--owner", "bob", "--json"])).toBe(4)
    expect((JSON.parse(expectSingleStdoutLine(stdout)) as { code: string }).code).toBe("LOCK_CONFLICT")

    stdout.length = 0
    expect(await cli.run(["unlock", "feature/lock-conflict", "--owner", "bob", "--json"])).toBe(4)
    expect((JSON.parse(expectSingleStdoutLine(stdout)) as { code: string }).code).toBe("LOCK_CONFLICT")
  })

  it("cd maps dependency errors and parses prompt/fzf args", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const selectPathWithFzf = vi.fn<(input: SelectPathWithFzfInput) => Promise<SelectPathWithFzfResult>>(async () => {
      throw new FzfDependencyError()
    })

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
      selectPathWithFzf,
      isInteractive: () => false,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/cd"])).toBe(0)
    stdout.length = 0

    expect(await cli.run(["cd", "--json", "--prompt=pick> ", "--fzf-arg=--ansi", "--fzf-arg", "--nth=1"])).toBe(5)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as { code: string; message: string }
    expect(payload.code).toBe("DEPENDENCY_MISSING")
    expect(payload.message).toContain("fzf is required")
    const firstCall = selectPathWithFzf.mock.calls[0]?.[0] ?? null
    expect(firstCall).not.toBeNull()
    expect((firstCall as SelectPathWithFzfInput).prompt).toBe("pick> ")
    expect((firstCall as SelectPathWithFzfInput).fzfExtraArgs).toEqual([
      "--delimiter=\t",
      "--with-nth=1",
      "--preview=printf '%b' {3}",
      "--preview-window=right,60%,wrap",
      "--ansi",
      "--nth=1",
    ])
  })

  it("cd reads selector settings from config.yml when CLI flags are omitted", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    await writeFile(
      join(repoRoot, ".vde", "worktree", "config.yml"),
      [
        "selector:",
        "  cd:",
        '    prompt: "cfg> "',
        "    surface: auto",
        '    tmuxPopupOpts: "70%,60%"',
        "    fzf:",
        "      extraArgs:",
        "        - --cycle",
      ].join("\n"),
      "utf8",
    )

    const stdout: string[] = []
    const selectPathWithFzf = vi.fn<(input: SelectPathWithFzfInput) => Promise<SelectPathWithFzfResult>>(
      async ({ candidates }) => {
        return {
          status: "selected",
          path: candidates[0] ?? "",
        }
      },
    )

    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
      selectPathWithFzf,
      isInteractive: () => true,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["switch", "feature/cd-config"])).toBe(0)
    stdout.length = 0
    expect(await cli.run(["cd", "--json"])).toBe(0)

    const firstCall = selectPathWithFzf.mock.calls[0]?.[0] ?? null
    expect(firstCall).not.toBeNull()
    expect((firstCall as SelectPathWithFzfInput).prompt).toBe("cfg> ")
    expect((firstCall as SelectPathWithFzfInput).surface).toBe("auto")
    expect((firstCall as SelectPathWithFzfInput).tmuxPopupOpts).toBe("70%,60%")
    expect((firstCall as SelectPathWithFzfInput).fzfExtraArgs).toEqual([
      "--delimiter=\t",
      "--with-nth=1",
      "--preview=printf '%b' {3}",
      "--preview-window=right,60%,wrap",
      "--ansi",
      "--cycle",
    ])
  })

  it("cd returns 130 when selection is cancelled", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const selectPathWithFzf = vi.fn<(input: SelectPathWithFzfInput) => Promise<SelectPathWithFzfResult>>(async () => ({
      status: "cancelled",
    }))

    const cli = createCli({
      cwd: repoRoot,
      selectPathWithFzf,
      isInteractive: () => true,
    })

    expect(await cli.run(["init"])).toBe(0)
    expect(await cli.run(["cd"])).toBe(130)
  })

  it("returns UNKNOWN_COMMAND for unsupported command names", async () => {
    const repoRoot = await setupRepo()
    tempDirs.add(repoRoot)
    const stdout: string[] = []
    const cli = createCli({
      cwd: repoRoot,
      stdout: (line) => stdout.push(line),
    })

    expect(await cli.run(["not-a-command", "--json"])).toBe(3)
    const payload = JSON.parse(expectSingleStdoutLine(stdout)) as { code: string; command: string }
    expect(payload.code).toBe("UNKNOWN_COMMAND")
    expect(payload.command).toBe("not-a-command")
  })
})
