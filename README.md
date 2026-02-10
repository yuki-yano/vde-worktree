# vde-worktree

`vde-worktree` is a safe Git worktree manager designed for both humans and coding agents.

It installs two command names:
- `vde-worktree`
- `vw` (alias)

Japanese documentation: [README.ja.md](./README.ja.md)

## Goals

- Keep all worktrees in one repo-local location: `.worktree/`
- Provide idempotent branch-to-worktree operations
- Prevent accidental destructive actions by default
- Expose stable JSON output for automation
- Support hook-driven customization

## Requirements

- Node.js 22+
- pnpm 10+
- `fzf` (required for `cd`)
- `gh` (optional, for PR-based merge status)

## Install / Build

```bash
pnpm install
pnpm run build
```

Validate locally:

```bash
pnpm run ci
```

## Quick Start

```bash
vw init
vw switch feature/foo
cd "$(vw cd)"
```

## Managed Directories

After `vw init`, the tool manages:

- `.worktree/` (worktree roots)
- `.vde/worktree/hooks/`
- `.vde/worktree/logs/`
- `.vde/worktree/locks/`
- `.vde/worktree/state/`

`init` updates `.git/info/exclude` idempotently.

## Global Behavior

- Most write commands require prior `init`.
- Write commands are protected by an internal repository lock.
- `--json` prints exactly one JSON object to stdout.
- Logs and warnings are written to stderr.
- Non-TTY unsafe overrides require `--allow-unsafe`.

## Global Options

- `--json`: machine-readable single-object output
- `--verbose`: verbose logging
- `--no-hooks`: disable hooks for this run (requires `--allow-unsafe`)
- `--allow-unsafe`: explicit unsafe override
- `--hook-timeout-ms <ms>`: hook timeout override
- `--lock-timeout-ms <ms>`: repository lock timeout override

## Command Guide

### `init`

```bash
vw init
```

What it does:
- Creates `.worktree/` and `.vde/worktree/*`
- Appends managed entries to `.git/info/exclude`
- Creates default hook templates

### `list`

```bash
vw list
vw list --json
```

What it does:
- Lists all worktrees from Git porcelain output
- Includes metadata such as branch, path, dirty, lock, merged, and upstream status

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
- Creates a new branch + worktree under `.worktree/`
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
- Extracts current primary worktree branch into `.worktree/`
- Switches primary worktree back to base branch
- `--stash` allows extraction when primary is dirty

Current limitation:
- Implementation currently supports primary worktree extraction flow.

### `use`

```bash
vw use feature/foo
vw use feature/foo --allow-agent --allow-unsafe
```

What it does:
- Checks out the target branch in the primary worktree
- Intended for human workflows where primary context must be fixed

Safety:
- Rejects dirty primary worktree
- In non-TTY mode, requires `--allow-agent` and `--allow-unsafe`

### `exec`

```bash
vw exec feature/foo -- pnpm test
vw exec feature/foo --json -- pnpm test
```

What it does:
- Executes command inside the target branch worktree path
- Does not use shell expansion

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

### `copy`

```bash
vw copy .envrc .claude/settings.local.json
```

What it does:
- Copies repo-relative files/dirs from repo root into target worktree
- Primarily intended for hook usage with `WT_WORKTREE_PATH`

### `link`

```bash
vw link .envrc
vw link .envrc --no-fallback
```

What it does:
- Creates symlink in target worktree pointing to repo-root file
- On Windows, can fallback to copy unless `--no-fallback`

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
- Prints selected absolute path to stdout

## Merge Status (Local + PR)

Each worktree reports:

- `merged.byAncestry`: local ancestry check (`git merge-base --is-ancestor <branch> <baseBranch>`)
- `merged.byPR`: PR-based merged check via GitHub CLI
- `merged.overall`: final decision

Overall policy:

- `byPR === true` => `overall = true`
- `byPR === false` => `overall = false`
- `byPR === null` => fallback to `byAncestry`

`byPR` becomes `null` when PR lookup is unavailable (for example: `gh` missing, auth missing, API error, or `vde-worktree.enableGh=false`).

## JSON Contract

With `--json`, stdout always emits exactly one JSON object.

Common success fields:
- `schemaVersion`
- `command`
- `status`
- `repoRoot`

Error shape:
- `status: "error"`
- `code`
- `message`
- `details`

## Configuration Keys

Configured via `git config`:

- `vde-worktree.baseBranch`
- `vde-worktree.baseRemote`
- `vde-worktree.enableGh`
- `vde-worktree.hooksEnabled`
- `vde-worktree.hookTimeoutMs`
- `vde-worktree.lockTimeoutMs`
- `vde-worktree.staleLockTTLSeconds`

## Current Scope

- Ink-based `tui` is not implemented yet.
