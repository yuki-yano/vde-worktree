# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_vw_global_optspecs
    string join \n C/directory= worktree= json dry-run verbose v/version hooks no-hooks gh no-gh full-path allow-unsafe strict-post-hooks hook-timeout-ms= lock-timeout-ms= prompt= fzf-arg= h/help
end

function __fish_vw_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_vw_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_vw_using_subcommand
    set -l cmd (__fish_vw_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c vw -n "__fish_vw_needs_command" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_needs_command" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_needs_command" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_needs_command" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_needs_command" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_needs_command" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_needs_command" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_needs_command" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_needs_command" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_needs_command" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_needs_command" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_needs_command" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_needs_command" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_needs_command" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_needs_command" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_needs_command" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_needs_command" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_needs_command" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_needs_command" -f -a "init" -d 'Initialize repository-local vde-worktree state'
complete -c vw -n "__fish_vw_needs_command" -f -a "list" -d 'List worktrees and status metadata'
complete -c vw -n "__fish_vw_needs_command" -f -a "status" -d 'Show a single worktree status'
complete -c vw -n "__fish_vw_needs_command" -f -a "path" -d 'Print the absolute path for a branch worktree'
complete -c vw -n "__fish_vw_needs_command" -f -a "switch" -d 'Reuse or create a worktree for a branch'
complete -c vw -n "__fish_vw_needs_command" -f -a "new" -d 'Create a branch and its worktree'
complete -c vw -n "__fish_vw_needs_command" -f -a "mv" -d 'Rename the current linked worktree branch'
complete -c vw -n "__fish_vw_needs_command" -f -a "del" -d 'Delete a linked worktree and branch'
complete -c vw -n "__fish_vw_needs_command" -f -a "gone" -d 'Find or delete stale merged worktrees'
complete -c vw -n "__fish_vw_needs_command" -f -a "adopt" -d 'Find or move unmanaged worktrees into the managed root'
complete -c vw -n "__fish_vw_needs_command" -f -a "get" -d 'Fetch and attach a remote branch'
complete -c vw -n "__fish_vw_needs_command" -f -a "extract" -d 'Extract the current primary branch into the managed root'
complete -c vw -n "__fish_vw_needs_command" -f -a "absorb" -d 'Transfer linked worktree changes into the primary worktree'
complete -c vw -n "__fish_vw_needs_command" -f -a "unabsorb" -d 'Transfer primary worktree changes into a linked worktree'
complete -c vw -n "__fish_vw_needs_command" -f -a "use" -d 'Check out a branch in the primary worktree'
complete -c vw -n "__fish_vw_needs_command" -f -a "exec" -d 'Run an argv command in a branch worktree'
complete -c vw -n "__fish_vw_needs_command" -f -a "invoke" -d 'Invoke a named hook'
complete -c vw -n "__fish_vw_needs_command" -f -a "copy" -d 'Copy repository-relative paths into the target worktree'
complete -c vw -n "__fish_vw_needs_command" -f -a "link" -d 'Link repository-relative paths into the target worktree'
complete -c vw -n "__fish_vw_needs_command" -f -a "lock" -d 'Protect a worktree with persistent lock metadata'
complete -c vw -n "__fish_vw_needs_command" -f -a "unlock" -d 'Remove persistent lock metadata'
complete -c vw -n "__fish_vw_needs_command" -f -a "cd" -d 'Select a worktree path interactively'
complete -c vw -n "__fish_vw_needs_command" -f -a "completion" -d 'Generate or install shell completions'
complete -c vw -n "__fish_vw_needs_command" -f -a "describe" -d 'Describe commands, arguments, effects, and the JSON output contract'
complete -c vw -n "__fish_vw_needs_command" -f -a "context" -d 'Show execution context, effective configuration and setting sources'
complete -c vw -n "__fish_vw_needs_command" -f -a "doctor" -d 'Diagnose repository setup, configuration and dependencies without changing state'
complete -c vw -n "__fish_vw_needs_command" -f -a "check" -d 'Inspect a lifecycle mutation supplied after -- without applying it'
complete -c vw -n "__fish_vw_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c vw -n "__fish_vw_using_subcommand init" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand init" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand init" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand init" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand init" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand init" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand init" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand init" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand init" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand init" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand init" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand init" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand init" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand init" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand init" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand init" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand init" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand init" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand list" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand list" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand list" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand list" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand list" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand list" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand list" -l monitor -d 'Emit a lightweight internal snapshot for monitor integrations'
complete -c vw -n "__fish_vw_using_subcommand list" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand list" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand list" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand list" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand list" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand list" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand list" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand list" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand list" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand list" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand list" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand list" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand status" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand status" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand status" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand status" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand status" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand status" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand status" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand status" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand status" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand status" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand status" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand status" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand status" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand status" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand status" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand status" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand status" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand status" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand path" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand path" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand path" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand path" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand path" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand path" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand path" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand path" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand path" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand path" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand path" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand path" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand path" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand path" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand path" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand path" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand path" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand path" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand switch" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand switch" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand switch" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand switch" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand switch" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand switch" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand switch" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand switch" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand switch" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand switch" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand switch" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand switch" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand switch" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand switch" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand switch" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand switch" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand switch" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand switch" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand new" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand new" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand new" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand new" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand new" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand new" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand new" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand new" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand new" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand new" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand new" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand new" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand new" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand new" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand new" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand new" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand new" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand new" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand mv" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand mv" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand mv" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand mv" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand mv" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand mv" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand mv" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand mv" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand mv" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand mv" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand mv" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand mv" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand mv" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand mv" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand mv" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand mv" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand mv" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand mv" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand del" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand del" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand del" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand del" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand del" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand del" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand del" -l force -d 'Enable every deletion override; non-interactive use requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand del" -l force-dirty -d 'Allow discarding dirty worktree files; non-interactive use requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand del" -l allow-unpushed -d 'Allow commits ahead of upstream or unknown upstream state; non-interactive use requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand del" -l force-unmerged -d 'Allow deleting work not known to be merged; non-interactive use requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand del" -l force-locked -d 'Allow deleting a protected worktree; non-interactive use requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand del" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand del" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand del" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand del" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand del" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand del" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand del" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand del" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand del" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand del" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand del" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand del" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand gone" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand gone" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand gone" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand gone" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand gone" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand gone" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand gone" -l apply -d 'Delete the eligible candidates (default: preview only)'
complete -c vw -n "__fish_vw_using_subcommand gone" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand gone" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand gone" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand gone" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand gone" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand gone" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand gone" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand gone" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand gone" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand gone" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand gone" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand gone" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand adopt" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand adopt" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand adopt" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand adopt" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand adopt" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand adopt" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand adopt" -l apply -d 'Move eligible external worktrees into the managed root (default: preview only)'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand adopt" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand adopt" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand get" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand get" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand get" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand get" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand get" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand get" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand get" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand get" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand get" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand get" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand get" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand get" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand get" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand get" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand get" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand get" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand get" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand get" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand extract" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand extract" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand extract" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand extract" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand extract" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand extract" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand extract" -l current -d 'Extract the current primary branch; required'
complete -c vw -n "__fish_vw_using_subcommand extract" -l stash -d 'Temporarily stash dirty tracked and untracked changes for transfer'
complete -c vw -n "__fish_vw_using_subcommand extract" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand extract" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand extract" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand extract" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand extract" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand extract" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand extract" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand extract" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand extract" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand extract" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand extract" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand extract" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l from -d 'Managed source worktree name when branch attachment is ambiguous' -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand absorb" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand absorb" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l keep-stash -d 'Retain the exact transfer stash after successful application'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l allow-agent -d 'Allow non-interactive transfer; also requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand absorb" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand absorb" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l to -d 'Managed target worktree name when branch attachment is ambiguous' -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l keep-stash -d 'Retain the exact transfer stash after successful application'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l allow-agent -d 'Allow non-interactive transfer; also requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand use" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand use" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand use" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand use" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand use" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand use" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand use" -l allow-agent -d 'Allow non-interactive checkout; also requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand use" -l allow-shared -d 'Allow the branch to remain attached to a linked worktree'
complete -c vw -n "__fish_vw_using_subcommand use" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand use" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand use" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand use" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand use" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand use" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand use" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand use" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand use" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand use" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand use" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand use" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand exec" -l timeout-ms -d 'Maximum child runtime in milliseconds, including captured stream draining' -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l max-output-bytes -d 'Retain at most this many raw bytes per JSON output stream (default: 1048576); drain the rest' -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l stdin -d 'Child stdin: null closes input; inherit passes the invoking process input through' -r -f -a "null\t''
inherit\t''"
complete -c vw -n "__fish_vw_using_subcommand exec" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand exec" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand exec" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand exec" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand exec" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand exec" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand exec" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand exec" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand exec" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand exec" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand exec" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand exec" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand exec" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand exec" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand invoke" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand invoke" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand invoke" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand invoke" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand invoke" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand invoke" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand invoke" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand invoke" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand invoke" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand copy" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand copy" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand copy" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand copy" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand copy" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand copy" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand copy" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand copy" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand copy" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand copy" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand copy" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand copy" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand copy" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand copy" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand copy" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand copy" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand copy" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand copy" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand link" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand link" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand link" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand link" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand link" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand link" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand link" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand link" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand link" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand link" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand link" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand link" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand link" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand link" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand link" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand link" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand link" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand link" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand lock" -l owner -d 'Lock owner (default: current user); use a unique session identifier for agents' -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l reason -d 'Reason for protecting the worktree' -r
complete -c vw -n "__fish_vw_using_subcommand lock" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand lock" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand lock" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand lock" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand lock" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand lock" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand lock" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand lock" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand lock" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand lock" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand lock" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand lock" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand lock" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand lock" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l owner -d 'Expected owner (default: current user)' -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand unlock" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand unlock" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l force -d 'Remove the lock regardless of owner or record validity'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand unlock" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand unlock" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand cd" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand cd" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand cd" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand cd" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand cd" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand cd" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand cd" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand cd" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand cd" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand cd" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand cd" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand cd" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand cd" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand cd" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand cd" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand cd" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand cd" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand cd" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand completion" -l path -d 'Installation path (default: the shell completion directory)' -r -F
complete -c vw -n "__fish_vw_using_subcommand completion" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand completion" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand completion" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand completion" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand completion" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand completion" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand completion" -l install -d 'Atomically install the generated script instead of printing it'
complete -c vw -n "__fish_vw_using_subcommand completion" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand completion" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand completion" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand completion" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand completion" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand completion" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand completion" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand completion" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand completion" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand completion" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand completion" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand completion" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand describe" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand describe" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand describe" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand describe" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand describe" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand describe" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand describe" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand describe" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand describe" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand describe" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand describe" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand describe" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand describe" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand describe" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand describe" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand describe" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand describe" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand describe" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand context" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand context" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand context" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand context" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand context" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand context" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand context" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand context" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand context" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand context" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand context" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand context" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand context" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand context" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand context" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand context" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand context" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand context" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand doctor" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand doctor" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand doctor" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand doctor" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand doctor" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand doctor" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand doctor" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand doctor" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand doctor" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand doctor" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand check" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vw -n "__fish_vw_using_subcommand check" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vw -n "__fish_vw_using_subcommand check" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vw -n "__fish_vw_using_subcommand check" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vw -n "__fish_vw_using_subcommand check" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vw -n "__fish_vw_using_subcommand check" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vw -n "__fish_vw_using_subcommand check" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vw -n "__fish_vw_using_subcommand check" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vw -n "__fish_vw_using_subcommand check" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vw -n "__fish_vw_using_subcommand check" -s v -l version -d 'Print the version'
complete -c vw -n "__fish_vw_using_subcommand check" -l hooks -d 'Enable automatic command hooks'
complete -c vw -n "__fish_vw_using_subcommand check" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vw -n "__fish_vw_using_subcommand check" -l gh -d 'Enable GitHub pull request lookup'
complete -c vw -n "__fish_vw_using_subcommand check" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vw -n "__fish_vw_using_subcommand check" -l full-path -d 'Show absolute paths in human list output'
complete -c vw -n "__fish_vw_using_subcommand check" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vw -n "__fish_vw_using_subcommand check" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vw -n "__fish_vw_using_subcommand check" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "init" -d 'Initialize repository-local vde-worktree state'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "list" -d 'List worktrees and status metadata'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "status" -d 'Show a single worktree status'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "path" -d 'Print the absolute path for a branch worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "switch" -d 'Reuse or create a worktree for a branch'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "new" -d 'Create a branch and its worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "mv" -d 'Rename the current linked worktree branch'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "del" -d 'Delete a linked worktree and branch'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "gone" -d 'Find or delete stale merged worktrees'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "adopt" -d 'Find or move unmanaged worktrees into the managed root'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "get" -d 'Fetch and attach a remote branch'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "extract" -d 'Extract the current primary branch into the managed root'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "absorb" -d 'Transfer linked worktree changes into the primary worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "unabsorb" -d 'Transfer primary worktree changes into a linked worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "use" -d 'Check out a branch in the primary worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "exec" -d 'Run an argv command in a branch worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "invoke" -d 'Invoke a named hook'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "copy" -d 'Copy repository-relative paths into the target worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "link" -d 'Link repository-relative paths into the target worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "lock" -d 'Protect a worktree with persistent lock metadata'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "unlock" -d 'Remove persistent lock metadata'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "cd" -d 'Select a worktree path interactively'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "completion" -d 'Generate or install shell completions'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "describe" -d 'Describe commands, arguments, effects, and the JSON output contract'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "context" -d 'Show execution context, effective configuration and setting sources'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "doctor" -d 'Diagnose repository setup, configuration and dependencies without changing state'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "check" -d 'Inspect a lifecycle mutation supplied after -- without applying it'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'

# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_vde_worktree_global_optspecs
    string join \n C/directory= worktree= json dry-run verbose v/version hooks no-hooks gh no-gh full-path allow-unsafe strict-post-hooks hook-timeout-ms= lock-timeout-ms= prompt= fzf-arg= h/help
end

function __fish_vde_worktree_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_vde_worktree_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_vde_worktree_using_subcommand
    set -l cmd (__fish_vde_worktree_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "init" -d 'Initialize repository-local vde-worktree state'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "list" -d 'List worktrees and status metadata'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "status" -d 'Show a single worktree status'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "path" -d 'Print the absolute path for a branch worktree'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "switch" -d 'Reuse or create a worktree for a branch'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "new" -d 'Create a branch and its worktree'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "mv" -d 'Rename the current linked worktree branch'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "del" -d 'Delete a linked worktree and branch'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "gone" -d 'Find or delete stale merged worktrees'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "adopt" -d 'Find or move unmanaged worktrees into the managed root'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "get" -d 'Fetch and attach a remote branch'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "extract" -d 'Extract the current primary branch into the managed root'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "absorb" -d 'Transfer linked worktree changes into the primary worktree'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "unabsorb" -d 'Transfer primary worktree changes into a linked worktree'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "use" -d 'Check out a branch in the primary worktree'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "exec" -d 'Run an argv command in a branch worktree'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "invoke" -d 'Invoke a named hook'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "copy" -d 'Copy repository-relative paths into the target worktree'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "link" -d 'Link repository-relative paths into the target worktree'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "lock" -d 'Protect a worktree with persistent lock metadata'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "unlock" -d 'Remove persistent lock metadata'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "cd" -d 'Select a worktree path interactively'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "completion" -d 'Generate or install shell completions'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "describe" -d 'Describe commands, arguments, effects, and the JSON output contract'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "context" -d 'Show execution context, effective configuration and setting sources'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "doctor" -d 'Diagnose repository setup, configuration and dependencies without changing state'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "check" -d 'Inspect a lifecycle mutation supplied after -- without applying it'
complete -c vde-worktree -n "__fish_vde_worktree_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand init" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l monitor -d 'Emit a lightweight internal snapshot for monitor integrations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand list" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand status" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand path" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand switch" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand new" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand mv" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l force -d 'Enable every deletion override; non-interactive use requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l force-dirty -d 'Allow discarding dirty worktree files; non-interactive use requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l allow-unpushed -d 'Allow commits ahead of upstream or unknown upstream state; non-interactive use requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l force-unmerged -d 'Allow deleting work not known to be merged; non-interactive use requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l force-locked -d 'Allow deleting a protected worktree; non-interactive use requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand del" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l apply -d 'Delete the eligible candidates (default: preview only)'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand gone" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l apply -d 'Move eligible external worktrees into the managed root (default: preview only)'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand adopt" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand get" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l current -d 'Extract the current primary branch; required'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l stash -d 'Temporarily stash dirty tracked and untracked changes for transfer'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand extract" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l from -d 'Managed source worktree name when branch attachment is ambiguous' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l keep-stash -d 'Retain the exact transfer stash after successful application'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l allow-agent -d 'Allow non-interactive transfer; also requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand absorb" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l to -d 'Managed target worktree name when branch attachment is ambiguous' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l keep-stash -d 'Retain the exact transfer stash after successful application'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l allow-agent -d 'Allow non-interactive transfer; also requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unabsorb" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l allow-agent -d 'Allow non-interactive checkout; also requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l allow-shared -d 'Allow the branch to remain attached to a linked worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand use" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l timeout-ms -d 'Maximum child runtime in milliseconds, including captured stream draining' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l max-output-bytes -d 'Retain at most this many raw bytes per JSON output stream (default: 1048576); drain the rest' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l stdin -d 'Child stdin: null closes input; inherit passes the invoking process input through' -r -f -a "null\t''
inherit\t''"
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand exec" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand invoke" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand copy" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand link" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l owner -d 'Lock owner (default: current user); use a unique session identifier for agents' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l reason -d 'Reason for protecting the worktree' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand lock" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l owner -d 'Expected owner (default: current user)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l force -d 'Remove the lock regardless of owner or record validity'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand unlock" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand cd" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l path -d 'Installation path (default: the shell completion directory)' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l install -d 'Atomically install the generated script instead of printing it'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand completion" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand describe" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand context" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand doctor" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -s C -l directory -d 'Resolve repository, config, hooks and relative paths from this directory' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l worktree -d 'Select a registered worktree by path for status, path, exec, copy or link' -r -F
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l hook-timeout-ms -d 'Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l lock-timeout-ms -d 'Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000)' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l prompt -d 'Override the interactive cd picker prompt' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l fzf-arg -d 'Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash' -r
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l json -d 'Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l dry-run -d 'Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l verbose -d 'Show resolved context and result diagnostics on stderr; repeat to include configuration'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -s v -l version -d 'Print the version'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l hooks -d 'Enable automatic command hooks'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l no-hooks -d 'Disable automatic hooks; requires --allow-unsafe'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l gh -d 'Enable GitHub pull request lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l no-gh -d 'Disable GitHub lookup and network requests made by that lookup'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l full-path -d 'Show absolute paths in human list output'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l allow-unsafe -d 'Acknowledge explicitly requested unsafe operations'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -l strict-post-hooks -d 'Return an error if a post-hook fails; retain the completed operation result'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand check" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "init" -d 'Initialize repository-local vde-worktree state'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "list" -d 'List worktrees and status metadata'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "status" -d 'Show a single worktree status'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "path" -d 'Print the absolute path for a branch worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "switch" -d 'Reuse or create a worktree for a branch'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "new" -d 'Create a branch and its worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "mv" -d 'Rename the current linked worktree branch'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "del" -d 'Delete a linked worktree and branch'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "gone" -d 'Find or delete stale merged worktrees'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "adopt" -d 'Find or move unmanaged worktrees into the managed root'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "get" -d 'Fetch and attach a remote branch'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "extract" -d 'Extract the current primary branch into the managed root'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "absorb" -d 'Transfer linked worktree changes into the primary worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "unabsorb" -d 'Transfer primary worktree changes into a linked worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "use" -d 'Check out a branch in the primary worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "exec" -d 'Run an argv command in a branch worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "invoke" -d 'Invoke a named hook'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "copy" -d 'Copy repository-relative paths into the target worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "link" -d 'Link repository-relative paths into the target worktree'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "lock" -d 'Protect a worktree with persistent lock metadata'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "unlock" -d 'Remove persistent lock metadata'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "cd" -d 'Select a worktree path interactively'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "completion" -d 'Generate or install shell completions'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "describe" -d 'Describe commands, arguments, effects, and the JSON output contract'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "context" -d 'Show execution context, effective configuration and setting sources'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "doctor" -d 'Diagnose repository setup, configuration and dependencies without changing state'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "check" -d 'Inspect a lifecycle mutation supplied after -- without applying it'
complete -c vde-worktree -n "__fish_vde_worktree_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion describe context doctor check help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'


# Dynamic candidates are emitted as shell-safe TSV by the Rust binary.
function __vw_dynamic_candidates
    set -l tokens (commandline -opc)
    set -l vw_bin vw
    if test (count $tokens) -gt 0
        set vw_bin $tokens[1]
    end
    command $vw_bin __complete $argv -- $tokens 2>/dev/null
end

for __vw_bin in vw vde-worktree
    complete -c $__vw_bin -f -n '__fish_vw_using_subcommand status path del absorb exec lock unlock' -a '(__vw_dynamic_candidates worktrees)'
    complete -c $__vw_bin -f -n '__fish_vw_using_subcommand switch use unabsorb' -a '(__vw_dynamic_candidates use-branches)'
    complete -c $__vw_bin -f -n '__fish_vw_using_subcommand get' -a '(__vw_dynamic_candidates remote-branches)'
    complete -c $__vw_bin -f -n '__fish_vw_using_subcommand invoke' -a '(__vw_dynamic_candidates hooks)'
    complete -c $__vw_bin -f -n '__fish_vw_using_subcommand absorb' -l from -a '(__vw_dynamic_candidates managed-worktrees)'
    complete -c $__vw_bin -f -n '__fish_vw_using_subcommand unabsorb' -l to -a '(__vw_dynamic_candidates managed-worktrees)'

end
