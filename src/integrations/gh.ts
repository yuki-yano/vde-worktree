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

type ResolveMergedByPrBatchInput = {
  readonly repoRoot: string
  readonly baseBranch: string | null
  readonly branches: readonly (string | null)[]
  readonly enabled?: boolean
  readonly runGh?: GhCommandRunner
}

type PrSummary = {
  readonly headRefName?: string | null
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

const toTargetBranches = ({
  branches,
  baseBranch,
}: {
  readonly branches: readonly (string | null)[]
  readonly baseBranch: string
}): string[] => {
  const uniqueBranches = new Set<string>()
  for (const branch of branches) {
    if (typeof branch !== "string" || branch.length === 0) {
      continue
    }
    if (branch === baseBranch) {
      continue
    }
    uniqueBranches.add(branch)
  }
  return [...uniqueBranches]
}

const buildUnknownBranchMap = (branches: readonly string[]): Map<string, null> => {
  return new Map(branches.map((branch) => [branch, null]))
}

const parseMergedBranches = ({
  raw,
  targetBranches,
}: {
  readonly raw: string
  readonly targetBranches: ReadonlySet<string>
}): Set<string> | null => {
  try {
    const parsed = JSON.parse(raw) as unknown
    if (Array.isArray(parsed) !== true) {
      return null
    }
    const records = parsed as PrSummary[]
    const mergedBranches = new Set<string>()

    for (const record of records) {
      if (typeof record?.headRefName !== "string" || record.headRefName.length === 0) {
        continue
      }
      if (targetBranches.has(record.headRefName) !== true) {
        continue
      }
      if (typeof record.mergedAt !== "string" || record.mergedAt.length === 0) {
        continue
      }
      mergedBranches.add(record.headRefName)
    }

    return mergedBranches
  } catch {
    return null
  }
}

export const resolveMergedByPrBatch = async ({
  repoRoot,
  baseBranch,
  branches,
  enabled = true,
  runGh = defaultRunGh,
}: ResolveMergedByPrBatchInput): Promise<ReadonlyMap<string, boolean | null>> => {
  if (enabled !== true) {
    return new Map()
  }
  if (baseBranch === null) {
    return new Map()
  }

  const targetBranches = toTargetBranches({ branches, baseBranch })
  if (targetBranches.length === 0) {
    return new Map()
  }

  try {
    const targetBranchSet = new Set(targetBranches)
    const searchQuery = targetBranches.map((branch) => `head:${branch}`).join(" OR ")
    const result = await runGh({
      cwd: repoRoot,
      args: [
        "pr",
        "list",
        "--state",
        "merged",
        "--base",
        baseBranch,
        "--search",
        searchQuery,
        "--limit",
        "1000",
        "--json",
        "headRefName,mergedAt",
      ],
    })
    if (result.exitCode !== 0) {
      return buildUnknownBranchMap(targetBranches)
    }

    const mergedBranches = parseMergedBranches({
      raw: result.stdout,
      targetBranches: targetBranchSet,
    })
    if (mergedBranches === null) {
      return buildUnknownBranchMap(targetBranches)
    }

    return new Map(
      targetBranches.map((branch) => {
        return [branch, mergedBranches.has(branch)]
      }),
    )
  } catch (error) {
    const execaError = error as ExecaLikeError
    if (execaError.code === "ENOENT") {
      return buildUnknownBranchMap(targetBranches)
    }
    return buildUnknownBranchMap(targetBranches)
  }
}
