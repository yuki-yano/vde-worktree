import { EXIT_CODE } from "../../../core/constants"
import { createCliError } from "../../../core/errors"
import type { CommandContext } from "../../runtime/command-context"

type DispatchHandled = {
  readonly handled: true
  readonly exitCode: number
}

type DispatchNotHandled = {
  readonly handled: false
}

export type DispatchReadOnlyCommandsResult = DispatchHandled | DispatchNotHandled

type ReadOnlyCommandContext = Pick<
  CommandContext,
  "command" | "commandArgs" | "positionals" | "parsedArgs" | "jsonEnabled"
>

export type DispatchReadOnlyCommandsInput<CommandHelpEntry, CompletionShell extends string> = ReadOnlyCommandContext & {
  readonly version: string
  readonly availableCommandNames: readonly string[]
  readonly stdout: (line: string) => void
  readonly findCommandHelp: (commandName: string) => CommandHelpEntry | undefined
  readonly renderGeneralHelpText: (input: { readonly version: string }) => string
  readonly renderCommandHelpText: (input: { readonly entry: CommandHelpEntry }) => string
  readonly ensureArgumentCount: (input: {
    readonly command: string
    readonly args: readonly string[]
    readonly min: number
    readonly max: number
  }) => void
  readonly resolveCompletionShell: (value: string) => CompletionShell
  readonly loadCompletionScript: (shell: CompletionShell) => Promise<string>
  readonly resolveCompletionInstallPath: (input: {
    readonly shell: CompletionShell
    readonly requestedPath?: string
  }) => string
  readonly installCompletionScript: (input: {
    readonly content: string
    readonly destinationPath: string
  }) => Promise<void>
  readonly readStringOption: (parsedArgsRecord: Record<string, unknown>, key: string) => string | undefined
  readonly buildJsonSuccess: (input: {
    readonly command: string
    readonly status: "ok"
    readonly repoRoot: null
    readonly details: Record<string, unknown>
  }) => Record<string, unknown>
}

const handled = (exitCode: number): DispatchHandled => {
  return {
    handled: true,
    exitCode,
  }
}

const NOT_HANDLED: DispatchNotHandled = {
  handled: false,
}

export const dispatchReadOnlyCommands = async <CommandHelpEntry, CompletionShell extends string>(
  input: DispatchReadOnlyCommandsInput<CommandHelpEntry, CompletionShell>,
): Promise<DispatchReadOnlyCommandsResult> => {
  if (input.parsedArgs.help === true) {
    const commandHelpTarget = input.command !== "unknown" && input.command !== "help" ? input.command : null
    if (commandHelpTarget !== null) {
      const entry = input.findCommandHelp(commandHelpTarget)
      if (entry !== undefined) {
        input.stdout(`${input.renderCommandHelpText({ entry })}\n`)
        return handled(EXIT_CODE.OK)
      }
    }
    input.stdout(`${input.renderGeneralHelpText({ version: input.version })}\n`)
    return handled(EXIT_CODE.OK)
  }

  if (input.parsedArgs.version === true) {
    input.stdout(input.version)
    return handled(EXIT_CODE.OK)
  }

  if (input.positionals.length === 0) {
    input.stdout(`${input.renderGeneralHelpText({ version: input.version })}\n`)
    return handled(EXIT_CODE.OK)
  }

  if (input.command === "help") {
    const helpTarget = input.positionals[1]
    if (typeof helpTarget !== "string" || helpTarget.length === 0) {
      input.stdout(`${input.renderGeneralHelpText({ version: input.version })}\n`)
      return handled(EXIT_CODE.OK)
    }

    const entry = input.findCommandHelp(helpTarget)
    if (entry === undefined) {
      throw createCliError("INVALID_ARGUMENT", {
        message: `Unknown command for help: ${helpTarget}`,
        details: {
          requested: helpTarget,
          availableCommands: input.availableCommandNames,
        },
      })
    }

    input.stdout(`${input.renderCommandHelpText({ entry })}\n`)
    return handled(EXIT_CODE.OK)
  }

  if (input.command === "completion") {
    input.ensureArgumentCount({
      command: input.command,
      args: input.commandArgs,
      min: 1,
      max: 1,
    })

    const shell = input.resolveCompletionShell(input.commandArgs[0] as string)
    const script = await input.loadCompletionScript(shell)

    if (input.parsedArgs.install === true) {
      const destinationPath = input.resolveCompletionInstallPath({
        shell,
        requestedPath: input.readStringOption(input.parsedArgs, "path"),
      })
      await input.installCompletionScript({
        content: script,
        destinationPath,
      })

      if (input.jsonEnabled) {
        input.stdout(
          JSON.stringify(
            input.buildJsonSuccess({
              command: input.command,
              status: "ok",
              repoRoot: null,
              details: {
                shell,
                installed: true,
                path: destinationPath,
              },
            }),
          ),
        )
        return handled(EXIT_CODE.OK)
      }

      input.stdout(`installed completion: ${destinationPath}`)
      if (shell === "zsh") {
        input.stdout("zsh note: ensure completion path is in fpath, then run: autoload -Uz compinit && compinit")
      }
      return handled(EXIT_CODE.OK)
    }

    if (input.jsonEnabled) {
      input.stdout(
        JSON.stringify(
          input.buildJsonSuccess({
            command: input.command,
            status: "ok",
            repoRoot: null,
            details: {
              shell,
              installed: false,
              script,
            },
          }),
        ),
      )
      return handled(EXIT_CODE.OK)
    }

    input.stdout(script)
    return handled(EXIT_CODE.OK)
  }

  return NOT_HANDLED
}
