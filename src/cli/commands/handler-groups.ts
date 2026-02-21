export type CommandHandler = () => Promise<number>

export type CommandHandlerMap = ReadonlyMap<string, CommandHandler>

const createHandlerMap = (entries: ReadonlyArray<readonly [string, CommandHandler]>): CommandHandlerMap => {
  return new Map(entries)
}

export const dispatchCommandHandler = async ({
  command,
  handlers,
}: {
  readonly command: string
  readonly handlers: CommandHandlerMap
}): Promise<number | undefined> => {
  const handler = handlers.get(command)
  if (handler === undefined) {
    return undefined
  }
  return await handler()
}

export const createEarlyRepoCommandHandlers = ({
  initHandler,
  listHandler,
  statusHandler,
  pathHandler,
}: {
  readonly initHandler: CommandHandler
  readonly listHandler: CommandHandler
  readonly statusHandler: CommandHandler
  readonly pathHandler: CommandHandler
}): CommandHandlerMap => {
  return createHandlerMap([
    ["init", initHandler],
    ["list", listHandler],
    ["status", statusHandler],
    ["path", pathHandler],
  ])
}

export const createWriteCommandHandlers = ({
  newHandler,
  switchHandler,
}: {
  readonly newHandler: CommandHandler
  readonly switchHandler: CommandHandler
}): CommandHandlerMap => {
  return createHandlerMap([
    ["new", newHandler],
    ["switch", switchHandler],
  ])
}

export const createWriteMutationHandlers = ({
  mvHandler,
  delHandler,
}: {
  readonly mvHandler: CommandHandler
  readonly delHandler: CommandHandler
}): CommandHandlerMap => {
  return createHandlerMap([
    ["mv", mvHandler],
    ["del", delHandler],
  ])
}

export const createWorktreeActionHandlers = ({
  goneHandler,
  getHandler,
  extractHandler,
}: {
  readonly goneHandler: CommandHandler
  readonly getHandler: CommandHandler
  readonly extractHandler: CommandHandler
}): CommandHandlerMap => {
  return createHandlerMap([
    ["gone", goneHandler],
    ["get", getHandler],
    ["extract", extractHandler],
  ])
}

export const createSynchronizationHandlers = ({
  absorbHandler,
  unabsorbHandler,
  useHandler,
}: {
  readonly absorbHandler: CommandHandler
  readonly unabsorbHandler: CommandHandler
  readonly useHandler: CommandHandler
}): CommandHandlerMap => {
  return createHandlerMap([
    ["absorb", absorbHandler],
    ["unabsorb", unabsorbHandler],
    ["use", useHandler],
  ])
}

export const createMiscCommandHandlers = ({
  execHandler,
  invokeHandler,
  copyHandler,
  linkHandler,
  lockHandler,
  unlockHandler,
  cdHandler,
}: {
  readonly execHandler: CommandHandler
  readonly invokeHandler: CommandHandler
  readonly copyHandler: CommandHandler
  readonly linkHandler: CommandHandler
  readonly lockHandler: CommandHandler
  readonly unlockHandler: CommandHandler
  readonly cdHandler: CommandHandler
}): CommandHandlerMap => {
  return createHandlerMap([
    ["exec", execHandler],
    ["invoke", invokeHandler],
    ["copy", copyHandler],
    ["link", linkHandler],
    ["lock", lockHandler],
    ["unlock", unlockHandler],
    ["cd", cdHandler],
  ])
}
