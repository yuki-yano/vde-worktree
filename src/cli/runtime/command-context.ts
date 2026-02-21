export type CommandContext = {
  readonly command: string
  readonly commandArgs: readonly string[]
  readonly positionals: readonly string[]
  readonly parsedArgs: Record<string, unknown>
  readonly jsonEnabled: boolean
}
