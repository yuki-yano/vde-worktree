pub mod contract;

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{ArgAction, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};

use crate::domain::error::{CliError, ErrorCode};
use crate::domain::fzf::validate_fzf_extra_args;
use crate::domain::hook::HookName;
use crate::domain::safety::{CommonSafetyPolicy, enforce_common_safety};

pub const COMMAND_NAMES: [&str; 27] = [
    "init",
    "list",
    "status",
    "path",
    "switch",
    "new",
    "mv",
    "del",
    "gone",
    "adopt",
    "get",
    "extract",
    "absorb",
    "unabsorb",
    "use",
    "exec",
    "invoke",
    "copy",
    "link",
    "lock",
    "unlock",
    "cd",
    "completion",
    "describe",
    "context",
    "doctor",
    "check",
];

#[derive(Debug, Clone, PartialEq, Parser)]
#[command(
    name = "vw",
    bin_name = "vw",
    version,
    about = "Git worktree manager with safe defaults for humans and coding agents",
    arg_required_else_help = true,
    subcommand_required = true,
    propagate_version = true,
    disable_version_flag = true,
    disable_help_subcommand = false
)]
struct Cli {
    #[command(flatten)]
    common: CommonOptions,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct CommonOptions {
    #[arg(short = 'C', long, global = true, value_name = "DIRECTORY")]
    /// Resolve repository, config, hooks and relative paths from this directory.
    pub directory: Option<PathBuf>,

    #[arg(long, global = true, value_name = "PATH")]
    /// Select a registered worktree by path for status, path, exec, copy or link.
    pub worktree: Option<PathBuf>,

    #[arg(long, global = true)]
    /// Emit one JSON schema 3 object on stdout; diagnostics remain available as structured warnings.
    pub json: bool,

    #[arg(long, global = true)]
    /// Inspect a lifecycle mutation without hooks, staging, locks, metadata writes or recovery.
    pub dry_run: bool,

    #[arg(skip)]
    pub check: bool,

    #[arg(long, global = true, action = ArgAction::Count)]
    /// Show resolved context and result diagnostics on stderr; repeat to include configuration.
    pub verbose: u8,

    #[arg(
        short = 'v',
        long,
        global = true,
        action = ArgAction::Version,
        required = false
    )]
    /// Print the version.
    version: Option<bool>,

    #[arg(long, global = true, overrides_with = "no_hooks")]
    /// Enable automatic command hooks.
    pub hooks: bool,

    #[arg(long = "no-hooks", global = true, overrides_with = "hooks")]
    /// Disable automatic hooks; requires --allow-unsafe.
    pub no_hooks: bool,

    #[arg(long, global = true, overrides_with = "no_gh")]
    /// Enable GitHub pull request lookup.
    pub gh: bool,

    #[arg(long = "no-gh", global = true, overrides_with = "gh")]
    /// Disable GitHub lookup and network requests made by that lookup.
    pub no_gh: bool,

    #[arg(long, global = true)]
    /// Show absolute paths in human list output.
    pub full_path: bool,

    #[arg(long, global = true)]
    /// Acknowledge explicitly requested unsafe operations.
    pub allow_unsafe: bool,

    #[arg(long, global = true)]
    /// Return an error if a post-hook fails; retain the completed operation result.
    pub strict_post_hooks: bool,

    #[arg(long, global = true, value_parser = clap::value_parser!(u64).range(1..))]
    /// Maximum time per hook in milliseconds (default: hooks.timeoutMs, 30000).
    pub hook_timeout_ms: Option<u64>,

    #[arg(long, global = true, value_parser = clap::value_parser!(u64).range(1..))]
    /// Maximum wait for the repository mutation lock in milliseconds (default: locks.timeoutMs, 15000).
    pub lock_timeout_ms: Option<u64>,

    #[arg(long, global = true)]
    /// Override the interactive cd picker prompt.
    pub prompt: Option<String>,

    #[arg(
        long = "fzf-arg",
        global = true,
        action = ArgAction::Append,
        allow_hyphen_values = true
    )]
    /// Append one fzf argument; use --fzf-arg=VALUE for values beginning with a dash.
    pub fzf_args: Vec<String>,
}

impl CommonOptions {
    pub const fn hooks_enabled(&self, configured: bool) -> bool {
        self.hooks || (!self.no_hooks && configured)
    }

    pub const fn gh_enabled(&self, configured: bool) -> bool {
        self.gh || (!self.no_gh && configured)
    }
}

#[derive(Debug, Clone, PartialEq, Subcommand)]
pub enum Command {
    /// Initialize repository-local vde-worktree state.
    Init,
    /// List worktrees and status metadata.
    List {
        /// Emit a lightweight internal snapshot for monitor integrations.
        #[arg(long)]
        monitor: bool,
    },
    /// Show a single worktree status.
    Status {
        /// Branch to inspect (default: current worktree).
        branch: Option<String>,
    },
    /// Print the absolute path for a branch worktree.
    Path {
        /// Branch whose attached worktree path is printed.
        branch: Option<String>,
    },
    /// Reuse or create a worktree for a branch.
    Switch {
        /// Existing or new local branch; new branches start at the configured base.
        branch: String,
    },
    /// Create a branch and its worktree.
    New {
        /// New branch name (default: a generated wip name).
        branch: Option<String>,
    },
    /// Rename the current linked worktree branch.
    Mv {
        /// New branch name and managed directory for the current linked worktree.
        new_branch: String,
    },
    /// Delete a linked worktree and branch.
    Del {
        /// Branch to delete (default: current linked worktree).
        branch: Option<String>,
        #[arg(long)]
        /// Enable every deletion override; non-interactive use requires --allow-unsafe.
        force: bool,
        #[arg(long)]
        /// Allow discarding dirty worktree files; non-interactive use requires --allow-unsafe.
        force_dirty: bool,
        #[arg(long)]
        /// Allow commits ahead of upstream or unknown upstream state; non-interactive use requires --allow-unsafe.
        allow_unpushed: bool,
        #[arg(long)]
        /// Allow deleting work not known to be merged; non-interactive use requires --allow-unsafe.
        force_unmerged: bool,
        #[arg(long)]
        /// Allow deleting a protected worktree; non-interactive use requires --allow-unsafe.
        force_locked: bool,
    },
    /// Find or delete stale merged worktrees.
    Gone {
        #[arg(long)]
        /// Delete the eligible candidates (default: preview only).
        apply: bool,
    },
    /// Find or move unmanaged worktrees into the managed root.
    Adopt {
        #[arg(long)]
        /// Move eligible external worktrees into the managed root (default: preview only).
        apply: bool,
    },
    /// Fetch and attach a remote branch.
    Get {
        /// Remote and branch separated by a slash, for example origin/feature/topic.
        remote_branch: String,
    },
    /// Extract the current primary branch into the managed root.
    Extract {
        #[arg(long, required = true)]
        /// Extract the current primary branch; required.
        current: bool,
        #[arg(long)]
        /// Temporarily stash dirty tracked and untracked changes for transfer.
        stash: bool,
    },
    /// Transfer linked worktree changes into the primary worktree.
    Absorb {
        /// Branch to check out in the primary worktree and receive changes.
        branch: String,
        #[arg(long)]
        /// Managed source worktree name when branch attachment is ambiguous.
        from: Option<String>,
        #[arg(long)]
        /// Retain the exact transfer stash after successful application.
        keep_stash: bool,
        #[arg(long)]
        /// Allow non-interactive transfer; also requires --allow-unsafe.
        allow_agent: bool,
    },
    /// Transfer primary worktree changes into a linked worktree.
    Unabsorb {
        /// Current primary branch whose changes will be transferred.
        branch: String,
        #[arg(long)]
        /// Managed target worktree name when branch attachment is ambiguous.
        to: Option<String>,
        #[arg(long)]
        /// Retain the exact transfer stash after successful application.
        keep_stash: bool,
        #[arg(long)]
        /// Allow non-interactive transfer; also requires --allow-unsafe.
        allow_agent: bool,
    },
    /// Check out a branch in the primary worktree.
    Use {
        /// Local branch to check out in the primary worktree.
        branch: String,
        #[arg(long)]
        /// Allow non-interactive checkout; also requires --allow-unsafe.
        allow_agent: bool,
        #[arg(long)]
        /// Allow the branch to remain attached to a linked worktree.
        allow_shared: bool,
    },
    /// Run an argv command in a branch worktree.
    Exec {
        #[command(flatten)]
        options: ExecOptions,
        /// Branch whose attached worktree becomes the child process cwd.
        branch: Option<String>,
        #[arg(last = true, required = true, num_args = 1.., allow_hyphen_values = true)]
        /// Executable and arguments after --; passed directly without shell interpretation.
        argv: Vec<OsString>,
    },
    /// Invoke a named hook.
    Invoke {
        /// Hook name, for example post-switch.
        hook: HookName,
        #[arg(last = true, num_args = 0.., allow_hyphen_values = true)]
        /// Arguments after --, passed directly to the named hook.
        argv: Vec<OsString>,
    },
    /// Copy repository-relative paths into the target worktree.
    Copy {
        #[arg(required = true, num_args = 1..)]
        /// Repository-relative files or directories to copy; no absolute paths or traversal.
        paths: Vec<PathBuf>,
    },
    /// Link repository-relative paths into the target worktree.
    Link {
        #[arg(required = true, num_args = 1..)]
        /// Repository-relative files or directories to symlink; no absolute paths or traversal.
        paths: Vec<PathBuf>,
    },
    /// Protect a worktree with persistent lock metadata.
    Lock {
        /// Branch to protect with persistent lock metadata.
        branch: String,
        #[arg(long)]
        /// Lock owner (default: current user); use a unique session identifier for agents.
        owner: Option<String>,
        #[arg(long)]
        /// Reason for protecting the worktree.
        reason: Option<String>,
    },
    /// Remove persistent lock metadata.
    Unlock {
        /// Branch whose persistent lock is removed.
        branch: String,
        #[arg(long)]
        /// Expected owner (default: current user).
        owner: Option<String>,
        #[arg(long)]
        /// Remove the lock regardless of owner or record validity.
        force: bool,
    },
    /// Select a worktree path interactively.
    Cd,
    /// Generate or install shell completions.
    Completion {
        /// Shell to generate completions for.
        shell: CompletionShell,
        #[arg(long)]
        /// Atomically install the generated script instead of printing it.
        install: bool,
        #[arg(long)]
        /// Installation path (default: the shell completion directory).
        path: Option<PathBuf>,
    },
    /// Describe commands, arguments, effects, and the JSON output contract.
    Describe {
        /// Command to describe (default: all public commands).
        command: Option<String>,
    },
    /// Show execution context, effective configuration and setting sources.
    Context,
    /// Diagnose repository setup, configuration and dependencies without changing state.
    Doctor,
    /// Inspect a lifecycle mutation supplied after -- without applying it.
    Check {
        #[arg(last = true, required = true, num_args = 1.., allow_hyphen_values = true)]
        /// Lifecycle command and its arguments, for example -- del feature/topic.
        argv: Vec<OsString>,
    },
    /// Internal completion candidate provider.
    #[command(name = "__complete", hide = true)]
    CompletionCandidates {
        /// Candidate category requested by the shell integration.
        kind: CompletionCandidateKind,
        /// Original shell command line, used only to resolve -C.
        #[arg(last = true, allow_hyphen_values = true)]
        commandline: Vec<OsString>,
    },
}

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::List { .. } => "list",
            Self::Status { .. } => "status",
            Self::Path { .. } => "path",
            Self::Switch { .. } => "switch",
            Self::New { .. } => "new",
            Self::Mv { .. } => "mv",
            Self::Del { .. } => "del",
            Self::Gone { .. } => "gone",
            Self::Adopt { .. } => "adopt",
            Self::Get { .. } => "get",
            Self::Extract { .. } => "extract",
            Self::Absorb { .. } => "absorb",
            Self::Unabsorb { .. } => "unabsorb",
            Self::Use { .. } => "use",
            Self::Exec { .. } => "exec",
            Self::Invoke { .. } => "invoke",
            Self::Copy { .. } => "copy",
            Self::Link { .. } => "link",
            Self::Lock { .. } => "lock",
            Self::Unlock { .. } => "unlock",
            Self::Cd => "cd",
            Self::Completion { .. } => "completion",
            Self::Describe { .. } => "describe",
            Self::Context => "context",
            Self::Doctor => "doctor",
            Self::Check { .. } => "check",
            Self::CompletionCandidates { .. } => "__complete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ExecOptions {
    #[arg(long, default_value_t = 300_000, value_parser = clap::value_parser!(u64).range(1..))]
    /// Maximum child runtime in milliseconds, including captured stream draining.
    pub timeout_ms: u64,

    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    /// Retain at most this many raw bytes per JSON output stream (default: 1048576); drain the rest.
    pub max_output_bytes: Option<u64>,

    #[arg(long, value_enum, default_value = "null")]
    /// Child stdin: null closes input; inherit passes the invoking process input through.
    pub stdin: ExecStdin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExecStdin {
    Null,
    Inherit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    Zsh,
    Fish,
}

impl CompletionShell {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Zsh => "zsh",
            Self::Fish => "fish",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CompletionCandidateKind {
    Worktrees,
    UseBranches,
    RemoteBranches,
    Hooks,
    ManagedWorktrees,
}

pub fn clap_command() -> clap::Command {
    contract::document_command(Cli::command())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRequest {
    pub common: CommonOptions,
    pub command: Command,
}

impl ParsedRequest {
    pub const fn output_command(&self) -> &'static str {
        if self.common.check {
            "check"
        } else {
            self.command.name()
        }
    }

    pub const fn is_preview(&self) -> bool {
        self.common.dry_run
            || matches!(
                self.command,
                Command::Gone { apply: false } | Command::Adopt { apply: false }
            )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CliParseResult {
    Parsed(ParsedRequest),
    Display(String),
    Invalid { error: CliError, rendered: String },
}

pub fn parse_from<I, T>(args: I) -> CliParseResult
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    parse_args(&args, false)
}

fn parse_args(args: &[OsString], nested_check: bool) -> CliParseResult {
    let hints = argument_hints(args);
    let parsed = clap_command()
        .try_get_matches_from(args)
        .and_then(|matches| Cli::from_arg_matches(&matches));
    match parsed {
        Ok(mut cli) => {
            if let Command::Check { argv } = &cli.command {
                if nested_check {
                    return invalid_request("check cannot inspect another check command");
                }
                let separator = args.len() - argv.len() - 1;
                let mut combined = args[..separator].to_vec();
                combined.remove(hints.command_index.expect("check is a parsed command"));
                combined.extend_from_slice(argv);
                let result = parse_args(&combined, true);
                return match result {
                    CliParseResult::Parsed(mut request) => {
                        if !crate::app::dispatch::is_write_command(&request.command) {
                            return invalid_request(
                                "check supports lifecycle mutation commands only",
                            );
                        }
                        request.common.check = true;
                        request.common.dry_run = true;
                        CliParseResult::Parsed(request)
                    }
                    other => other,
                };
            }
            if cli.common.dry_run && !crate::app::dispatch::is_write_command(&cli.command) {
                return invalid_request(
                    "--dry-run and check support lifecycle mutation commands only",
                );
            }
            if let Command::CompletionCandidates { commandline, .. } = &cli.command
                && !commandline.is_empty()
            {
                cli.common.directory = argument_hints(commandline).directory;
            }
            if cli.common.worktree.is_some()
                && !matches!(
                    cli.command,
                    Command::Status { .. }
                        | Command::Path { .. }
                        | Command::Exec { .. }
                        | Command::Copy { .. }
                        | Command::Link { .. }
                )
            {
                let error = CliError::new(
                    ErrorCode::InvalidArgument,
                    "--worktree is supported by status, path, exec, copy and link",
                );
                return CliParseResult::Invalid {
                    rendered: error.message.clone(),
                    error,
                };
            }
            apply_toggle(&mut cli.common.hooks, &mut cli.common.no_hooks, hints.hooks);
            apply_toggle(&mut cli.common.gh, &mut cli.common.no_gh, hints.gh);
            if let Err(error) = validate_common_options(&cli.common)
                .and_then(|()| validate_command_options(&cli.command, &cli.common))
            {
                let rendered = format!("error: {}\n", error.message);
                return CliParseResult::Invalid { error, rendered };
            }
            CliParseResult::Parsed(ParsedRequest {
                common: cli.common,
                command: cli.command,
            })
        }
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                CliParseResult::Display(error.to_string())
            }
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand if args.len() == 1 => {
                CliParseResult::Display(error.to_string())
            }
            _ => CliParseResult::Invalid {
                error: CliError::new(
                    if error.kind() == ErrorKind::InvalidSubcommand {
                        ErrorCode::UnknownCommand
                    } else {
                        ErrorCode::InvalidArgument
                    },
                    error.to_string(),
                ),
                rendered: error.to_string(),
            },
        },
    }
}

fn invalid_request(message: &str) -> CliParseResult {
    let error = CliError::new(ErrorCode::InvalidArgument, message);
    CliParseResult::Invalid {
        rendered: format!("error: {message}\n"),
        error,
    }
}

fn validate_common_options(common: &CommonOptions) -> Result<(), CliError> {
    enforce_common_safety(CommonSafetyPolicy {
        hooks_disabled: common.no_hooks,
        allow_unsafe: common.allow_unsafe,
    })?;
    validate_fzf_extra_args(&common.fzf_args)
        .map_err(|error| CliError::new(ErrorCode::InvalidArgument, error.to_string()))
}

fn validate_command_options(command: &Command, common: &CommonOptions) -> Result<(), CliError> {
    // Clap validates subcommand relations before propagating ancestor global arguments.
    // Cross-command constraints must use the fully resolved options, independent of argv position.
    let message = match command {
        Command::Status { branch: Some(_) }
        | Command::Path { branch: Some(_) }
        | Command::Exec {
            branch: Some(_), ..
        } if common.worktree.is_some() => Some("branch and --worktree cannot be used together"),
        Command::Path { branch: None } | Command::Exec { branch: None, .. }
            if common.worktree.is_none() =>
        {
            Some("a branch or --worktree path is required")
        }
        Command::Exec { options, .. } if options.max_output_bytes.is_some() && !common.json => {
            Some("--max-output-bytes requires --json")
        }
        Command::List { monitor: true } if !common.json || !common.no_gh || common.gh => {
            Some("--monitor requires --json and --no-gh, and cannot be used with --gh")
        }
        Command::Gone { apply: true } | Command::Adopt { apply: true } if common.dry_run => {
            Some("--apply and --dry-run cannot be used together")
        }
        _ => None,
    };
    message.map_or(Ok(()), |message| {
        Err(CliError::new(ErrorCode::InvalidArgument, message))
    })
}

#[derive(Default, Debug)]
pub struct ArgumentHints {
    pub directory: Option<PathBuf>,
    pub json: bool,
    pub command: Option<String>,
    command_index: Option<usize>,
    hooks: Option<bool>,
    gh: Option<bool>,
}

/// Tolerant scan used only for error rendering and last-wins global toggles. Option arity comes
/// from the same Clap definition as parsing; option values and child argv are never reinterpreted.
pub fn argument_hints(args: &[OsString]) -> ArgumentHints {
    let mut definition = Cli::command();
    definition.build();
    let mut command = &definition;
    let mut hints = ArgumentHints::default();
    let mut index = 1;
    while let Some(token) = args.get(index) {
        index += 1;
        if token == OsStr::new("--") {
            break;
        }
        let Some(token) = token.to_str() else {
            continue;
        };
        if let Some(long) = token.strip_prefix("--") {
            if let Some(value) = long.strip_prefix("directory=") {
                hints.directory = Some(PathBuf::from(value));
            } else if long == "directory"
                && args
                    .get(index)
                    .is_some_and(|value| !value.as_encoded_bytes().starts_with(b"-"))
            {
                hints.directory = args.get(index).map(PathBuf::from);
            }
            let (long, inline) = long
                .split_once('=')
                .map_or((long, false), |(key, _)| (key, true));
            if let Some(arg) = command
                .get_arguments()
                .find(|arg| arg.get_long() == Some(long))
            {
                if !inline {
                    record_hint(&mut hints, arg.get_id().as_str());
                }
                if arg.get_action().takes_values()
                    && !inline
                    && consumes_next_value(arg, args.get(index))
                {
                    index += 1;
                }
            }
        } else if let Some(shorts) = token.strip_prefix('-').filter(|value| !value.is_empty()) {
            let mut shorts = shorts.chars().peekable();
            while let Some(short) = shorts.next() {
                if let Some(arg) = command
                    .get_arguments()
                    .find(|arg| arg.get_short() == Some(short))
                {
                    record_hint(&mut hints, arg.get_id().as_str());
                    if arg.get_action().takes_values() {
                        if arg.get_id() == "directory" {
                            let inline = shorts.clone().collect::<String>();
                            hints.directory = if inline.is_empty() {
                                args.get(index)
                                    .filter(|_| consumes_next_value(arg, args.get(index)))
                                    .map(PathBuf::from)
                            } else {
                                Some(PathBuf::from(inline.trim_start_matches('=')))
                            };
                        }
                        if shorts.peek().is_none() && consumes_next_value(arg, args.get(index)) {
                            index += 1;
                        }
                        break;
                    }
                }
            }
        } else if hints.command.is_none() {
            hints.command = Some(token.to_owned());
            hints.command_index = Some(index - 1);
            if let Some(subcommand) = definition.find_subcommand(token) {
                command = subcommand;
            }
        }
    }
    hints
}

fn consumes_next_value(argument: &clap::Arg, next: Option<&OsString>) -> bool {
    next.is_some_and(|value| {
        argument.is_allow_hyphen_values_set()
            || value == "-"
            || !value.as_encoded_bytes().starts_with(b"-")
    })
}

fn record_hint(hints: &mut ArgumentHints, id: &str) {
    match id {
        "json" => hints.json = true,
        "hooks" => hints.hooks = Some(true),
        "no_hooks" => hints.hooks = Some(false),
        "gh" => hints.gh = Some(true),
        "no_gh" => hints.gh = Some(false),
        _ => {}
    }
}

fn apply_toggle(positive: &mut bool, negative: &mut bool, enabled: Option<bool>) {
    if let Some(enabled) = enabled {
        *positive = enabled;
        *negative = !enabled;
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{
        COMMAND_NAMES, CliParseResult, Command, CompletionCandidateKind, clap_command, parse_from,
    };
    use crate::domain::error::ErrorCode;

    fn request(command_args: &[&str]) -> super::ParsedRequest {
        let mut full_args = vec!["vw"];
        full_args.extend_from_slice(command_args);
        match parse_from(full_args) {
            CliParseResult::Parsed(request) => request,
            outcome => panic!("expected parsed request, got {outcome:?}"),
        }
    }

    #[test]
    fn parses_all_public_command_variants() {
        let cases: [&[&str]; 27] = [
            &["init"],
            &["list"],
            &["status"],
            &["path", "main"],
            &["switch", "main"],
            &["new"],
            &["mv", "renamed"],
            &["del"],
            &["gone"],
            &["adopt"],
            &["get", "origin/main"],
            &["extract", "--current"],
            &["absorb", "main"],
            &["unabsorb", "main"],
            &["use", "main"],
            &["exec", "main", "--", "true"],
            &["invoke", "post-switch"],
            &["copy", "config.yml"],
            &["link", "config.yml"],
            &["lock", "main"],
            &["unlock", "main"],
            &["cd"],
            &["completion", "zsh"],
            &["describe"],
            &["context"],
            &["doctor"],
            &["check", "--", "new", "topic"],
        ];

        let parsed_names: Vec<_> = cases
            .iter()
            .map(|args| request(args).output_command())
            .collect();
        assert_eq!(parsed_names, COMMAND_NAMES);
    }

    #[test]
    fn check_preserves_options_and_rejects_nested_or_non_lifecycle_commands() {
        let request = request(&[
            "check",
            "--fzf-arg",
            "--json",
            "-C",
            "/repo",
            "--",
            "del",
            "topic",
            "--force-dirty",
            "--allow-unsafe",
        ]);
        assert!(request.common.check && request.common.dry_run);
        assert!(!request.common.json);
        assert_eq!(request.common.fzf_args, ["--json"]);
        assert_eq!(
            request.common.directory.as_deref(),
            Some(std::path::Path::new("/repo"))
        );
        assert!(matches!(
            request.command,
            Command::Del {
                force_dirty: true,
                ..
            }
        ));
        for args in [
            vec!["vw", "check", "--", "check", "--", "new", "topic"],
            vec!["vw", "check", "--", "exec", "topic", "--", "true"],
            vec!["vw", "list", "--dry-run"],
            vec!["vw", "completion", "zsh", "--dry-run"],
        ] {
            assert!(matches!(parse_from(args), CliParseResult::Invalid { .. }));
        }
    }

    #[test]
    fn monitor_mode_requires_its_machine_only_option_contract() {
        let parsed = request(&["list", "--json", "--no-gh", "--monitor"]);
        assert!(matches!(parsed.command, Command::List { monitor: true }));

        for args in [
            &["list", "--monitor"][..],
            &["list", "--json", "--monitor"],
            &["list", "--json", "--no-gh", "--gh", "--monitor"],
            &["status", "--json", "--no-gh", "--monitor"],
        ] {
            let mut full_args = vec!["vw"];
            full_args.extend_from_slice(args);
            let CliParseResult::Invalid { error, .. } = parse_from(full_args) else {
                panic!("expected validation failure for {args:?}");
            };
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{args:?}");
        }
    }

    #[test]
    fn accepts_global_options_before_and_after_the_command() {
        let before = request(&["--json", "--lock-timeout-ms", "42", "list"]);
        let after = request(&["list", "--json", "--lock-timeout-ms", "42"]);

        assert_eq!(before.common, after.common);
        assert!(before.common.json);
        assert_eq!(before.common.lock_timeout_ms, Some(42));
    }

    #[test]
    fn validates_global_relations_independently_of_argument_position() {
        for (globals, command) in [
            (
                vec!["--json"],
                vec!["exec", "topic", "--max-output-bytes", "17"],
            ),
            (vec!["--worktree", "/repo/topic"], vec!["exec"]),
            (vec!["--worktree", "/repo/topic"], vec!["path"]),
            (vec!["--worktree", "/repo/topic"], vec!["status"]),
            (vec!["--json", "--no-gh"], vec!["list", "--monitor"]),
        ] {
            let child = if command[0] == "exec" {
                vec!["--", "true"]
            } else {
                vec![]
            };
            let before = [globals.clone(), command.clone(), child.clone()].concat();
            let after = [command, globals, child].concat();
            assert_eq!(request(&before), request(&after), "{before:?}");
        }
        for args in [
            vec!["--worktree", "/repo/topic", "status", "topic"],
            vec!["--worktree", "/repo/topic", "path", "topic"],
            vec!["--worktree", "/repo/topic", "exec", "topic", "--", "true"],
            vec!["--dry-run", "gone", "--apply"],
            vec!["--dry-run", "adopt", "--apply"],
            vec!["--json", "list", "--monitor"],
            vec!["--no-gh", "list", "--monitor"],
            vec!["path"],
            vec!["exec", "--", "true"],
        ] {
            let result = parse_from([vec!["vw"], args.clone()].concat());
            assert!(matches!(result, CliParseResult::Invalid { .. }), "{args:?}");
        }
    }

    #[test]
    fn wires_every_non_display_root_option_through_the_public_parser() {
        let parsed = request(&[
            "list",
            "--json",
            "--verbose",
            "--verbose",
            "--hooks",
            "--gh",
            "--full-path",
            "--allow-unsafe",
            "--strict-post-hooks",
            "--hook-timeout-ms",
            "101",
            "--lock-timeout-ms",
            "202",
            "--prompt",
            "pick> ",
            "--fzf-arg=--ansi",
            "--fzf-arg=--nth=1",
        ]);

        assert!(parsed.common.json);
        assert_eq!(parsed.common.verbose, 2);
        assert!(parsed.common.hooks_enabled(true));
        assert!(parsed.common.gh_enabled(true));
        assert!(parsed.common.full_path);
        assert!(parsed.common.allow_unsafe);
        assert!(parsed.common.strict_post_hooks);
        assert_eq!(parsed.common.hook_timeout_ms, Some(101));
        assert_eq!(parsed.common.lock_timeout_ms, Some(202));
        assert_eq!(parsed.common.prompt.as_deref(), Some("pick> "));
        assert_eq!(parsed.common.fzf_args, ["--ansi", "--nth=1"]);

        let disabled = request(&["list", "--no-hooks", "--allow-unsafe", "--no-gh"]);
        assert!(!disabled.common.hooks_enabled(true));
        assert!(!disabled.common.gh_enabled(true));
    }

    #[test]
    fn parses_explicit_boolean_forms() {
        let disabled = request(&["list", "--no-hooks", "--allow-unsafe", "--no-gh"]);
        assert!(!disabled.common.hooks_enabled(true));
        assert!(!disabled.common.gh_enabled(true));

        let enabled = request(&["--hooks", "--gh", "list"]);
        assert!(enabled.common.hooks);
        assert!(enabled.common.gh);
        assert!(enabled.common.hooks_enabled(true));
        assert!(enabled.common.gh_enabled(true));

        let disabled_last = request(&[
            "--hooks",
            "--gh",
            "list",
            "--no-hooks",
            "--allow-unsafe",
            "--no-gh",
        ]);
        assert!(!disabled_last.common.hooks_enabled(true));
        assert!(!disabled_last.common.gh_enabled(true));

        let enabled_last = request(&[
            "--no-hooks",
            "--allow-unsafe",
            "--no-gh",
            "list",
            "--hooks",
            "--gh",
        ]);
        assert!(enabled_last.common.hooks_enabled(true));
        assert!(enabled_last.common.gh_enabled(true));
    }

    #[test]
    fn preserves_repeated_fzf_arguments() {
        let parsed = request(&["cd", "--fzf-arg=--ansi", "--fzf-arg", "--nth=1"]);
        assert_eq!(parsed.common.fzf_args, ["--ansi", "--nth=1"]);
    }

    #[test]
    fn toggle_like_option_values_are_not_reinterpreted_as_global_flags() {
        let request = request(&["cd", "--fzf-arg", "--no-hooks", "--fzf-arg", "--gh"]);

        assert!(!request.common.no_hooks);
        assert!(!request.common.gh);
        assert_eq!(request.common.fzf_args, ["--no-hooks", "--gh"]);
    }

    #[test]
    fn rejects_unsafe_hook_disable_and_reserved_fzf_options() {
        for (args, expected_code) in [
            (
                vec!["vw", "list", "--no-hooks"],
                ErrorCode::UnsafeFlagRequired,
            ),
            (
                vec!["vw", "cd", "--fzf-arg=--prompt=override"],
                ErrorCode::InvalidArgument,
            ),
        ] {
            let CliParseResult::Invalid { error, .. } = parse_from(args) else {
                panic!("expected validation failure");
            };
            assert_eq!(error.code, expected_code);
        }

        for argument in ["--no-height", "--no-border"] {
            let result = parse_from(vec![
                OsString::from("vw"),
                OsString::from("cd"),
                OsString::from("--fzf-arg"),
                OsString::from(argument),
            ]);
            match result {
                CliParseResult::Invalid { error, .. } => {
                    assert_eq!(error.code, ErrorCode::InvalidArgument);
                    assert!(error.message.contains("reserved fzf option"));
                }
                outcome => panic!("expected reserved fzf argument rejection, got {outcome:?}"),
            }
        }
    }

    #[test]
    fn preserves_argv_after_double_dash_for_exec_and_invoke() {
        let exec = request(&["exec", "main", "--", "node", "-e", "process.exit(0)"]);
        match exec.command {
            Command::Exec { argv, .. } => {
                assert_eq!(argv, ["node", "-e", "process.exit(0)"].map(OsString::from));
            }
            command => panic!("unexpected command: {command:?}"),
        }

        let invoke = request(&["invoke", "post-switch", "--", "--verbose", "value"]);
        match invoke.command {
            Command::Invoke { argv, .. } => {
                assert_eq!(argv, ["--verbose", "value"].map(OsString::from));
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn help_and_version_are_successful_display_outcomes() {
        for args in [
            vec!["vw"],
            vec!["vw", "--help"],
            vec!["vw", "help", "exec"],
            vec!["vw", "--version"],
            vec!["vw", "-v"],
            vec!["vw", "list", "--version"],
        ] {
            match parse_from(args) {
                CliParseResult::Display(text) => assert!(!text.is_empty()),
                outcome => panic!("expected display outcome, got {outcome:?}"),
            }
        }
    }

    #[test]
    fn reserves_short_v_for_version_and_keeps_verbose_long_only() {
        assert!(matches!(
            parse_from(["vw", "-v"]),
            CliParseResult::Display(_)
        ));

        let CliParseResult::Display(help) = parse_from(["vw", "--help"]) else {
            panic!("expected help display");
        };
        assert!(help.contains("-v, --version"));
        assert!(help.contains("--verbose"));
        assert!(!help.contains("-V, --version"));

        let parsed = request(&["--verbose", "--verbose", "list"]);
        assert_eq!(parsed.common.verbose, 2);
    }

    #[test]
    fn parser_failures_are_typed_invalid_arguments() {
        for args in [
            vec!["vw", "list", "--fallback"],
            vec!["vw", "link", "file", "--no-fallback"],
            vec!["vw", "extract", "--from", "."],
        ] {
            match parse_from(args) {
                CliParseResult::Invalid { error, .. } => {
                    assert_eq!(error.code, ErrorCode::InvalidArgument);
                    assert_eq!(error.exit_code(), 3);
                }
                outcome => panic!("expected invalid outcome, got {outcome:?}"),
            }
        }
    }

    #[test]
    fn unknown_subcommands_have_their_own_error_code() {
        let CliParseResult::Invalid { error, .. } = parse_from(["vw", "not-a-command"]) else {
            panic!("expected invalid outcome");
        };
        assert_eq!(error.code, ErrorCode::UnknownCommand);
        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn invoke_rejects_hook_names_outside_the_validated_namespace() {
        for hook in ["switch", "pre-../escape", "post-a/b", "pre-Switch"] {
            let CliParseResult::Invalid { error, .. } = parse_from(["vw", "invoke", hook]) else {
                panic!("expected invalid hook name: {hook}");
            };
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert_eq!(error.exit_code(), 3);
        }
    }

    #[test]
    fn internal_completion_provider_parses_but_is_hidden_from_help() {
        let parsed = request(&["__complete", "worktrees"]);
        assert_eq!(
            parsed.command,
            Command::CompletionCandidates {
                kind: CompletionCandidateKind::Worktrees,
                commandline: Vec::new(),
            }
        );
        assert!(
            clap_command()
                .get_subcommands()
                .filter(|command| !command.is_hide_set())
                .all(|command| command.get_name() != "__complete")
        );
    }
}
