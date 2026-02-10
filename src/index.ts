#!/usr/bin/env node
import { createCli } from "./cli/index"

const main = async (): Promise<void> => {
  const cli = createCli()
  try {
    const exitCode = await cli.run(process.argv.slice(2))
    if (typeof exitCode === "number" && exitCode !== 0) {
      process.exit(exitCode)
    }
  } catch (error) {
    if (error instanceof Error) {
      console.error("Error:", error.message)
      if (process.env.VDE_WORKTREE_DEBUG === "true" || process.env.VDE_DEBUG === "true") {
        console.error(error.stack)
      }
    } else {
      console.error("An unexpected error occurred:", String(error))
    }

    process.exit(1)
  }
}

void main()
