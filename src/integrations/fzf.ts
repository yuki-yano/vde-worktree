import { execa } from "execa"

const FZF_BINARY = "fzf"
const FZF_CHECK_TIMEOUT_MS = 5_000
const RESERVED_FZF_ARGS = new Set(["prompt", "layout", "height", "border", "tmux"])
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

type FzfErrorCode =
  | "FZF_DEPENDENCY_MISSING"
  | "FZF_INTERACTIVE_REQUIRED"
  | "FZF_INVALID_ARGUMENT"
  | "FZF_INVALID_SELECTION"

type FzfErrorOptions = {
  readonly code: FzfErrorCode
  readonly message: string
}

class FzfError extends Error {
  readonly code: FzfErrorCode

  constructor(options: FzfErrorOptions) {
    super(options.message)
    this.name = "FzfError"
    this.code = options.code
  }
}

export class FzfDependencyError extends FzfError {
  constructor(message = "fzf is required for interactive selection") {
    super({
      code: "FZF_DEPENDENCY_MISSING",
      message,
    })
    this.name = "FzfDependencyError"
  }
}

export class FzfInteractiveRequiredError extends FzfError {
  constructor(message = "fzf selection requires an interactive terminal") {
    super({
      code: "FZF_INTERACTIVE_REQUIRED",
      message,
    })
    this.name = "FzfInteractiveRequiredError"
  }
}

class FzfInvalidArgumentError extends FzfError {
  constructor(message: string) {
    super({
      code: "FZF_INVALID_ARGUMENT",
      message,
    })
    this.name = "FzfInvalidArgumentError"
  }
}

class FzfInvalidSelectionError extends FzfError {
  constructor(message: string) {
    super({
      code: "FZF_INVALID_SELECTION",
      message,
    })
    this.name = "FzfInvalidSelectionError"
  }
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
  readonly surface?: "auto" | "inline" | "tmux-popup"
  readonly tmuxPopupOpts?: string
  readonly fzfExtraArgs?: ReadonlyArray<string>
  readonly cwd?: string
  readonly env?: NodeJS.ProcessEnv
  readonly isInteractive?: () => boolean
  readonly checkFzfAvailability?: () => Promise<boolean>
  readonly checkFzfTmuxSupport?: () => Promise<boolean>
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
      throw new FzfInvalidArgumentError("Empty value is not allowed for --fzf-arg")
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
      throw new FzfInvalidArgumentError(`--fzf-arg cannot override reserved fzf option: --${optionName}`)
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

const defaultCheckFzfTmuxSupport = async (): Promise<boolean> => {
  try {
    const result = await execa(FZF_BINARY, ["--help"], {
      timeout: FZF_CHECK_TIMEOUT_MS,
    })
    return result.stdout.includes("--tmux")
  } catch {
    return false
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

  throw new FzfDependencyError()
}

const shouldTryTmuxPopup = async ({
  surface,
  env,
  checkFzfTmuxSupport,
}: {
  readonly surface: "auto" | "inline" | "tmux-popup"
  readonly env: NodeJS.ProcessEnv
  readonly checkFzfTmuxSupport: () => Promise<boolean>
}): Promise<boolean> => {
  if (surface === "inline") {
    return false
  }
  if (surface === "tmux-popup") {
    return true
  }
  if (typeof env.TMUX !== "string" || env.TMUX.length === 0) {
    return false
  }
  try {
    return await checkFzfTmuxSupport()
  } catch {
    return false
  }
}

const isTmuxUnknownOptionError = (error: unknown): boolean => {
  const execaError = error as ExecaLikeError & {
    readonly stderr?: string
    readonly stdout?: string
    readonly shortMessage?: string
  }
  const text = [execaError.message, execaError.shortMessage, execaError.stderr, execaError.stdout]
    .filter((value): value is string => typeof value === "string" && value.length > 0)
    .join("\n")
  return /unknown option.*--tmux|--tmux.*unknown option/i.test(text)
}

export const selectPathWithFzf = async ({
  candidates,
  prompt = "worktree> ",
  surface = "inline",
  tmuxPopupOpts = "80%,70%",
  fzfExtraArgs = [],
  cwd = process.cwd(),
  env = process.env,
  isInteractive = (): boolean => process.stdout.isTTY === true && process.stderr.isTTY === true,
  checkFzfAvailability = defaultCheckFzfAvailability,
  checkFzfTmuxSupport = defaultCheckFzfTmuxSupport,
  runFzf = defaultRunFzf,
}: SelectPathWithFzfInput): Promise<SelectPathWithFzfResult> => {
  if (candidates.length === 0) {
    throw new FzfInvalidArgumentError("No candidates provided for fzf selection")
  }

  if (isInteractive() !== true) {
    throw new FzfInteractiveRequiredError()
  }

  await ensureFzfAvailable(checkFzfAvailability)
  const baseArgs = buildFzfArgs({ prompt, fzfExtraArgs })
  const tryTmuxPopup = await shouldTryTmuxPopup({
    surface,
    env,
    checkFzfTmuxSupport,
  })
  const args = tryTmuxPopup ? [...baseArgs, `--tmux=${tmuxPopupOpts}`] : baseArgs
  const input = buildFzfInput(candidates)
  if (input.length === 0) {
    throw new FzfInvalidArgumentError("All candidates are empty after sanitization")
  }

  const candidateSet = new Set(input.split("\n").map((candidate) => stripAnsi(candidate)))

  const runWithValidation = async (fzfArgs: string[]): Promise<SelectPathWithFzfResult> => {
    const result = await runFzf({
      args: fzfArgs,
      input,
      cwd,
      env,
    })

    const selectedPath = stripAnsi(stripTrailingNewlines(result.stdout))
    if (selectedPath.length === 0) {
      return { status: "cancelled" }
    }

    if (!candidateSet.has(selectedPath)) {
      throw new FzfInvalidSelectionError("fzf returned a value that is not in the candidate list")
    }

    return {
      status: "selected",
      path: selectedPath,
    }
  }

  try {
    return await runWithValidation(args)
  } catch (error) {
    if (tryTmuxPopup && isTmuxUnknownOptionError(error)) {
      try {
        return await runWithValidation(baseArgs)
      } catch (fallbackError) {
        const fallbackExecaError = fallbackError as ExecaLikeError
        if (fallbackExecaError.exitCode === 130) {
          return { status: "cancelled" }
        }
        throw fallbackError
      }
    }
    const execaError = error as ExecaLikeError
    if (execaError.exitCode === 130) {
      return { status: "cancelled" }
    }
    throw error
  }
}
