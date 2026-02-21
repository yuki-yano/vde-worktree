import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

const fixtureRoots = new Set<string>()

export const createRepoFixture = async ({
  prefix = "vde-worktree-test-",
  setup,
}: {
  readonly prefix?: string
  readonly setup?: ((repoRoot: string) => Promise<void> | void) | undefined
} = {}): Promise<string> => {
  const repoRoot = await mkdtemp(join(tmpdir(), prefix))
  fixtureRoots.add(repoRoot)
  if (setup !== undefined) {
    await setup(repoRoot)
  }
  return repoRoot
}

export const cleanupRepoFixtures = async (): Promise<void> => {
  const roots = [...fixtureRoots]
  try {
    const results = await Promise.allSettled(
      roots.map(async (repoRoot) => {
        await rm(repoRoot, { recursive: true, force: true })
      }),
    )
    const errors = results
      .filter((result): result is PromiseRejectedResult => result.status === "rejected")
      .map((result) => result.reason)
    if (errors.length > 0) {
      throw new AggregateError(errors, "Failed to clean up some repo fixtures")
    }
  } finally {
    fixtureRoots.clear()
  }
}
