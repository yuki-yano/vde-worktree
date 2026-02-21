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

type HookExecutionResult = {
  readonly exitCode: number
  readonly stderr: string
  readonly startedAt: string
  readonly endedAt: string
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

const ensureHookExists = async ({
  path,
  hookName,
  requireExists,
}: {
  readonly path: string
  readonly hookName: string
  readonly requireExists: boolean
}): Promise<boolean> => {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    if (requireExists) {
      throw createCliError("HOOK_NOT_FOUND", {
        message: `Hook not found: ${hookName}`,
        details: { hook: hookName, path },
      })
    }
    return false
  }
}

const ensureHookExecutable = async ({
  path,
  hookName,
}: {
  readonly path: string
  readonly hookName: string
}): Promise<void> => {
  try {
    await access(path, fsConstants.X_OK)
  } catch {
    throw createCliError("HOOK_NOT_EXECUTABLE", {
      message: `Hook is not executable: ${hookName}`,
      details: { hook: hookName, path },
    })
  }
}

const executeHookProcess = async ({
  path,
  args,
  context,
}: {
  readonly path: string
  readonly args: readonly string[]
  readonly context: HookExecutionContext
}): Promise<HookExecutionResult> => {
  const startedAt = new Date().toISOString()
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
  return {
    exitCode: result.exitCode ?? 0,
    stderr: result.stderr ?? "",
    startedAt,
    endedAt,
  }
}

const writeHookLog = async ({
  repoRoot,
  action,
  branch,
  hookName,
  phase,
  result,
}: {
  readonly repoRoot: string
  readonly action: string
  readonly branch?: string | null
  readonly hookName: string
  readonly phase: HookPhase
  readonly result: HookExecutionResult
}): Promise<void> => {
  const logContent = [
    `hook=${hookName}`,
    `phase=${phase}`,
    `start=${result.startedAt}`,
    `end=${result.endedAt}`,
    `exitCode=${String(result.exitCode)}`,
    `stderr=${result.stderr}`,
    "",
  ].join("\n")
  await appendHookLog({
    repoRoot,
    action,
    branch,
    content: logContent,
  })
}

const shouldIgnorePostHookFailure = ({
  phase,
  context,
}: {
  readonly phase: HookPhase
  readonly context: HookExecutionContext
}): boolean => {
  return phase === "post" && context.strictPostHooks !== true
}

const handleIgnoredPostHookFailure = ({
  context,
  hookName,
  message,
}: {
  readonly context: HookExecutionContext
  readonly hookName: string
  readonly message?: string
}): void => {
  context.stderr(message ?? `Hook failed: ${hookName}`)
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
  const exists = await ensureHookExists({
    path,
    hookName,
    requireExists,
  })
  if (exists !== true) {
    return
  }

  await ensureHookExecutable({
    path,
    hookName,
  })

  try {
    const result = await executeHookProcess({
      path,
      args,
      context,
    })
    await writeHookLog({
      repoRoot: context.repoRoot,
      action: context.action,
      branch: context.branch,
      hookName,
      phase,
      result,
    })

    if (result.exitCode === 0) {
      return
    }

    const message = `Hook failed: ${hookName} (exitCode=${String(result.exitCode || 1)})`
    if (shouldIgnorePostHookFailure({ phase, context })) {
      handleIgnoredPostHookFailure({
        context,
        hookName,
        message,
      })
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

    if (shouldIgnorePostHookFailure({ phase, context })) {
      handleIgnoredPostHookFailure({
        context,
        hookName,
      })
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
