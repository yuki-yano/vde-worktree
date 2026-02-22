import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, describe, expect, it } from "vitest"
import { loadResolvedConfig } from "./loader"

const tempDirs = new Set<string>()
const envBackup = new Map<string, string | undefined>()

const createTempDir = async (prefix: string): Promise<string> => {
  const dir = await mkdtemp(join(tmpdir(), prefix))
  tempDirs.add(dir)
  return dir
}

const setEnv = (key: string, value: string | undefined): void => {
  if (envBackup.has(key) !== true) {
    envBackup.set(key, process.env[key])
  }
  if (value === undefined) {
    delete process.env[key]
    return
  }
  process.env[key] = value
}

afterEach(async () => {
  for (const [key, value] of envBackup.entries()) {
    if (value === undefined) {
      delete process.env[key]
    } else {
      process.env[key] = value
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

describe("loadResolvedConfig", () => {
  it("returns defaults when no config files are present", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-default-")
    await mkdir(join(repoRoot, ".git"), { recursive: true })

    const result = await loadResolvedConfig({
      cwd: repoRoot,
      repoRoot,
    })

    expect(result.loadedFiles).toEqual([])
    expect(result.config.paths.worktreeRoot).toBe(".worktree")
    expect(result.config.list.table.columns).toEqual([
      "branch",
      "dirty",
      "merged",
      "pr",
      "locked",
      "ahead",
      "behind",
      "path",
    ])
    expect(result.config.selector.cd.surface).toBe("auto")
  })

  it("merges global < repoRoot < local(cwd-near) in priority order", async () => {
    const xdgRoot = await createTempDir("vde-worktree-config-xdg-")
    const repoRoot = await createTempDir("vde-worktree-config-repo-")
    const cwd = join(repoRoot, "apps", "api")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    await mkdir(cwd, { recursive: true })
    setEnv("XDG_CONFIG_HOME", xdgRoot)

    await mkdir(join(xdgRoot, "vde", "worktree"), { recursive: true })
    await writeFile(
      join(xdgRoot, "vde", "worktree", "config.yml"),
      ["hooks:", "  enabled: false", "  timeoutMs: 12000", "list:", "  table:", "    columns: [branch, path]"].join(
        "\n",
      ),
      "utf8",
    )

    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    await writeFile(
      join(repoRoot, ".vde", "worktree", "config.yml"),
      ["paths:", "  worktreeRoot: .worktrees", "hooks:", "  enabled: true"].join("\n"),
      "utf8",
    )

    await mkdir(join(cwd, ".vde", "worktree"), { recursive: true })
    await writeFile(
      join(cwd, ".vde", "worktree", "config.yml"),
      ["hooks:", "  timeoutMs: 9000", "selector:", "  cd:", '    prompt: "pick> "'].join("\n"),
      "utf8",
    )

    const result = await loadResolvedConfig({ cwd, repoRoot })
    expect(result.config.hooks.enabled).toBe(true)
    expect(result.config.hooks.timeoutMs).toBe(9000)
    expect(result.config.paths.worktreeRoot).toBe(".worktrees")
    expect(result.config.list.table.columns).toEqual(["branch", "path"])
    expect(result.config.selector.cd.prompt).toBe("pick> ")
  })

  it("always includes repoRoot local config even when cwd is in linked worktree", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-main-")
    const linkedRoot = await createTempDir("vde-worktree-config-linked-")
    const cwd = join(linkedRoot, "subdir")
    await mkdir(cwd, { recursive: true })
    await writeFile(join(linkedRoot, ".git"), "gitdir: /tmp/fake\n", "utf8")

    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    await writeFile(
      join(repoRoot, ".vde", "worktree", "config.yml"),
      ["locks:", "  timeoutMs: 3333"].join("\n"),
      "utf8",
    )

    const result = await loadResolvedConfig({ cwd, repoRoot })
    expect(result.config.locks.timeoutMs).toBe(3333)
  })

  it("deduplicates same realpath and keeps higher-priority candidate path", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-dedupe-")
    const cwd = join(repoRoot, "apps", "api")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    await mkdir(cwd, { recursive: true })

    const repoConfigDir = join(repoRoot, ".vde", "worktree")
    await mkdir(repoConfigDir, { recursive: true })
    const repoConfigPath = join(repoConfigDir, "config.yml")
    await writeFile(repoConfigPath, "hooks:\n  timeoutMs: 7777\n", "utf8")

    const cwdConfigDir = join(cwd, ".vde", "worktree")
    await mkdir(cwdConfigDir, { recursive: true })
    const cwdConfigPath = join(cwdConfigDir, "config.yml")
    await symlink(repoConfigPath, cwdConfigPath)

    const result = await loadResolvedConfig({ cwd, repoRoot })
    expect(result.config.hooks.timeoutMs).toBe(7777)
    expect(result.loadedFiles).toEqual([cwdConfigPath])
  })

  it("replaces array values across layers instead of concatenating", async () => {
    const xdgRoot = await createTempDir("vde-worktree-config-array-xdg-")
    const repoRoot = await createTempDir("vde-worktree-config-array-repo-")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    setEnv("XDG_CONFIG_HOME", xdgRoot)

    await mkdir(join(xdgRoot, "vde", "worktree"), { recursive: true })
    await writeFile(
      join(xdgRoot, "vde", "worktree", "config.yml"),
      ["selector:", "  cd:", "    fzf:", "      extraArgs:", "        - --cycle", "        - --ansi"].join("\n"),
      "utf8",
    )

    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    await writeFile(
      join(repoRoot, ".vde", "worktree", "config.yml"),
      ["selector:", "  cd:", "    fzf:", "      extraArgs:", "        - --info=inline"].join("\n"),
      "utf8",
    )

    const result = await loadResolvedConfig({ cwd: repoRoot, repoRoot })
    expect(result.config.selector.cd.fzf.extraArgs).toEqual(["--info=inline"])
  })

  it("accepts list.table.path.minWidth at boundaries 8 and 200", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-min-width-ok-")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    const configFile = join(repoRoot, ".vde", "worktree", "config.yml")

    await writeFile(
      configFile,
      ["list:", "  table:", "    path:", "      minWidth: 8", "      truncate: auto"].join("\n"),
      "utf8",
    )
    const lower = await loadResolvedConfig({ cwd: repoRoot, repoRoot })
    expect(lower.config.list.table.path.minWidth).toBe(8)

    await writeFile(
      configFile,
      ["list:", "  table:", "    path:", "      minWidth: 200", "      truncate: auto"].join("\n"),
      "utf8",
    )
    const upper = await loadResolvedConfig({ cwd: repoRoot, repoRoot })
    expect(upper.config.list.table.path.minWidth).toBe(200)
  })

  it("rejects list.table.path.minWidth outside 8..200", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-min-width-ng-")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    const configFile = join(repoRoot, ".vde", "worktree", "config.yml")

    await writeFile(
      configFile,
      ["list:", "  table:", "    path:", "      minWidth: 7", "      truncate: auto"].join("\n"),
      "utf8",
    )
    await expect(loadResolvedConfig({ cwd: repoRoot, repoRoot })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      details: {
        file: configFile,
        keyPath: "list.table.path.minWidth",
      },
    })

    await writeFile(
      configFile,
      ["list:", "  table:", "    path:", "      minWidth: 201", "      truncate: auto"].join("\n"),
      "utf8",
    )
    await expect(loadResolvedConfig({ cwd: repoRoot, repoRoot })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      details: {
        file: configFile,
        keyPath: "list.table.path.minWidth",
      },
    })
  })

  it("rejects duplicate/empty/unsupported list.table.columns", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-columns-ng-")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    const configFile = join(repoRoot, ".vde", "worktree", "config.yml")

    await writeFile(configFile, "list:\n  table:\n    columns: []\n", "utf8")
    await expect(loadResolvedConfig({ cwd: repoRoot, repoRoot })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      details: {
        file: configFile,
        keyPath: "list.table.columns",
      },
    })

    await writeFile(configFile, "list:\n  table:\n    columns: [branch, branch]\n", "utf8")
    await expect(loadResolvedConfig({ cwd: repoRoot, repoRoot })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      details: {
        file: configFile,
        keyPath: "list.table.columns.1",
      },
    })

    await writeFile(configFile, "list:\n  table:\n    columns: [branch, nope]\n", "utf8")
    await expect(loadResolvedConfig({ cwd: repoRoot, repoRoot })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      details: {
        file: configFile,
        keyPath: "list.table.columns.1",
      },
    })
  })

  it("throws INVALID_CONFIG on unknown keys", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-invalid-")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    const configFile = join(repoRoot, ".vde", "worktree", "config.yml")
    await writeFile(configFile, "unknown:\n  key: true\n", "utf8")

    await expect(loadResolvedConfig({ cwd: repoRoot, repoRoot })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      details: {
        file: configFile,
        keyPath: "unknown",
      },
    })
  })

  it("accepts worktreeRoot under .git", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-git-root-")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    const configFile = join(repoRoot, ".vde", "worktree", "config.yml")
    await writeFile(configFile, "paths:\n  worktreeRoot: .git/worktrees\n", "utf8")

    const result = await loadResolvedConfig({ cwd: repoRoot, repoRoot })
    expect(result.config.paths.worktreeRoot).toBe(".git/worktrees")
    expect(result.loadedFiles).toContain(configFile)
  })

  it("rejects worktreeRoot when it points to an existing file", async () => {
    const repoRoot = await createTempDir("vde-worktree-config-existing-file-")
    await mkdir(join(repoRoot, ".git"), { recursive: true })
    await writeFile(join(repoRoot, "managed-root-file"), "x", "utf8")
    await mkdir(join(repoRoot, ".vde", "worktree"), { recursive: true })
    await writeFile(
      join(repoRoot, ".vde", "worktree", "config.yml"),
      "paths:\n  worktreeRoot: managed-root-file\n",
      "utf8",
    )

    await expect(loadResolvedConfig({ cwd: repoRoot, repoRoot })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      details: {
        keyPath: "paths.worktreeRoot",
      },
    })
  })
})
