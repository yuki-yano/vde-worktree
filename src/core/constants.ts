export const SCHEMA_VERSION = 1

export const EXIT_CODE = {
  OK: 0,
  NOT_GIT_REPOSITORY: 2,
  INVALID_ARGUMENT: 3,
  SAFETY_REJECTED: 4,
  DEPENDENCY_MISSING: 5,
  LOCK_FAILED: 6,
  HOOK_FAILED: 10,
  GIT_COMMAND_FAILED: 20,
  CHILD_PROCESS_FAILED: 21,
  INTERNAL_ERROR: 30,
} as const

export const DEFAULT_HOOK_TIMEOUT_MS = 30_000
export const DEFAULT_LOCK_TIMEOUT_MS = 15_000
export const DEFAULT_STALE_LOCK_TTL_SECONDS = 1_800

export const COMMAND_NAMES = {
  INIT: "init",
  LIST: "list",
  STATUS: "status",
  PATH: "path",
  SWITCH: "switch",
  NEW: "new",
  MV: "mv",
  DEL: "del",
  GONE: "gone",
  GET: "get",
  EXTRACT: "extract",
  USE: "use",
  EXEC: "exec",
  INVOKE: "invoke",
  COPY: "copy",
  LINK: "link",
  LOCK: "lock",
  UNLOCK: "unlock",
  CD: "cd",
} as const

export const WRITE_COMMANDS = new Set<string>([
  COMMAND_NAMES.INIT,
  COMMAND_NAMES.SWITCH,
  COMMAND_NAMES.NEW,
  COMMAND_NAMES.MV,
  COMMAND_NAMES.DEL,
  COMMAND_NAMES.GONE,
  COMMAND_NAMES.GET,
  COMMAND_NAMES.EXTRACT,
  COMMAND_NAMES.USE,
  COMMAND_NAMES.LOCK,
  COMMAND_NAMES.UNLOCK,
])
