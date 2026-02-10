import { constants as fsConstants } from "node:fs"
import { access, appendFile, mkdir } from "node:fs/promises"
import { join } from "node:path"
import { execa } from "execa"
import { DEFAULT_HOOK_TIMEOUT_MS } from "./constants"
import { CliError, createCliError } from "./errors"
import { getHooksDirectoryPath, getLogsDirectoryPath } from "./paths"

type HookPhase = "pre" | "post"

export type HookExecutionContext = {
  readonly repoRoot: string
  readonly action: string
  readonly branch?: string | null
  readonly worktreePath?: string
  readonly timeoutMs?: number
  readonly enabled: boolean
  readonly stderr: (line: string) => void
  readonly strictPostHooks?: boolean
  readonly extraEnv?: Record<string, string>
}

type HookExecutionError = Error & {
  readonly exitCode?: number
  readonly stderr?: string
  readonly code?: string
}

const nowTimestamp = (): string => {
  return new Date().toISOString().replace(/[^\d]/g, "").slice(0, 14)
}

const toLogFileName = ({ action, branch }: { readonly action: string; readonly branch?: string | null }): string => {
  const safeBranch = typeof branch === "string" && branch.length > 0 ? branch.replace(/[^\w.-]/g, "_") : "none"
  return `${nowTimestamp()}_${action}_${safeBranch}.log`
}

const hookPath = (repoRoot: string, hookName: string): string => {
  return join(getHooksDirectoryPath(repoRoot), hookName)
}

const appendHookLog = async ({
  repoRoot,
  action,
  branch,
  content,
}: {
  readonly repoRoot: string
  readonly action: string
  readonly branch?: string | null
  readonly content: string
}): Promise<void> => {
  const logsDir = getLogsDirectoryPath(repoRoot)
  await mkdir(logsDir, { recursive: true })
  const logPath = join(logsDir, toLogFileName({ action, branch }))
  await appendFile(logPath, content, "utf8")
}

const runHook = async ({
  phase,
  hookName,
  args,
  context,
  requireExists = false,
}: {
  readonly phase: HookPhase
  readonly hookName: string
  readonly args: readonly string[]
  readonly context: HookExecutionContext
  readonly requireExists?: boolean
}): Promise<void> => {
  if (context.enabled !== true) {
    return
  }

  const path = hookPath(context.repoRoot, hookName)
  try {
    await access(path, fsConstants.F_OK)
  } catch {
    if (requireExists) {
      throw createCliError("HOOK_NOT_FOUND", {
        message: `Hook not found: ${hookName}`,
        details: { hook: hookName, path },
      })
    }
    return
  }

  try {
    await access(path, fsConstants.X_OK)
  } catch {
    throw createCliError("HOOK_NOT_EXECUTABLE", {
      message: `Hook is not executable: ${hookName}`,
      details: { hook: hookName, path },
    })
  }

  const startedAt = new Date().toISOString()
  try {
    const result = await execa(path, [...args], {
      cwd: context.worktreePath ?? context.repoRoot,
      env: {
        ...process.env,
        WT_REPO_ROOT: context.repoRoot,
        WT_ACTION: context.action,
        WT_BRANCH: context.branch ?? "",
        WT_WORKTREE_PATH: context.worktreePath ?? "",
        WT_IS_TTY: process.stdout.isTTY === true ? "1" : "0",
        WT_TOOL: "vde-worktree",
        ...(context.extraEnv ?? {}),
      },
      timeout: context.timeoutMs ?? DEFAULT_HOOK_TIMEOUT_MS,
      reject: false,
    })

    const endedAt = new Date().toISOString()
    const logContent = [
      `hook=${hookName}`,
      `phase=${phase}`,
      `start=${startedAt}`,
      `end=${endedAt}`,
      `exitCode=${String(result.exitCode ?? 0)}`,
      `stderr=${result.stderr ?? ""}`,
      "",
    ].join("\n")
    await appendHookLog({
      repoRoot: context.repoRoot,
      action: context.action,
      branch: context.branch,
      content: logContent,
    })

    if ((result.exitCode ?? 0) === 0) {
      return
    }

    const message = `Hook failed: ${hookName} (exitCode=${String(result.exitCode ?? 1)})`
    if (phase === "post" && context.strictPostHooks !== true) {
      context.stderr(message)
      return
    }

    throw createCliError("HOOK_FAILED", {
      message,
      details: {
        hook: hookName,
        exitCode: result.exitCode,
        stderr: result.stderr,
      },
    })
  } catch (error) {
    if (error instanceof CliError) {
      throw error
    }

    const hookError = error as HookExecutionError
    if (hookError.code === "ETIMEDOUT" || hookError.code === "ERR_EXECA_TIMEOUT") {
      throw createCliError("HOOK_TIMEOUT", {
        message: `Hook timed out: ${hookName}`,
        details: {
          hook: hookName,
          timeoutMs: context.timeoutMs ?? DEFAULT_HOOK_TIMEOUT_MS,
          stderr: hookError.stderr ?? "",
        },
        cause: error,
      })
    }

    if (phase === "post" && context.strictPostHooks !== true) {
      context.stderr(`Hook failed: ${hookName}`)
      return
    }

    throw createCliError("HOOK_FAILED", {
      message: `Hook failed: ${hookName}`,
      details: {
        hook: hookName,
        stderr: hookError.stderr ?? hookError.message,
      },
      cause: error,
    })
  }
}

export const runPreHook = async ({
  name,
  context,
}: {
  readonly name: string
  readonly context: HookExecutionContext
}): Promise<void> => {
  await runHook({
    phase: "pre",
    hookName: `pre-${name}`,
    args: [],
    context,
  })
}

export const runPostHook = async ({
  name,
  context,
}: {
  readonly name: string
  readonly context: HookExecutionContext
}): Promise<void> => {
  await runHook({
    phase: "post",
    hookName: `post-${name}`,
    args: [],
    context,
  })
}

export const invokeHook = async ({
  hookName,
  args,
  context,
}: {
  readonly hookName: string
  readonly args: readonly string[]
  readonly context: HookExecutionContext
}): Promise<void> => {
  await runHook({
    phase: hookName.startsWith("pre-") ? "pre" : "post",
    hookName,
    args,
    context,
    requireExists: true,
  })
}
