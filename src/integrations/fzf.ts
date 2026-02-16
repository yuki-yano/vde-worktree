import { execa } from "execa"

const FZF_BINARY = "fzf"
const FZF_CHECK_TIMEOUT_MS = 5_000
const RESERVED_FZF_ARGS = new Set(["prompt", "layout", "height", "border"])
const ANSI_ESCAPE_SEQUENCE_PATTERN = String.raw`\u001B\[[0-?]*[ -/]*[@-~]`
const ANSI_ESCAPE_SEQUENCE_REGEX = new RegExp(ANSI_ESCAPE_SEQUENCE_PATTERN, "g")

type ExecaLikeError = Error & {
  readonly code?: string
  readonly exitCode?: number
  readonly timedOut?: boolean
}

type RunFzfInput = {
  readonly args: string[]
  readonly input: string
  readonly cwd: string
  readonly env: NodeJS.ProcessEnv
}

type RunFzfResult = {
  readonly stdout: string
}

export type SelectPathWithFzfResult =
  | {
      readonly status: "selected"
      readonly path: string
    }
  | {
      readonly status: "cancelled"
    }

export type SelectPathWithFzfInput = {
  readonly candidates: ReadonlyArray<string>
  readonly prompt?: string
  readonly fzfExtraArgs?: ReadonlyArray<string>
  readonly cwd?: string
  readonly env?: NodeJS.ProcessEnv
  readonly isInteractive?: () => boolean
  readonly checkFzfAvailability?: () => Promise<boolean>
  readonly runFzf?: (input: RunFzfInput) => Promise<RunFzfResult>
}

const sanitizeCandidate = (value: string): string => value.replace(/[\r\n]+/g, " ").trim()
const stripAnsi = (value: string): string => value.replace(ANSI_ESCAPE_SEQUENCE_REGEX, "")
const stripTrailingNewlines = (value: string): string => value.replace(/[\r\n]+$/g, "")

const buildFzfInput = (candidates: ReadonlyArray<string>): string => {
  return candidates
    .map((candidate) => sanitizeCandidate(candidate))
    .filter((candidate) => candidate.length > 0)
    .join("\n")
}

const validateExtraFzfArgs = (fzfExtraArgs: ReadonlyArray<string>): void => {
  for (const arg of fzfExtraArgs) {
    if (typeof arg !== "string" || arg.length === 0) {
      throw new Error("Empty value is not allowed for --fzf-arg")
    }

    if (!arg.startsWith("--")) {
      continue
    }

    const withoutPrefix = arg.slice(2)
    if (withoutPrefix.length === 0) {
      continue
    }

    const optionName = withoutPrefix.split("=")[0]
    if (optionName !== undefined && RESERVED_FZF_ARGS.has(optionName)) {
      throw new Error(`--fzf-arg cannot override reserved fzf option: --${optionName}`)
    }
  }
}

const buildFzfArgs = ({
  prompt,
  fzfExtraArgs,
}: {
  readonly prompt: string
  readonly fzfExtraArgs: ReadonlyArray<string>
}): string[] => {
  validateExtraFzfArgs(fzfExtraArgs)

  return [`--prompt=${prompt}`, "--layout=reverse", "--height=80%", "--border", ...fzfExtraArgs]
}

const defaultCheckFzfAvailability = async (): Promise<boolean> => {
  try {
    await execa(FZF_BINARY, ["--version"], {
      timeout: FZF_CHECK_TIMEOUT_MS,
    })
    return true
  } catch (error) {
    const execaError = error as ExecaLikeError
    if (
      execaError.code === "ENOENT" ||
      execaError.code === "ETIMEDOUT" ||
      execaError.code === "ERR_EXECA_TIMEOUT" ||
      execaError.timedOut === true
    ) {
      return false
    }
    throw error
  }
}

const defaultRunFzf = async ({ args, input, cwd, env }: RunFzfInput): Promise<RunFzfResult> => {
  const result = await execa(FZF_BINARY, args, {
    input,
    cwd,
    env,
    stderr: "inherit",
  })
  return { stdout: result.stdout }
}

const ensureFzfAvailable = async (checkFzfAvailability: () => Promise<boolean>): Promise<void> => {
  const available = await checkFzfAvailability()
  if (available) {
    return
  }

  throw new Error("fzf is required for interactive selection")
}

export const selectPathWithFzf = async ({
  candidates,
  prompt = "worktree> ",
  fzfExtraArgs = [],
  cwd = process.cwd(),
  env = process.env,
  isInteractive = (): boolean => process.stdout.isTTY === true && process.stderr.isTTY === true,
  checkFzfAvailability = defaultCheckFzfAvailability,
  runFzf = defaultRunFzf,
}: SelectPathWithFzfInput): Promise<SelectPathWithFzfResult> => {
  if (candidates.length === 0) {
    throw new Error("No candidates provided for fzf selection")
  }

  if (isInteractive() !== true) {
    throw new Error("fzf selection requires an interactive terminal")
  }

  await ensureFzfAvailable(checkFzfAvailability)
  const args = buildFzfArgs({ prompt, fzfExtraArgs })
  const input = buildFzfInput(candidates)
  if (input.length === 0) {
    throw new Error("All candidates are empty after sanitization")
  }

  const candidateSet = new Set(input.split("\n").map((candidate) => stripAnsi(candidate)))

  try {
    const result = await runFzf({
      args,
      input,
      cwd,
      env,
    })

    const selectedPath = stripAnsi(stripTrailingNewlines(result.stdout))
    if (selectedPath.length === 0) {
      return { status: "cancelled" }
    }

    if (!candidateSet.has(selectedPath)) {
      throw new Error("fzf returned a value that is not in the candidate list")
    }

    return {
      status: "selected",
      path: selectedPath,
    }
  } catch (error) {
    const execaError = error as ExecaLikeError
    if (execaError.exitCode === 130) {
      return { status: "cancelled" }
    }
    throw error
  }
}
