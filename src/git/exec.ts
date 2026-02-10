import { execa } from "execa"
import { createCliError } from "../core/errors"

type ExecaFailure = Error & {
  readonly stderr?: string
  readonly stdout?: string
  readonly shortMessage?: string
  readonly exitCode?: number
}

export type RunGitCommandInput = {
  readonly cwd: string
  readonly args: readonly string[]
  readonly reject?: boolean
}

export type RunGitCommandOutput = {
  readonly stdout: string
  readonly stderr: string
  readonly exitCode: number
}

export const runGitCommand = async ({ cwd, args, reject = true }: RunGitCommandInput): Promise<RunGitCommandOutput> => {
  try {
    const result = await execa("git", [...args], {
      cwd,
      reject,
    })
    return {
      stdout: result.stdout,
      stderr: result.stderr,
      exitCode: result.exitCode ?? 0,
    }
  } catch (error) {
    const execaError = error as ExecaFailure
    throw createCliError("GIT_COMMAND_FAILED", {
      message: "git command failed",
      details: {
        command: ["git", ...args],
        cwd,
        exitCode: execaError.exitCode,
        stdout: execaError.stdout ?? "",
        stderr: execaError.stderr ?? "",
        shortMessage: execaError.shortMessage ?? execaError.message,
      },
      cause: error,
    })
  }
}

export const doesGitRefExist = async (cwd: string, ref: string): Promise<boolean> => {
  const result = await runGitCommand({
    cwd,
    args: ["show-ref", "--verify", "--quiet", ref],
    reject: false,
  })
  return result.exitCode === 0
}
