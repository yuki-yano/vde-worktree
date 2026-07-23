# vde-worktree

`vde-worktree` is a safe Git worktree manager designed for both humans and coding agents.

It installs two command names:

- `vde-worktree`
- `vw` (alias)

Japanese documentation: [README.ja.md](./README.ja.md)

## Goals

- Keep managed worktrees under a configurable root (default: `.worktree/`)
- Provide idempotent branch-to-worktree operations
- Prevent accidental destructive actions by default
- Expose stable JSON output for automation
- Support hook-driven customization

## Requirements

- Rust 1.89 or newer when installing from source
- `fzf` (required for `cd`)
- `gh` (optional, for PR-based merge status)

Supported platforms are macOS arm64, macOS x86_64, and Linux x86_64.

## Install / Build

Install from crates.io:

```bash
cargo install vde-worktree --locked
```

Install from the current local checkout:

```bash
cargo install --path . --locked
```

Replace an existing local build:

```bash
cargo install --path . --locked --force
```

Cargo normally installs `vw` and `vde-worktree` into `~/.cargo/bin`.

```bash
~/.cargo/bin/vw --version
```

If `vw` is not found, add `~/.cargo/bin` to `PATH` and refresh the shell command cache.

Local build:

```bash
cargo build --locked
```

Validate locally:

```bash
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

The installed binaries have no JavaScript runtime dependency.

## Quick Start

```bash
vw init
vw switch feature/foo
cd "$(vw cd)"
```

`vw cd` prints the selected worktree path. It cannot change the parent shell directory by itself.

## Shell Completion

Generate from command:

```bash
vw completion zsh
vw completion fish
```

Install to default locations:

```bash
vw completion zsh --install
vw completion fish --install
```

Install to custom file path:

```bash
vw completion zsh --install --path ~/.zsh/completions/_vw
vw completion fish --install --path ~/.config/fish/completions/vw.fish
```

For zsh, ensure completion path is loaded:

```bash
fpath=(~/.zsh/completions $fpath)
autoload -Uz compinit && compinit
```

The generated scripts obtain dynamic candidates directly from the Rust binary.

`--install` atomically replaces the completion file through a same-filesystem transaction directory. A directory-sync failure restores the previous file before returning an error.

## Managed Directories

After `vw init`, the tool manages:

- `<worktreeRoot>/` (managed worktree root; default: `.worktree/`)
- `.vde/worktree/hooks/`
- `.vde/worktree/logs/`
- `.vde/worktree/locks/`
- `.vde/worktree/state/`

`init` updates `info/exclude` in the Git common directory (normally `.git/info/exclude`) idempotently.

## Global Behavior

- Most write commands require prior `init`.
- Worktree lifecycle commands that mutate Git refs, worktrees, or repository-local state are protected by an internal repository lock. `exec`, `invoke`, `copy`, and `link` do not acquire it.
- `--json` prints exactly one JSON object to stdout.
- Logs and warnings are written to stderr.
- Non-TTY unsafe overrides require `--allow-unsafe`.

## Global Options

- `--json`: machine-readable single-object output
- `--verbose`: verbose logging
- `--hooks` / `--no-hooks`: enable or disable hooks (disabling requires `--allow-unsafe`)
- `--gh` / `--no-gh`: enable or disable `gh`-based PR status checks
- `--full-path`: disable path truncation in `list`
- `--allow-unsafe`: explicit unsafe override
- `--strict-post-hooks`: treat post-hook failures as errors instead of warnings
- `--hook-timeout-ms <ms>`: hook timeout override
- `--lock-timeout-ms <ms>`: repository lock timeout override
- `--prompt <text>`: override the `cd` fzf prompt
- `--fzf-arg <arg>`: append a non-reserved fzf argument; may be repeated

## Command Guide

### `init`

```bash
vw init
```

What it does:

- Creates `<worktreeRoot>/` and `.vde/worktree/*`
- Appends managed entries to `.git/info/exclude`
- Creates default hook templates

### `list`

```bash
vw list
vw list --json
vw list --no-gh
vw list --full-path
vw list --json --no-gh --monitor
```

What it does:

- Lists all worktrees from Git porcelain output
- Includes metadata such as branch, path, dirty, lock, merged, PR status, and upstream status
- JSON metadata includes `pr.status` and `pr.url` for each non-base branch
- In table output, long `path` values are truncated with `…` to fit terminal width by default
- Use `--full-path` to disable path truncation in table output
- With `--no-gh`, skips PR status checks (`pr.status` becomes `unknown`, `merged.byPR` becomes `null`)
- `--monitor` is an internal, machine-only snapshot profile for monitor integrations. It requires `--json --no-gh`, cannot be combined with `--gh`, skips upstream probes, returns unknown (`null`) upstream fields, and does not persist lifecycle observations.
- In interactive terminal, uses Catppuccin-style ANSI colors
- Disables all ANSI colors when `NO_COLOR` is set or stdout is not a TTY

### `status`

```bash
vw status
vw status feature/foo
vw status --json
```

What it does:

- Shows one worktree state
- Without branch argument, resolves current worktree from current `cwd`

### `path`

```bash
vw path feature/foo
vw path feature/foo --json
```

What it does:

- Resolves and returns the absolute worktree path for the target branch

### `new`

```bash
vw new
vw new feature/foo
```

What it does:

- Creates a new branch + worktree under configured managed worktree root (`paths.worktreeRoot`)
- Without argument, generates `wip-xxxxxx`

### `switch`

```bash
vw switch feature/foo
```

What it does:

- Idempotent branch entrypoint
- Reuses existing worktree if present, otherwise creates one

### `mv`

```bash
vw mv feature/new-name
```

What it does:

- Renames current non-primary worktree branch and moves its directory
- Requires branch checkout (not detached HEAD)

### `del`

```bash
vw del
vw del feature/foo
vw del feature/foo --force-unmerged --allow-unpushed --allow-unsafe
```

What it does:

- Removes worktree and branch safely
- By default, rejects dirty, locked, unmerged/unknown, or unpushed/unknown states

Useful force flags:

- `--force-dirty`
- `--allow-unpushed`
- `--force-unmerged`
- `--force-locked`
- `--force` (enables all force flags)

### `gone`

```bash
vw gone
vw gone --apply
vw gone --json
```

What it does:

- Bulk cleanup candidate finder/remover
- Default mode is dry-run
- `--apply` actually deletes eligible branches/worktrees

### `adopt`

```bash
vw adopt
vw adopt --json
vw adopt --apply
```

What it does:

- Finds unmanaged non-primary worktrees and plans moves into the managed worktree root
- Default mode is dry-run; `--apply` runs `git worktree move`
- Reports skipped entries with reasons (`detached`, `locked`, `target_exists`, `target_conflict`)

### `get`

```bash
vw get origin/feature/foo
```

What it does:

- Fetches remote branch
- Creates tracking local branch when missing
- Creates/reuses local worktree

### `extract`

```bash
vw extract --current
vw extract --current --stash
```

What it does:

- Extracts current primary worktree branch into managed worktree root (`paths.worktreeRoot`)
- Switches primary worktree back to base branch
- `--stash` allows extraction when primary is dirty

Current limitation:

- Implementation currently supports primary worktree extraction flow.

### `absorb`

```bash
vw absorb feature/foo --allow-agent --allow-unsafe
vw absorb feature/foo --from feature/foo --keep-stash --allow-agent --allow-unsafe
```

What it does:

- Moves changes from non-primary worktree to primary worktree, including uncommitted files
- Stashes source worktree changes, checks out branch in primary, then applies stash
- `--from` accepts vw-managed worktree name only (`<worktreeRoot>/...` path prefix is rejected)

Safety:

- Rejects dirty primary worktree
- In non-TTY mode, requires `--allow-agent` and `--allow-unsafe`
- `--keep-stash` keeps the stash entry after apply for rollback/debugging

### `unabsorb`

```bash
vw unabsorb feature/foo --allow-agent --allow-unsafe
vw unabsorb feature/foo --to feature/foo --keep-stash --allow-agent --allow-unsafe
```

What it does:

- Pushes changes from primary worktree to non-primary worktree, including uncommitted files
- Stashes primary worktree changes, applies stash in target worktree
- `--to` accepts vw-managed worktree name only (`<worktreeRoot>/...` path prefix is rejected)

Safety:

- Requires primary worktree to be on target branch
- Rejects clean primary worktree
- Rejects dirty target worktree
- In non-TTY mode, requires `--allow-agent` and `--allow-unsafe`
- `--keep-stash` keeps the stash entry after apply for rollback/debugging

### `use`

```bash
vw use feature/foo
vw use feature/foo --allow-shared
vw use feature/foo --allow-agent --allow-unsafe
```

What it does:

- Checks out the target branch in the primary worktree
- Intended for human workflows where primary context must be fixed

Safety:

- Rejects dirty primary worktree
- If target branch is attached by another worktree, requires `--allow-shared` and prints a warning
- In non-TTY mode, requires `--allow-agent` and `--allow-unsafe`

### `exec`

```bash
vw exec feature/foo -- cargo test
vw exec feature/foo --json -- cargo test
```

What it does:

- Executes command inside the target branch worktree path
- Does not use shell expansion
- In human mode, inherits the child process stdin, stdout, and stderr
- In JSON mode, captures child stdout and stderr as `data.childStdout` and `data.childStderr`

Exit behavior:

- Child success => `0`
- Child failure => `21` (`CHILD_PROCESS_FAILED` in JSON mode)

### `invoke`

```bash
vw invoke post-switch
vw invoke pre-new -- --arg1 --arg2
```

What it does:

- Manually invokes `pre-*` / `post-*` hook scripts
- Useful for debugging hook behavior

## Hook Contract

Hooks are executable files at `.vde/worktree/hooks/pre-<action>` or `.vde/worktree/hooks/post-<action>`.

A pre-hook runs with the existing source worktree or repository root as its cwd. A post-hook runs with the applied target worktree as its cwd.

Common environment variables:

- `WT_REPO_ROOT`: repository root.
- `WT_ACTION`: action name such as `new` or `switch`.
- `WT_BRANCH`: target branch fixed during preflight, or an empty string when absent.
- `WT_WORKTREE_PATH`: target path fixed during preflight, or an empty string when absent.
- `WT_IS_TTY`: `1` for a TTY invocation, otherwise `0`.
- `WT_TOOL`: `vde-worktree`.

`mv` also provides `WT_OLD_BRANCH` and `WT_NEW_BRANCH`. `absorb` and `unabsorb` also provide `WT_SOURCE` and `WT_TARGET`.

Execution logs are stored under `.vde/worktree/logs/` with `hook`, `phase`, `start`, `end`, `exitCode`, `timedOut`, and `stderr` fields.

A pre-hook failure stops the operation. A post-hook failure is a warning by default and becomes an error with `--strict-post-hooks`. Use `--hook-timeout-ms` to set the timeout.

### `copy`

```bash
vw copy .envrc .claude/settings.local.json
```

What it does:

- Copies repo-relative files/dirs from repo root into target worktree
- Primarily intended for hook usage with `WT_WORKTREE_PATH`
- Stages the complete path batch in a private random transaction directory before changing the target
- Rolls back every earlier path if any later path fails during commit

### `link`

```bash
vw link .envrc
```

What it does:

- Creates symlink in target worktree pointing to repo-root file
- Creates only a relative symlink that resolves to the repository-root source
- Returns an error when symlink creation fails and never switches implicitly to `copy`

### `lock` / `unlock`

```bash
vw lock feature/foo --owner codex --reason "agent in progress"
vw unlock feature/foo --owner codex
vw unlock feature/foo --force
```

What they do:

- `lock` writes lock metadata under `.vde/worktree/locks/`
- `unlock` clears lock, enforcing owner match unless `--force`

### `cd`

```bash
cd "$(vw cd)"
```

What it does:

- Interactive worktree picker via `fzf`
- Picker list shows worktree branch names with minimal states (dirty/merged/lock)
- Preview pane shows path and worktree states (dirty/locked/merged/upstream)
- Picker and preview use Catppuccin-style ANSI colors in interactive terminal
- Prints selected absolute path to stdout

### `completion`

```bash
vw completion zsh
vw completion fish
vw completion zsh --install
```

What it does:

- Prints completion script for zsh/fish
- With `--install`, atomically replaces the completion file at the shell default path or `--path`
- Obtains dynamic branch, remote branch, hook, and managed-worktree candidates from the Rust binary
- Restores the previous completion if the post-rename directory sync fails

## Merge Status (Local + PR)

Each worktree reports:

- `merged.byAncestry`: local ancestry check (`git merge-base --is-ancestor <branch> <baseBranch>`)
- `merged.byPR`: PR-based merged check via GitHub CLI
- `merged.overall`: final decision
- `pr.status`: PR state (`none` / `open` / `merged` / `closed_unmerged` / `unknown`)
- `pr.url`: latest PR URL for the branch (`null` when unavailable)

Overall policy:

- `byPR === true` => `overall = true` (includes squash/rebase merges)
- `byAncestry === false` => `overall = false`
- when `byAncestry === true`, require divergence evidence before treating as merged
  - lifecycle evidence from `.vde/worktree/state/branches/*.json`
  - reflog fallback (`git reflog`) when lifecycle evidence is missing
- if divergence evidence is contained in `baseBranch`, `overall = true`
- `byPR === false` or explicit lifecycle "not merged" evidence => `overall = false`
- otherwise `overall = null`

`byPR` becomes `null` and `pr.status` becomes `unknown` when PR lookup is unavailable (for example: `gh` missing, auth missing, API error, `github.enabled=false` in config.yml, or `--no-gh`).

## JSON Contract

With `--json`, stdout always emits exactly one schema version 2 JSON object.

Common success fields:

- `schemaVersion`
- `command`
- `status`
- `repoRoot`
- `data`
- `error`

Error shape:

- `status: "error"`
- `data` is normally `null`; commands with partial success retain their completed result
- `error.code`
- `error.message`
- `error.details`

## Configuration (`config.yml`)

Configuration is loaded from:

- `$XDG_CONFIG_HOME/vde/worktree/config.yml` (fallback: `~/.config/vde/worktree/config.yml`)
- `.vde/worktree/config.yml` discovered from `cwd` to the local Git boundary (`.git`)
- `<repoRoot>/.vde/worktree/config.yml` (always considered, including linked worktree execution)

Supported keys (examples):

```yaml
paths:
  worktreeRoot: .worktree
git:
  baseBranch: null
  baseRemote: origin
github:
  enabled: true
hooks:
  enabled: true
  timeoutMs: 30000
locks:
  timeoutMs: 15000
  staleLockTTLSeconds: 1800
list:
  table:
    columns: [branch, dirty, merged, pr, locked, ahead, behind, path]
selector:
  cd:
    prompt: "worktree> "
    surface: auto # auto | inline | tmux-popup
    tmuxPopupOpts: "80%,70%"
```

Notes:

- `paths.worktreeRoot` accepts repo-relative and absolute paths
- Paths under `.git` are allowed in regular repositories (for example: `.git/worktrees`)
- In a submodule, `.git` is a file; use the default `.worktree` or another non-`.git` path instead
- If `paths.worktreeRoot` points to an existing file, config loading fails

## Current Scope

- The first Rust release does not include a built-in TUI.
- The `fzf` picker, preview, and tmux popup provide the graphical workflow.
