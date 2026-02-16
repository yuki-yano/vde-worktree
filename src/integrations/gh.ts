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

type ResolvePrStatusByBranchBatchInput = {
  readonly repoRoot: string
  readonly baseBranch: string | null
  readonly branches: readonly (string | null)[]
  readonly enabled?: boolean
  readonly runGh?: GhCommandRunner
}

export type PrStatus = "none" | "open" | "merged" | "closed_unmerged" | "unknown"

type PrSummary = {
  readonly headRefName?: string | null
  readonly state?: string | null
  readonly mergedAt?: string | null
  readonly updatedAt?: string | null
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

const buildUnknownPrStatusMap = (branches: readonly string[]): Map<string, PrStatus> => {
  return new Map(branches.map((branch) => [branch, "unknown"]))
}

const parseUpdatedAtMillis = (value: unknown): number => {
  if (typeof value !== "string" || value.length === 0) {
    return Number.NEGATIVE_INFINITY
  }
  const parsed = Date.parse(value)
  if (Number.isNaN(parsed)) {
    return Number.NEGATIVE_INFINITY
  }
  return parsed
}

const toPrStatus = (record: PrSummary): PrStatus => {
  if (typeof record.mergedAt === "string" && record.mergedAt.length > 0) {
    return "merged"
  }
  const state = typeof record.state === "string" ? record.state.toUpperCase() : ""
  if (state === "MERGED") {
    return "merged"
  }
  if (state === "OPEN") {
    return "open"
  }
  if (state === "CLOSED") {
    return "closed_unmerged"
  }
  return "unknown"
}

const parsePrStatusByBranch = ({
  raw,
  targetBranches,
}: {
  readonly raw: string
  readonly targetBranches: readonly string[]
}): Map<string, PrStatus> | null => {
  try {
    const parsed = JSON.parse(raw) as unknown
    if (Array.isArray(parsed) !== true) {
      return null
    }
    const targetBranchSet = new Set(targetBranches)
    const records = parsed as PrSummary[]
    const latestByBranch = new Map<string, { updatedAtMillis: number; index: number; status: PrStatus }>()

    for (const [index, record] of records.entries()) {
      if (typeof record?.headRefName !== "string" || record.headRefName.length === 0) {
        continue
      }
      if (targetBranchSet.has(record.headRefName) !== true) {
        continue
      }
      const updatedAtMillis = parseUpdatedAtMillis(record.updatedAt)
      const status = toPrStatus(record)
      const current = latestByBranch.get(record.headRefName)
      if (
        current === undefined ||
        updatedAtMillis > current.updatedAtMillis ||
        (updatedAtMillis === current.updatedAtMillis && index > current.index)
      ) {
        latestByBranch.set(record.headRefName, {
          updatedAtMillis,
          index,
          status,
        })
      }
    }

    const result = new Map<string, PrStatus>()
    for (const branch of targetBranches) {
      const latest = latestByBranch.get(branch)
      result.set(branch, latest?.status ?? "none")
    }
    return result
  } catch {
    return null
  }
}

export const resolvePrStatusByBranchBatch = async ({
  repoRoot,
  baseBranch,
  branches,
  enabled = true,
  runGh = defaultRunGh,
}: ResolvePrStatusByBranchBatchInput): Promise<ReadonlyMap<string, PrStatus>> => {
  if (baseBranch === null) {
    return new Map()
  }

  const targetBranches = toTargetBranches({ branches, baseBranch })
  if (targetBranches.length === 0) {
    return new Map()
  }
  if (enabled !== true) {
    return buildUnknownPrStatusMap(targetBranches)
  }

  try {
    const searchQuery = targetBranches.map((branch) => `head:${branch}`).join(" OR ")
    const result = await runGh({
      cwd: repoRoot,
      args: [
        "pr",
        "list",
        "--state",
        "all",
        "--base",
        baseBranch,
        "--search",
        searchQuery,
        "--limit",
        "1000",
        "--json",
        "headRefName,state,mergedAt,updatedAt",
      ],
    })
    if (result.exitCode !== 0) {
      return buildUnknownPrStatusMap(targetBranches)
    }

    const prStatusByBranch = parsePrStatusByBranch({
      raw: result.stdout,
      targetBranches,
    })
    if (prStatusByBranch === null) {
      return buildUnknownPrStatusMap(targetBranches)
    }

    return prStatusByBranch
  } catch (error) {
    const execaError = error as ExecaLikeError
    if (execaError.code === "ENOENT") {
      return buildUnknownPrStatusMap(targetBranches)
    }
    return buildUnknownPrStatusMap(targetBranches)
  }
}

export const resolveMergedByPrBatch = async (
  input: ResolvePrStatusByBranchBatchInput,
): Promise<ReadonlyMap<string, boolean | null>> => {
  const prStatusByBranch = await resolvePrStatusByBranchBatch(input)
  return new Map(
    [...prStatusByBranch.entries()].map(([branch, status]) => {
      if (status === "merged") {
        return [branch, true]
      }
      if (status === "unknown") {
        return [branch, null]
      }
      return [branch, false]
    }),
  )
}
