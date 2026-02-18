import { DEFAULT_HOOK_TIMEOUT_MS, DEFAULT_LOCK_TIMEOUT_MS, DEFAULT_STALE_LOCK_TTL_SECONDS } from "../core/constants"

export const LIST_TABLE_COLUMNS = ["branch", "dirty", "merged", "pr", "locked", "ahead", "behind", "path"] as const

export const LIST_PATH_TRUNCATE_VALUES = ["auto", "never"] as const
export const SELECTOR_CD_SURFACE_VALUES = ["auto", "inline", "tmux-popup"] as const

export type ListTableColumn = (typeof LIST_TABLE_COLUMNS)[number]
export type ListPathTruncate = (typeof LIST_PATH_TRUNCATE_VALUES)[number]
export type SelectorCdSurface = (typeof SELECTOR_CD_SURFACE_VALUES)[number]

export type ResolvedConfig = {
  readonly paths: {
    readonly worktreeRoot: string
  }
  readonly git: {
    readonly baseBranch: string | null
    readonly baseRemote: string
  }
  readonly github: {
    readonly enabled: boolean
  }
  readonly hooks: {
    readonly enabled: boolean
    readonly timeoutMs: number
  }
  readonly locks: {
    readonly timeoutMs: number
    readonly staleLockTTLSeconds: number
  }
  readonly list: {
    readonly table: {
      readonly columns: ReadonlyArray<ListTableColumn>
      readonly path: {
        readonly truncate: ListPathTruncate
        readonly minWidth: number
      }
    }
  }
  readonly selector: {
    readonly cd: {
      readonly prompt: string
      readonly surface: SelectorCdSurface
      readonly tmuxPopupOpts: string
      readonly fzf: {
        readonly extraArgs: ReadonlyArray<string>
      }
    }
  }
}

export type DeepPartial<T> = {
  -readonly [K in keyof T]?: T[K] extends ReadonlyArray<infer U>
    ? ReadonlyArray<U>
    : T[K] extends object
      ? DeepPartial<T[K]>
      : T[K]
}

export type PartialConfig = DeepPartial<ResolvedConfig>

export const DEFAULT_CONFIG: ResolvedConfig = {
  paths: {
    worktreeRoot: ".worktree",
  },
  git: {
    baseBranch: null,
    baseRemote: "origin",
  },
  github: {
    enabled: true,
  },
  hooks: {
    enabled: true,
    timeoutMs: DEFAULT_HOOK_TIMEOUT_MS,
  },
  locks: {
    timeoutMs: DEFAULT_LOCK_TIMEOUT_MS,
    staleLockTTLSeconds: DEFAULT_STALE_LOCK_TTL_SECONDS,
  },
  list: {
    table: {
      columns: [...LIST_TABLE_COLUMNS],
      path: {
        truncate: "auto",
        minWidth: 12,
      },
    },
  },
  selector: {
    cd: {
      prompt: "worktree> ",
      surface: "auto",
      tmuxPopupOpts: "80%,70%",
      fzf: {
        extraArgs: [],
      },
    },
  },
}
