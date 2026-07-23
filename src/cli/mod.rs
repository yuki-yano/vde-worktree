use std::ffi::OsString;
use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};

use crate::domain::error::{CliError, ErrorCode};
use crate::domain::fzf::validate_fzf_extra_args;
use crate::domain::hook::HookName;
use crate::domain::safety::{CommonSafetyPolicy, enforce_common_safety};

pub const COMMAND_NAMES: [&str; 23] = [
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
    #[arg(long, global = true)]
    pub json: bool,

    #[arg(long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    #[arg(
        short = 'v',
        long,
        global = true,
        action = ArgAction::Version,
        required = false
    )]
    version: Option<bool>,

    #[arg(long, global = true, overrides_with = "no_hooks")]
    pub hooks: bool,

    #[arg(long = "no-hooks", global = true, overrides_with = "hooks")]
    pub no_hooks: bool,

    #[arg(long, global = true, overrides_with = "no_gh")]
    pub gh: bool,

    #[arg(long = "no-gh", global = true, overrides_with = "gh")]
    pub no_gh: bool,

    #[arg(long, global = true)]
    pub full_path: bool,

    #[arg(long, global = true)]
    pub allow_unsafe: bool,

    #[arg(long, global = true)]
    pub strict_post_hooks: bool,

    #[arg(long, global = true, value_parser = clap::value_parser!(u64).range(1..))]
    pub hook_timeout_ms: Option<u64>,

    #[arg(long, global = true, value_parser = clap::value_parser!(u64).range(1..))]
    pub lock_timeout_ms: Option<u64>,

    #[arg(long, global = true)]
    pub prompt: Option<String>,

    #[arg(
        long = "fzf-arg",
        global = true,
        action = ArgAction::Append,
        allow_hyphen_values = true
    )]
    pub fzf_args: Vec<String>,
}

impl CommonOptions {
    pub const fn hooks_enabled(&self) -> bool {
        !self.no_hooks
    }

    pub const fn gh_enabled(&self) -> bool {
        !self.no_gh
    }
}

#[derive(Debug, Clone, PartialEq, Subcommand)]
pub enum Command {
    /// Initialize repository-local vde-worktree state.
    Init,
    /// List worktrees and status metadata.
    List {
        /// Emit a lightweight internal snapshot for monitor integrations.
        #[arg(
            long,
            requires_all = ["json", "no_gh"],
            conflicts_with = "gh"
        )]
        monitor: bool,
    },
    /// Show a single worktree status.
    Status { branch: Option<String> },
    /// Print the absolute path for a branch worktree.
    Path { branch: String },
    /// Reuse or create a worktree for a branch.
    Switch { branch: String },
    /// Create a branch and its worktree.
    New { branch: Option<String> },
    /// Rename the current linked worktree branch.
    Mv { new_branch: String },
    /// Delete a linked worktree and branch.
    Del {
        branch: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        force_dirty: bool,
        #[arg(long)]
        allow_unpushed: bool,
        #[arg(long)]
        force_unmerged: bool,
        #[arg(long)]
        force_locked: bool,
    },
    /// Find or delete stale merged worktrees.
    Gone {
        #[arg(long, conflicts_with = "dry_run")]
        apply: bool,
        #[arg(long, conflicts_with = "apply")]
        dry_run: bool,
    },
    /// Find or move unmanaged worktrees into the managed root.
    Adopt {
        #[arg(long, conflicts_with = "dry_run")]
        apply: bool,
        #[arg(long, conflicts_with = "apply")]
        dry_run: bool,
    },
    /// Fetch and attach a remote branch.
    Get { remote_branch: String },
    /// Extract the current primary branch into the managed root.
    Extract {
        #[arg(long, required = true)]
        current: bool,
        #[arg(long)]
        stash: bool,
    },
    /// Transfer linked worktree changes into the primary worktree.
    Absorb {
        branch: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        keep_stash: bool,
        #[arg(long)]
        allow_agent: bool,
    },
    /// Transfer primary worktree changes into a linked worktree.
    Unabsorb {
        branch: String,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        keep_stash: bool,
        #[arg(long)]
        allow_agent: bool,
    },
    /// Check out a branch in the primary worktree.
    Use {
        branch: String,
        #[arg(long)]
        allow_agent: bool,
        #[arg(long)]
        allow_shared: bool,
    },
    /// Run an argv command in a branch worktree.
    Exec {
        branch: String,
        #[arg(last = true, required = true, num_args = 1.., allow_hyphen_values = true)]
        argv: Vec<OsString>,
    },
    /// Invoke a named hook.
    Invoke {
        hook: HookName,
        #[arg(last = true, num_args = 0.., allow_hyphen_values = true)]
        argv: Vec<OsString>,
    },
    /// Copy repository-relative paths into the target worktree.
    Copy {
        #[arg(required = true, num_args = 1..)]
        paths: Vec<PathBuf>,
    },
    /// Link repository-relative paths into the target worktree.
    Link {
        #[arg(required = true, num_args = 1..)]
        paths: Vec<PathBuf>,
    },
    /// Protect a worktree with persistent lock metadata.
    Lock {
        branch: String,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Remove persistent lock metadata.
    Unlock {
        branch: String,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Select a worktree path interactively.
    Cd,
    /// Generate or install shell completions.
    Completion {
        shell: CompletionShell,
        #[arg(long)]
        install: bool,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Internal completion candidate provider.
    #[command(skip)]
    CompletionCandidates { kind: CompletionCandidateKind },
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
            Self::CompletionCandidates { .. } => "__complete",
        }
    }
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
    Cli::command()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRequest {
    pub common: CommonOptions,
    pub command: Command,
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
    if args.get(1).is_some_and(|value| value == "__complete") {
        return parse_completion_candidates(&args);
    }
    let hooks_enabled = last_toggle(&args, "--hooks", "--no-hooks");
    let gh_enabled = last_toggle(&args, "--gh", "--no-gh");
    match Cli::try_parse_from(args) {
        Ok(mut cli) => {
            apply_toggle(
                &mut cli.common.hooks,
                &mut cli.common.no_hooks,
                hooks_enabled,
            );
            apply_toggle(&mut cli.common.gh, &mut cli.common.no_gh, gh_enabled);
            if let Err(error) = validate_common_options(&cli.common) {
                let rendered = format!("error: {}\n", error.message);
                return CliParseResult::Invalid { error, rendered };
            }
            CliParseResult::Parsed(ParsedRequest {
                common: cli.common,
                command: cli.command,
            })
        }
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | ErrorKind::DisplayVersion
            | ErrorKind::MissingSubcommand => CliParseResult::Display(error.to_string()),
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

fn parse_completion_candidates(args: &[OsString]) -> CliParseResult {
    let parsed = args
        .get(2)
        .and_then(|value| value.to_str())
        .and_then(|value| CompletionCandidateKind::from_str(value, true).ok());
    if args.len() == 3
        && let Some(kind) = parsed
    {
        return CliParseResult::Parsed(ParsedRequest {
            common: CommonOptions::default(),
            command: Command::CompletionCandidates { kind },
        });
    }
    let error = CliError::new(
        ErrorCode::InvalidArgument,
        "internal completion provider requires exactly one valid candidate kind",
    );
    CliParseResult::Invalid {
        rendered: format!("error: {}\n", error.message),
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

fn last_toggle(args: &[OsString], positive: &str, negative: &str) -> Option<bool> {
    const VALUE_OPTIONS: [&str; 4] = [
        "--hook-timeout-ms",
        "--lock-timeout-ms",
        "--prompt",
        "--fzf-arg",
    ];

    let mut result = None;
    let mut skip_value = false;
    for arg in args.iter().skip(1) {
        if arg.as_os_str() == std::ffi::OsStr::new("--") {
            break;
        }
        if skip_value {
            skip_value = false;
            continue;
        }
        let value = arg.to_string_lossy();
        if VALUE_OPTIONS.contains(&value.as_ref()) {
            skip_value = true;
            continue;
        }
        if value == positive {
            result = Some(true);
        } else if value == negative {
            result = Some(false);
        }
    }
    result
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
    fn parses_all_twenty_three_command_variants() {
        let cases: [&[&str]; 23] = [
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
        ];

        let parsed_names: Vec<_> = cases
            .iter()
            .map(|args| request(args).command.name())
            .collect();
        assert_eq!(parsed_names, COMMAND_NAMES);
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
        assert!(parsed.common.hooks_enabled());
        assert!(parsed.common.gh_enabled());
        assert!(parsed.common.full_path);
        assert!(parsed.common.allow_unsafe);
        assert!(parsed.common.strict_post_hooks);
        assert_eq!(parsed.common.hook_timeout_ms, Some(101));
        assert_eq!(parsed.common.lock_timeout_ms, Some(202));
        assert_eq!(parsed.common.prompt.as_deref(), Some("pick> "));
        assert_eq!(parsed.common.fzf_args, ["--ansi", "--nth=1"]);

        let disabled = request(&["list", "--no-hooks", "--allow-unsafe", "--no-gh"]);
        assert!(!disabled.common.hooks_enabled());
        assert!(!disabled.common.gh_enabled());
    }

    #[test]
    fn parses_explicit_boolean_forms() {
        let disabled = request(&["list", "--no-hooks", "--allow-unsafe", "--no-gh"]);
        assert!(!disabled.common.hooks_enabled());
        assert!(!disabled.common.gh_enabled());

        let enabled = request(&["--hooks", "--gh", "list"]);
        assert!(enabled.common.hooks);
        assert!(enabled.common.gh);
        assert!(enabled.common.hooks_enabled());
        assert!(enabled.common.gh_enabled());

        let disabled_last = request(&[
            "--hooks",
            "--gh",
            "list",
            "--no-hooks",
            "--allow-unsafe",
            "--no-gh",
        ]);
        assert!(!disabled_last.common.hooks_enabled());
        assert!(!disabled_last.common.gh_enabled());

        let enabled_last = request(&[
            "--no-hooks",
            "--allow-unsafe",
            "--no-gh",
            "list",
            "--hooks",
            "--gh",
        ]);
        assert!(enabled_last.common.hooks_enabled());
        assert!(enabled_last.common.gh_enabled());
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
    fn internal_completion_provider_parses_but_is_absent_from_public_clap_definition() {
        let parsed = request(&["__complete", "worktrees"]);
        assert_eq!(
            parsed.command,
            Command::CompletionCandidates {
                kind: CompletionCandidateKind::Worktrees,
            }
        );
        assert!(
            clap_command()
                .get_subcommands()
                .all(|command| command.get_name() != "__complete")
        );
    }
}
