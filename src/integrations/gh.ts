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
  readonly stderr?: string
  readonly exitCode?: number
}

export class GhUnavailableError extends Error {
  readonly code = "GH_UNAVAILABLE"

  constructor(message = "gh command is unavailable") {
    super(message)
    this.name = "GhUnavailableError"
  }
}

export class GhCommandError extends Error {
  readonly code = "GH_COMMAND_FAILED"
  readonly details: {
    readonly exitCode: number
    readonly stderr: string
  }

  constructor({ exitCode, stderr }: { readonly exitCode: number; readonly stderr: string }) {
    super(`gh command failed with exitCode=${String(exitCode)}`)
    this.name = "GhCommandError"
    this.details = {
      exitCode,
      stderr,
    }
  }
}

type ResolvePrByBranchBatchInput = {
  readonly repoRoot: string
  readonly baseBranch: string | null
  readonly branches: readonly (string | null)[]
  readonly enabled?: boolean
  readonly runGh?: GhCommandRunner
}

export type PrStatus = "none" | "open" | "merged" | "closed_unmerged" | "unknown"
export type PrState = {
  readonly status: PrStatus
  readonly url: string | null
}

type PrSummary = {
  readonly headRefName?: string | null
  readonly state?: string | null
  readonly mergedAt?: string | null
  readonly updatedAt?: string | null
  readonly url?: string | null
}

const defaultRunGh: GhCommandRunner = async ({ cwd, args }) => {
  try {
    const result = await execa("gh", [...args], {
      cwd,
      reject: false,
    })
    return {
      exitCode: result.exitCode ?? 0,
      stdout: result.stdout,
      stderr: result.stderr,
    }
  } catch (error) {
    const execaError = error as ExecaLikeError
    if (execaError.code === "ENOENT") {
      throw new GhUnavailableError("gh command not found")
    }
    throw error
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

const buildUnknownPrStateMap = (branches: readonly string[]): Map<string, PrState> => {
  return new Map(
    branches.map((branch) => [
      branch,
      {
        status: "unknown",
        url: null,
      },
    ]),
  )
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

const toPrUrl = (record: PrSummary): string | null => {
  if (typeof record.url === "string" && record.url.length > 0) {
    return record.url
  }
  return null
}

const parsePrStateByBranch = ({
  raw,
  targetBranches,
}: {
  readonly raw: string
  readonly targetBranches: readonly string[]
}): Map<string, PrState> | null => {
  try {
    const parsed = JSON.parse(raw) as unknown
    if (Array.isArray(parsed) !== true) {
      return null
    }
    const targetBranchSet = new Set(targetBranches)
    const records = parsed as PrSummary[]
    const latestByBranch = new Map<
      string,
      { updatedAtMillis: number; index: number; status: PrStatus; url: string | null }
    >()

    for (const [index, record] of records.entries()) {
      if (typeof record?.headRefName !== "string" || record.headRefName.length === 0) {
        continue
      }
      if (targetBranchSet.has(record.headRefName) !== true) {
        continue
      }
      const updatedAtMillis = parseUpdatedAtMillis(record.updatedAt)
      const status = toPrStatus(record)
      const url = toPrUrl(record)
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
          url,
        })
      }
    }

    const result = new Map<string, PrState>()
    for (const branch of targetBranches) {
      const latest = latestByBranch.get(branch)
      if (latest === undefined) {
        result.set(branch, {
          status: "none",
          url: null,
        })
        continue
      }
      result.set(branch, {
        status: latest.status,
        url: latest.url,
      })
    }
    return result
  } catch {
    return null
  }
}

export const resolvePrStateByBranchBatch = async ({
  repoRoot,
  baseBranch,
  branches,
  enabled = true,
  runGh = defaultRunGh,
}: ResolvePrByBranchBatchInput): Promise<ReadonlyMap<string, PrState>> => {
  if (baseBranch === null) {
    return new Map()
  }

  const targetBranches = toTargetBranches({ branches, baseBranch })
  if (targetBranches.length === 0) {
    return new Map()
  }
  if (enabled !== true) {
    return buildUnknownPrStateMap(targetBranches)
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
        "headRefName,state,mergedAt,updatedAt,url",
      ],
    })
    if (result.exitCode !== 0) {
      throw new GhCommandError({
        exitCode: result.exitCode,
        stderr: result.stderr,
      })
    }

    const prStatusByBranch = parsePrStateByBranch({
      raw: result.stdout,
      targetBranches,
    })
    if (prStatusByBranch === null) {
      return buildUnknownPrStateMap(targetBranches)
    }

    return prStatusByBranch
  } catch (error) {
    if (error instanceof GhUnavailableError || error instanceof GhCommandError) {
      return buildUnknownPrStateMap(targetBranches)
    }
    const execaError = error as ExecaLikeError
    if (execaError.code === "ENOENT") {
      return buildUnknownPrStateMap(targetBranches)
    }
    return buildUnknownPrStateMap(targetBranches)
  }
}

export const resolvePrStatusByBranchBatch = async (
  input: ResolvePrByBranchBatchInput,
): Promise<ReadonlyMap<string, PrStatus>> => {
  const prStateByBranch = await resolvePrStateByBranchBatch(input)
  return new Map([...prStateByBranch.entries()].map(([branch, prState]) => [branch, prState.status]))
}

export const resolveMergedByPrBatch = async (
  input: ResolvePrByBranchBatchInput,
): Promise<ReadonlyMap<string, boolean | null>> => {
  const prStateByBranch = await resolvePrStateByBranchBatch(input)
  return new Map(
    [...prStateByBranch.entries()].map(([branch, prState]) => {
      const status = prState.status
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
