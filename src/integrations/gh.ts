import { execa } from "execa"

type GhCommandRunnerInput = {
  readonly cwd: string
  readonly args: readonly string[]
}

type GhCommandRunnerOutput = {
  readonly exitCode: number
  readonly stdout: string
  readonly stderr: string
}

type GhCommandRunner = (input: GhCommandRunnerInput) => Promise<GhCommandRunnerOutput>

type ExecaLikeError = Error & {
  readonly code?: string
}

type ResolveMergedByPrInput = {
  readonly repoRoot: string
  readonly branch: string
  readonly enabled?: boolean
  readonly runGh?: GhCommandRunner
}

type PrSummary = {
  readonly mergedAt?: string | null
}

const defaultRunGh: GhCommandRunner = async ({ cwd, args }) => {
  const result = await execa("gh", [...args], {
    cwd,
    reject: false,
  })
  return {
    exitCode: result.exitCode ?? 0,
    stdout: result.stdout,
    stderr: result.stderr,
  }
}

const parseMergedResult = (raw: string): boolean | null => {
  try {
    const parsed = JSON.parse(raw) as unknown
    if (Array.isArray(parsed) !== true) {
      return null
    }
    const records = parsed as PrSummary[]
    if (records.length === 0) {
      return false
    }

    return records.some((record) => typeof record?.mergedAt === "string" && record.mergedAt.length > 0)
  } catch {
    return null
  }
}

export const resolveMergedByPr = async ({
  repoRoot,
  branch,
  enabled = true,
  runGh = defaultRunGh,
}: ResolveMergedByPrInput): Promise<boolean | null> => {
  if (enabled !== true) {
    return null
  }

  try {
    const result = await runGh({
      cwd: repoRoot,
      args: ["pr", "list", "--state", "merged", "--head", branch, "--limit", "1", "--json", "mergedAt"],
    })
    if (result.exitCode !== 0) {
      return null
    }

    return parseMergedResult(result.stdout)
  } catch (error) {
    const execaError = error as ExecaLikeError
    if (execaError.code === "ENOENT") {
      return null
    }
    return null
  }
}
