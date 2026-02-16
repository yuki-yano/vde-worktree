function __vw_worktree_branches
  command git rev-parse --is-inside-work-tree >/dev/null 2>/dev/null; or return 0
  command git worktree list --porcelain 2>/dev/null \
    | string match -r '^branch refs/heads/.+$' \
    | string replace 'branch refs/heads/' '' \
    | sort -u
end

function __vw_default_branch
  command git rev-parse --is-inside-work-tree >/dev/null 2>/dev/null; or return 0

  set -l configured (command git config --get vde-worktree.baseBranch 2>/dev/null)
  if test -n "$configured"
    echo $configured
    return
  end

  command git show-ref --verify --quiet refs/heads/main >/dev/null 2>/dev/null
  and begin
    echo main
    return
  end

  command git show-ref --verify --quiet refs/heads/master >/dev/null 2>/dev/null
  and begin
    echo master
    return
  end
end

function __vw_current_bin
  set -l tokens (commandline -opc)
  if test (count $tokens) -ge 1
    set -l candidate $tokens[1]
    if command -sq $candidate
      echo $candidate
      return
    end
  end

  if command -sq vw
    echo vw
    return
  end
  if command -sq vde-worktree
    echo vde-worktree
    return
  end
end

function __vw_worktree_candidates_with_meta
  command git rev-parse --is-inside-work-tree >/dev/null 2>/dev/null; or return 0
  set -l vw_bin (__vw_current_bin)
  test -n "$vw_bin"; or return 0

  command $vw_bin list --json 2>/dev/null | command node -e '
const fs = require("fs")
const home = process.env.HOME || ""
const toDisplayPath = (path) => {
  if (typeof path !== "string" || path.length === 0) return ""
  if (home.length === 0) return path
  if (path === home) return "~"
  if (path.startsWith(`${home}/`)) return `~${path.slice(home.length)}`
  return path
}
const toFlag = (value) => {
  if (value === true) return "yes"
  if (value === false) return "no"
  return "unknown"
}
const toPrStatus = (value) => {
  if (typeof value !== "string" || value.length === 0) return "n/a"
  return value
}
let payload
try {
  payload = JSON.parse(fs.readFileSync(0, "utf8"))
} catch {
  process.exit(0)
}
const worktrees = Array.isArray(payload.worktrees) ? payload.worktrees : []
for (const worktree of worktrees) {
  if (typeof worktree?.branch !== "string" || worktree.branch.length === 0) continue
  const merged = toFlag(worktree?.merged?.overall)
  const pr = toPrStatus(worktree?.pr?.status)
  const dirty = worktree?.dirty === true ? "yes" : "no"
  const locked = worktree?.locked?.value === true ? "yes" : "no"
  const path = toDisplayPath(worktree?.path)
  const summary = `merged=${merged} pr=${pr} dirty=${dirty} locked=${locked}${path ? ` path=${path}` : ""}`
  const sanitized = summary.replace(/[\t\r\n]+/g, " ").trim()
  process.stdout.write(`${worktree.branch}\t${sanitized}\n`)
}
' 2>/dev/null
end

function __vw_managed_worktree_names_with_meta
  command git rev-parse --is-inside-work-tree >/dev/null 2>/dev/null; or return 0
  set -l vw_bin (__vw_current_bin)
  test -n "$vw_bin"; or return 0

  command $vw_bin list --json 2>/dev/null | command node -e '
const fs = require("fs")
const path = require("path")
const toFlag = (value) => {
  if (value === true) return "yes"
  if (value === false) return "no"
  return "unknown"
}
const toPrStatus = (value) => {
  if (typeof value !== "string" || value.length === 0) return "n/a"
  return value
}
let payload
try {
  payload = JSON.parse(fs.readFileSync(0, "utf8"))
} catch {
  process.exit(0)
}
const repoRoot = typeof payload?.repoRoot === "string" ? payload.repoRoot : ""
if (repoRoot.length === 0) process.exit(0)
const worktreeRoot = path.join(repoRoot, ".worktree")
const worktrees = Array.isArray(payload.worktrees) ? payload.worktrees : []
for (const worktree of worktrees) {
  if (typeof worktree?.path !== "string" || worktree.path.length === 0) continue
  const rel = path.relative(worktreeRoot, worktree.path)
  if (!rel || rel === "." || rel === ".." || rel.startsWith(`..${path.sep}`)) continue
  const name = rel.split(path.sep).join("/")
  const branch = typeof worktree?.branch === "string" && worktree.branch.length > 0 ? worktree.branch : "(detached)"
  const merged = toFlag(worktree?.merged?.overall)
  const pr = toPrStatus(worktree?.pr?.status)
  const dirty = worktree?.dirty === true ? "yes" : "no"
  const locked = worktree?.locked?.value === true ? "yes" : "no"
  const summary = `branch=${branch} merged=${merged} pr=${pr} dirty=${dirty} locked=${locked}`
  const sanitized = summary.replace(/[\t\r\n]+/g, " ").trim()
  process.stdout.write(`${name}\t${sanitized}\n`)
}
' 2>/dev/null
end

function __vw_use_candidates_with_meta
  begin
    __vw_worktree_branches
    __vw_default_branch
  end | sort -u
end

function __vw_remote_branches
  command git rev-parse --is-inside-work-tree >/dev/null 2>/dev/null; or return 0
  command git for-each-ref --format='%(refname:short)' refs/remotes 2>/dev/null \
    | string match -rv '.*/HEAD$' \
    | sort -u
end

function __vw_hook_names
  command git rev-parse --is-inside-work-tree >/dev/null 2>/dev/null; or return 0
  set -l repo_root (command git rev-parse --show-toplevel 2>/dev/null); or return 0
  if test -d "$repo_root/.vde/worktree/hooks"
    command ls -1 "$repo_root/.vde/worktree/hooks" 2>/dev/null | string match -r '^(pre|post)-' | sort -u
  end
end

set -l __vw_commands init list status path new switch mv del gone get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help

for __vw_bin in vw vde-worktree
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a init -d "Initialize directories, hooks, and managed exclude entries"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a list -d "List worktrees with status metadata"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a status -d "Show a single worktree status"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a path -d "Print absolute worktree path for branch"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a new -d "Create branch + worktree under .worktree"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a switch -d "Idempotent branch entrypoint"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a mv -d "Rename current non-primary worktree branch"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a del -d "Delete worktree + branch with safety checks"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a gone -d "Bulk cleanup by safety-filtered candidate selection"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a get -d "Fetch remote branch and attach worktree"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a extract -d "Extract current primary branch into .worktree"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a absorb -d "Bring non-primary worktree changes into primary worktree"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a unabsorb -d "Push primary worktree changes into non-primary worktree"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a use -d "Checkout target branch in primary worktree"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a exec -d "Run command in target branch worktree"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a invoke -d "Manually run hook script"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a copy -d "Copy repo-root files/dirs to target worktree"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a link -d "Create symlink from target worktree to repo-root file"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a lock -d "Create or update lock metadata"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a unlock -d "Remove lock metadata"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a cd -d "Interactive fzf picker"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a completion -d "Print or install shell completion scripts"
  complete -c $__vw_bin -f -n "not __fish_seen_subcommand_from $__vw_commands" -a help -d "Show help"

  complete -c $__vw_bin -l json -d "Output machine-readable JSON"
  complete -c $__vw_bin -l verbose -d "Enable verbose logs"
  complete -c $__vw_bin -l no-hooks -d "Disable hooks for this run (requires --allow-unsafe)"
  complete -c $__vw_bin -l no-gh -d "Disable GitHub CLI based PR status checks for this run"
  complete -c $__vw_bin -l full-path -d "Disable list table path truncation"
  complete -c $__vw_bin -l allow-unsafe -d "Explicit unsafe override in non-TTY mode"
  complete -c $__vw_bin -l strict-post-hooks -d "Fail when post hooks fail"
  complete -c $__vw_bin -l hook-timeout-ms -r -d "Override hook timeout"
  complete -c $__vw_bin -l lock-timeout-ms -r -d "Override lock timeout"
  complete -c $__vw_bin -s h -l help -d "Show help"
  complete -c $__vw_bin -s v -l version -d "Show version"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from status" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from path" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from switch" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from del" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from list" -l full-path -d "Disable list table path truncation"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from get" -a "(__vw_remote_branches)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from absorb" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from unabsorb" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from use" -a "(__vw_use_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from exec" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from invoke" -a "(__vw_hook_names)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from lock" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from unlock" -a "(__vw_worktree_candidates_with_meta)"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from help" -a "$__vw_commands"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from del" -l force-dirty -d "Allow dirty worktree for del"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from del" -l allow-unpushed -d "Allow unpushed commits for del"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from del" -l force-unmerged -d "Allow unmerged worktree for del"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from del" -l force-locked -d "Allow deleting locked worktree"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from del" -l force -d "Enable all del force flags"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from gone" -l apply -d "Apply deletion"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from gone" -l dry-run -d "Dry-run mode"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from extract" -l current -d "Extract current worktree branch"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from extract" -l from -r -d "Path used by extract --from"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from extract" -l stash -d "Allow stash when dirty"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from absorb" -l from -r -a "(__vw_managed_worktree_names_with_meta)" -d "Source managed worktree name"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from absorb" -l keep-stash -d "Keep stash entry after absorb"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from absorb" -l allow-agent -d "Allow non-TTY execution for absorb"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from absorb" -l allow-unsafe -d "Allow unsafe behavior in non-TTY mode"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from unabsorb" -l to -r -a "(__vw_managed_worktree_names_with_meta)" -d "Target managed worktree name"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from unabsorb" -l keep-stash -d "Keep stash entry after unabsorb"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from unabsorb" -l allow-agent -d "Allow non-TTY execution for unabsorb"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from unabsorb" -l allow-unsafe -d "Allow unsafe behavior in non-TTY mode"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from use" -l allow-agent -d "Allow non-TTY execution for use"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from use" -l allow-shared -d "Allow checkout when branch is attached by another worktree"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from use" -l allow-unsafe -d "Allow unsafe behavior in non-TTY mode"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from link" -l no-fallback -d "Disable copy fallback when symlink fails"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from lock" -l owner -r -d "Lock owner"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from lock" -l reason -r -d "Lock reason"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from unlock" -l owner -r -d "Unlock owner"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from unlock" -l force -d "Force unlock"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from cd" -l prompt -r -d "Custom fzf prompt"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from cd" -l fzf-arg -r -d "Extra argument passed to fzf"

  complete -c $__vw_bin -n "__fish_seen_subcommand_from completion" -a "zsh fish" -d "Shell name"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from completion" -l install -d "Install completion file"
  complete -c $__vw_bin -n "__fish_seen_subcommand_from completion" -l path -r -d "Install destination file path"
end
