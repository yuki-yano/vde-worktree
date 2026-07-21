# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_vw_global_optspecs
    string join \n json verbose v/version hooks no-hooks gh no-gh full-path allow-unsafe strict-post-hooks hook-timeout-ms= lock-timeout-ms= prompt= fzf-arg= h/help
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

complete -c vw -n "__fish_vw_needs_command" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_needs_command" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_needs_command" -l prompt -r
complete -c vw -n "__fish_vw_needs_command" -l fzf-arg -r
complete -c vw -n "__fish_vw_needs_command" -l json
complete -c vw -n "__fish_vw_needs_command" -l verbose
complete -c vw -n "__fish_vw_needs_command" -s v -l version
complete -c vw -n "__fish_vw_needs_command" -l hooks
complete -c vw -n "__fish_vw_needs_command" -l no-hooks
complete -c vw -n "__fish_vw_needs_command" -l gh
complete -c vw -n "__fish_vw_needs_command" -l no-gh
complete -c vw -n "__fish_vw_needs_command" -l full-path
complete -c vw -n "__fish_vw_needs_command" -l allow-unsafe
complete -c vw -n "__fish_vw_needs_command" -l strict-post-hooks
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
complete -c vw -n "__fish_vw_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c vw -n "__fish_vw_using_subcommand init" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand init" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand init" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand init" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand init" -l json
complete -c vw -n "__fish_vw_using_subcommand init" -l verbose
complete -c vw -n "__fish_vw_using_subcommand init" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand init" -l hooks
complete -c vw -n "__fish_vw_using_subcommand init" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand init" -l gh
complete -c vw -n "__fish_vw_using_subcommand init" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand init" -l full-path
complete -c vw -n "__fish_vw_using_subcommand init" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand init" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand init" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand list" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand list" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand list" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand list" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand list" -l json
complete -c vw -n "__fish_vw_using_subcommand list" -l verbose
complete -c vw -n "__fish_vw_using_subcommand list" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand list" -l hooks
complete -c vw -n "__fish_vw_using_subcommand list" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand list" -l gh
complete -c vw -n "__fish_vw_using_subcommand list" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand list" -l full-path
complete -c vw -n "__fish_vw_using_subcommand list" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand list" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand list" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand status" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand status" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand status" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand status" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand status" -l json
complete -c vw -n "__fish_vw_using_subcommand status" -l verbose
complete -c vw -n "__fish_vw_using_subcommand status" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand status" -l hooks
complete -c vw -n "__fish_vw_using_subcommand status" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand status" -l gh
complete -c vw -n "__fish_vw_using_subcommand status" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand status" -l full-path
complete -c vw -n "__fish_vw_using_subcommand status" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand status" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand status" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand path" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand path" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand path" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand path" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand path" -l json
complete -c vw -n "__fish_vw_using_subcommand path" -l verbose
complete -c vw -n "__fish_vw_using_subcommand path" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand path" -l hooks
complete -c vw -n "__fish_vw_using_subcommand path" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand path" -l gh
complete -c vw -n "__fish_vw_using_subcommand path" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand path" -l full-path
complete -c vw -n "__fish_vw_using_subcommand path" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand path" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand path" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand switch" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand switch" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand switch" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand switch" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand switch" -l json
complete -c vw -n "__fish_vw_using_subcommand switch" -l verbose
complete -c vw -n "__fish_vw_using_subcommand switch" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand switch" -l hooks
complete -c vw -n "__fish_vw_using_subcommand switch" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand switch" -l gh
complete -c vw -n "__fish_vw_using_subcommand switch" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand switch" -l full-path
complete -c vw -n "__fish_vw_using_subcommand switch" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand switch" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand switch" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand new" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand new" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand new" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand new" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand new" -l json
complete -c vw -n "__fish_vw_using_subcommand new" -l verbose
complete -c vw -n "__fish_vw_using_subcommand new" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand new" -l hooks
complete -c vw -n "__fish_vw_using_subcommand new" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand new" -l gh
complete -c vw -n "__fish_vw_using_subcommand new" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand new" -l full-path
complete -c vw -n "__fish_vw_using_subcommand new" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand new" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand new" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand mv" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand mv" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand mv" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand mv" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand mv" -l json
complete -c vw -n "__fish_vw_using_subcommand mv" -l verbose
complete -c vw -n "__fish_vw_using_subcommand mv" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand mv" -l hooks
complete -c vw -n "__fish_vw_using_subcommand mv" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand mv" -l gh
complete -c vw -n "__fish_vw_using_subcommand mv" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand mv" -l full-path
complete -c vw -n "__fish_vw_using_subcommand mv" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand mv" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand mv" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand del" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand del" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand del" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand del" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand del" -l force
complete -c vw -n "__fish_vw_using_subcommand del" -l force-dirty
complete -c vw -n "__fish_vw_using_subcommand del" -l allow-unpushed
complete -c vw -n "__fish_vw_using_subcommand del" -l force-unmerged
complete -c vw -n "__fish_vw_using_subcommand del" -l force-locked
complete -c vw -n "__fish_vw_using_subcommand del" -l json
complete -c vw -n "__fish_vw_using_subcommand del" -l verbose
complete -c vw -n "__fish_vw_using_subcommand del" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand del" -l hooks
complete -c vw -n "__fish_vw_using_subcommand del" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand del" -l gh
complete -c vw -n "__fish_vw_using_subcommand del" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand del" -l full-path
complete -c vw -n "__fish_vw_using_subcommand del" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand del" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand del" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand gone" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand gone" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand gone" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand gone" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand gone" -l apply
complete -c vw -n "__fish_vw_using_subcommand gone" -l dry-run
complete -c vw -n "__fish_vw_using_subcommand gone" -l json
complete -c vw -n "__fish_vw_using_subcommand gone" -l verbose
complete -c vw -n "__fish_vw_using_subcommand gone" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand gone" -l hooks
complete -c vw -n "__fish_vw_using_subcommand gone" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand gone" -l gh
complete -c vw -n "__fish_vw_using_subcommand gone" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand gone" -l full-path
complete -c vw -n "__fish_vw_using_subcommand gone" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand gone" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand gone" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand adopt" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand adopt" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand adopt" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand adopt" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand adopt" -l apply
complete -c vw -n "__fish_vw_using_subcommand adopt" -l dry-run
complete -c vw -n "__fish_vw_using_subcommand adopt" -l json
complete -c vw -n "__fish_vw_using_subcommand adopt" -l verbose
complete -c vw -n "__fish_vw_using_subcommand adopt" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand adopt" -l hooks
complete -c vw -n "__fish_vw_using_subcommand adopt" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand adopt" -l gh
complete -c vw -n "__fish_vw_using_subcommand adopt" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand adopt" -l full-path
complete -c vw -n "__fish_vw_using_subcommand adopt" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand adopt" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand adopt" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand get" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand get" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand get" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand get" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand get" -l json
complete -c vw -n "__fish_vw_using_subcommand get" -l verbose
complete -c vw -n "__fish_vw_using_subcommand get" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand get" -l hooks
complete -c vw -n "__fish_vw_using_subcommand get" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand get" -l gh
complete -c vw -n "__fish_vw_using_subcommand get" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand get" -l full-path
complete -c vw -n "__fish_vw_using_subcommand get" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand get" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand get" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand extract" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand extract" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand extract" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand extract" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand extract" -l current
complete -c vw -n "__fish_vw_using_subcommand extract" -l stash
complete -c vw -n "__fish_vw_using_subcommand extract" -l json
complete -c vw -n "__fish_vw_using_subcommand extract" -l verbose
complete -c vw -n "__fish_vw_using_subcommand extract" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand extract" -l hooks
complete -c vw -n "__fish_vw_using_subcommand extract" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand extract" -l gh
complete -c vw -n "__fish_vw_using_subcommand extract" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand extract" -l full-path
complete -c vw -n "__fish_vw_using_subcommand extract" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand extract" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand extract" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand absorb" -l from -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand absorb" -l keep-stash
complete -c vw -n "__fish_vw_using_subcommand absorb" -l allow-agent
complete -c vw -n "__fish_vw_using_subcommand absorb" -l json
complete -c vw -n "__fish_vw_using_subcommand absorb" -l verbose
complete -c vw -n "__fish_vw_using_subcommand absorb" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand absorb" -l hooks
complete -c vw -n "__fish_vw_using_subcommand absorb" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand absorb" -l gh
complete -c vw -n "__fish_vw_using_subcommand absorb" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand absorb" -l full-path
complete -c vw -n "__fish_vw_using_subcommand absorb" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand absorb" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand absorb" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l to -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l keep-stash
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l allow-agent
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l json
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l verbose
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l hooks
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l gh
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l full-path
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand unabsorb" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand use" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand use" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand use" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand use" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand use" -l allow-agent
complete -c vw -n "__fish_vw_using_subcommand use" -l allow-shared
complete -c vw -n "__fish_vw_using_subcommand use" -l json
complete -c vw -n "__fish_vw_using_subcommand use" -l verbose
complete -c vw -n "__fish_vw_using_subcommand use" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand use" -l hooks
complete -c vw -n "__fish_vw_using_subcommand use" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand use" -l gh
complete -c vw -n "__fish_vw_using_subcommand use" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand use" -l full-path
complete -c vw -n "__fish_vw_using_subcommand use" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand use" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand use" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand exec" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand exec" -l json
complete -c vw -n "__fish_vw_using_subcommand exec" -l verbose
complete -c vw -n "__fish_vw_using_subcommand exec" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand exec" -l hooks
complete -c vw -n "__fish_vw_using_subcommand exec" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand exec" -l gh
complete -c vw -n "__fish_vw_using_subcommand exec" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand exec" -l full-path
complete -c vw -n "__fish_vw_using_subcommand exec" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand exec" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand exec" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand invoke" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand invoke" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand invoke" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand invoke" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand invoke" -l json
complete -c vw -n "__fish_vw_using_subcommand invoke" -l verbose
complete -c vw -n "__fish_vw_using_subcommand invoke" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand invoke" -l hooks
complete -c vw -n "__fish_vw_using_subcommand invoke" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand invoke" -l gh
complete -c vw -n "__fish_vw_using_subcommand invoke" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand invoke" -l full-path
complete -c vw -n "__fish_vw_using_subcommand invoke" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand invoke" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand invoke" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand copy" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand copy" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand copy" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand copy" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand copy" -l json
complete -c vw -n "__fish_vw_using_subcommand copy" -l verbose
complete -c vw -n "__fish_vw_using_subcommand copy" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand copy" -l hooks
complete -c vw -n "__fish_vw_using_subcommand copy" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand copy" -l gh
complete -c vw -n "__fish_vw_using_subcommand copy" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand copy" -l full-path
complete -c vw -n "__fish_vw_using_subcommand copy" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand copy" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand copy" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand link" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand link" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand link" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand link" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand link" -l json
complete -c vw -n "__fish_vw_using_subcommand link" -l verbose
complete -c vw -n "__fish_vw_using_subcommand link" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand link" -l hooks
complete -c vw -n "__fish_vw_using_subcommand link" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand link" -l gh
complete -c vw -n "__fish_vw_using_subcommand link" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand link" -l full-path
complete -c vw -n "__fish_vw_using_subcommand link" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand link" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand link" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand lock" -l owner -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l reason -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand lock" -l json
complete -c vw -n "__fish_vw_using_subcommand lock" -l verbose
complete -c vw -n "__fish_vw_using_subcommand lock" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand lock" -l hooks
complete -c vw -n "__fish_vw_using_subcommand lock" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand lock" -l gh
complete -c vw -n "__fish_vw_using_subcommand lock" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand lock" -l full-path
complete -c vw -n "__fish_vw_using_subcommand lock" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand lock" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand lock" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand unlock" -l owner -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand unlock" -l force
complete -c vw -n "__fish_vw_using_subcommand unlock" -l json
complete -c vw -n "__fish_vw_using_subcommand unlock" -l verbose
complete -c vw -n "__fish_vw_using_subcommand unlock" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand unlock" -l hooks
complete -c vw -n "__fish_vw_using_subcommand unlock" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand unlock" -l gh
complete -c vw -n "__fish_vw_using_subcommand unlock" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand unlock" -l full-path
complete -c vw -n "__fish_vw_using_subcommand unlock" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand unlock" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand unlock" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand cd" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand cd" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand cd" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand cd" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand cd" -l json
complete -c vw -n "__fish_vw_using_subcommand cd" -l verbose
complete -c vw -n "__fish_vw_using_subcommand cd" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand cd" -l hooks
complete -c vw -n "__fish_vw_using_subcommand cd" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand cd" -l gh
complete -c vw -n "__fish_vw_using_subcommand cd" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand cd" -l full-path
complete -c vw -n "__fish_vw_using_subcommand cd" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand cd" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand cd" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand completion" -l path -r -F
complete -c vw -n "__fish_vw_using_subcommand completion" -l hook-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand completion" -l lock-timeout-ms -r
complete -c vw -n "__fish_vw_using_subcommand completion" -l prompt -r
complete -c vw -n "__fish_vw_using_subcommand completion" -l fzf-arg -r
complete -c vw -n "__fish_vw_using_subcommand completion" -l install
complete -c vw -n "__fish_vw_using_subcommand completion" -l json
complete -c vw -n "__fish_vw_using_subcommand completion" -l verbose
complete -c vw -n "__fish_vw_using_subcommand completion" -s v -l version
complete -c vw -n "__fish_vw_using_subcommand completion" -l hooks
complete -c vw -n "__fish_vw_using_subcommand completion" -l no-hooks
complete -c vw -n "__fish_vw_using_subcommand completion" -l gh
complete -c vw -n "__fish_vw_using_subcommand completion" -l no-gh
complete -c vw -n "__fish_vw_using_subcommand completion" -l full-path
complete -c vw -n "__fish_vw_using_subcommand completion" -l allow-unsafe
complete -c vw -n "__fish_vw_using_subcommand completion" -l strict-post-hooks
complete -c vw -n "__fish_vw_using_subcommand completion" -s h -l help -d 'Print help'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "init" -d 'Initialize repository-local vde-worktree state'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "list" -d 'List worktrees and status metadata'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "status" -d 'Show a single worktree status'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "path" -d 'Print the absolute path for a branch worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "switch" -d 'Reuse or create a worktree for a branch'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "new" -d 'Create a branch and its worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "mv" -d 'Rename the current linked worktree branch'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "del" -d 'Delete a linked worktree and branch'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "gone" -d 'Find or delete stale merged worktrees'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "adopt" -d 'Find or move unmanaged worktrees into the managed root'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "get" -d 'Fetch and attach a remote branch'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "extract" -d 'Extract the current primary branch into the managed root'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "absorb" -d 'Transfer linked worktree changes into the primary worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "unabsorb" -d 'Transfer primary worktree changes into a linked worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "use" -d 'Check out a branch in the primary worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "exec" -d 'Run an argv command in a branch worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "invoke" -d 'Invoke a named hook'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "copy" -d 'Copy repository-relative paths into the target worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "link" -d 'Link repository-relative paths into the target worktree'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "lock" -d 'Protect a worktree with persistent lock metadata'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "unlock" -d 'Remove persistent lock metadata'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "cd" -d 'Select a worktree path interactively'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "completion" -d 'Generate or install shell completions'
complete -c vw -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'

complete -c vde-worktree -n "__fish_vw_needs_command" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_needs_command" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_needs_command" -l prompt -r
complete -c vde-worktree -n "__fish_vw_needs_command" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_needs_command" -l json
complete -c vde-worktree -n "__fish_vw_needs_command" -l verbose
complete -c vde-worktree -n "__fish_vw_needs_command" -s v -l version
complete -c vde-worktree -n "__fish_vw_needs_command" -l hooks
complete -c vde-worktree -n "__fish_vw_needs_command" -l no-hooks
complete -c vde-worktree -n "__fish_vw_needs_command" -l gh
complete -c vde-worktree -n "__fish_vw_needs_command" -l no-gh
complete -c vde-worktree -n "__fish_vw_needs_command" -l full-path
complete -c vde-worktree -n "__fish_vw_needs_command" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_needs_command" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_needs_command" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "init" -d 'Initialize repository-local vde-worktree state'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "list" -d 'List worktrees and status metadata'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "status" -d 'Show a single worktree status'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "path" -d 'Print the absolute path for a branch worktree'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "switch" -d 'Reuse or create a worktree for a branch'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "new" -d 'Create a branch and its worktree'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "mv" -d 'Rename the current linked worktree branch'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "del" -d 'Delete a linked worktree and branch'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "gone" -d 'Find or delete stale merged worktrees'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "adopt" -d 'Find or move unmanaged worktrees into the managed root'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "get" -d 'Fetch and attach a remote branch'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "extract" -d 'Extract the current primary branch into the managed root'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "absorb" -d 'Transfer linked worktree changes into the primary worktree'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "unabsorb" -d 'Transfer primary worktree changes into a linked worktree'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "use" -d 'Check out a branch in the primary worktree'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "exec" -d 'Run an argv command in a branch worktree'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "invoke" -d 'Invoke a named hook'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "copy" -d 'Copy repository-relative paths into the target worktree'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "link" -d 'Link repository-relative paths into the target worktree'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "lock" -d 'Protect a worktree with persistent lock metadata'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "unlock" -d 'Remove persistent lock metadata'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "cd" -d 'Select a worktree path interactively'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "completion" -d 'Generate or install shell completions'
complete -c vde-worktree -n "__fish_vw_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand init" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand list" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand status" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand path" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand switch" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand new" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand mv" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l force
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l force-dirty
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l allow-unpushed
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l force-unmerged
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l force-locked
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand del" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l apply
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l dry-run
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand gone" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l apply
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l dry-run
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand adopt" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand get" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l current
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l stash
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand extract" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l from -r
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l keep-stash
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l allow-agent
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand absorb" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l to -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l keep-stash
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l allow-agent
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand unabsorb" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l allow-agent
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l allow-shared
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand use" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand exec" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand invoke" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand copy" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand link" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l owner -r
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l reason -r
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand lock" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l owner -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l force
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand unlock" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand cd" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l path -r -F
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l hook-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l lock-timeout-ms -r
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l prompt -r
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l fzf-arg -r
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l install
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l json
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l verbose
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -s v -l version
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l no-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l gh
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l no-gh
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l full-path
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l allow-unsafe
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -l strict-post-hooks
complete -c vde-worktree -n "__fish_vw_using_subcommand completion" -s h -l help -d 'Print help'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "init" -d 'Initialize repository-local vde-worktree state'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "list" -d 'List worktrees and status metadata'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "status" -d 'Show a single worktree status'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "path" -d 'Print the absolute path for a branch worktree'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "switch" -d 'Reuse or create a worktree for a branch'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "new" -d 'Create a branch and its worktree'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "mv" -d 'Rename the current linked worktree branch'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "del" -d 'Delete a linked worktree and branch'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "gone" -d 'Find or delete stale merged worktrees'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "adopt" -d 'Find or move unmanaged worktrees into the managed root'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "get" -d 'Fetch and attach a remote branch'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "extract" -d 'Extract the current primary branch into the managed root'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "absorb" -d 'Transfer linked worktree changes into the primary worktree'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "unabsorb" -d 'Transfer primary worktree changes into a linked worktree'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "use" -d 'Check out a branch in the primary worktree'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "exec" -d 'Run an argv command in a branch worktree'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "invoke" -d 'Invoke a named hook'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "copy" -d 'Copy repository-relative paths into the target worktree'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "link" -d 'Link repository-relative paths into the target worktree'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "lock" -d 'Protect a worktree with persistent lock metadata'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "unlock" -d 'Remove persistent lock metadata'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "cd" -d 'Select a worktree path interactively'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "completion" -d 'Generate or install shell completions'
complete -c vde-worktree -n "__fish_vw_using_subcommand help; and not __fish_seen_subcommand_from init list status path switch new mv del gone adopt get extract absorb unabsorb use exec invoke copy link lock unlock cd completion help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'

# Dynamic candidates are emitted as shell-safe TSV by the Rust binary.
function __vw_dynamic_candidates
    set -l tokens (commandline -opc)
    set -l vw_bin vw
    if test (count $tokens) -gt 0
        set vw_bin $tokens[1]
    end
    command $vw_bin __complete $argv 2>/dev/null
end

for __vw_bin in vw vde-worktree
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from status path switch del absorb exec lock unlock' -a '(__vw_dynamic_candidates worktrees)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from use unabsorb' -a '(__vw_dynamic_candidates use-branches)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from get' -a '(__vw_dynamic_candidates remote-branches)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from invoke' -a '(__vw_dynamic_candidates hooks)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from absorb' -l from -a '(__vw_dynamic_candidates managed-worktrees)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from unabsorb' -l to -a '(__vw_dynamic_candidates managed-worktrees)'
end
