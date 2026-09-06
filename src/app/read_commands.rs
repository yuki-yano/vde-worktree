use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::adapters::fzf::{FzfAdapter, FzfRequest, FzfSelection};
use crate::app::dispatch::CommandOutput;
use crate::app::error_mapper::MapToCliError;
use crate::app::result::TerminalCapabilities;
use crate::app::snapshot::{
    SnapshotCollectionOptions, SnapshotCollector, read_registry, resolve_base_branch,
    resolve_distance_from_base,
};
use crate::app::target;
use crate::cli::{Command, ParsedRequest};
use crate::domain::error::{CliError, ErrorCode};
use crate::domain::repo::RepoContext;
use crate::domain::worktree::{
    PrStatus, SnapshotWarning, SnapshotWarningCode, WorktreeSnapshot, WorktreeStatus,
};
use crate::ports::process::ProcessRunner;
use crate::ports::snapshot::{GitSnapshotPort, PrStateLookup};
use crate::presentation::picker::{
    PickerMergedState, PickerWorktree, build_picker_candidates, home_relative_path,
};
use crate::presentation::table::{
    ListTableRow, MergedCellState, PrCellState, TableRenderOptions, render_table,
};
use crate::presentation::theme::ColorPolicy;
use crate::state::config::ResolvedConfig;

pub struct ReadCommandRuntime<'a, G, P, R> {
    pub git: &'a G,
    pub pr_lookup: &'a P,
    pub fzf: &'a FzfAdapter<R>,
    pub terminal: TerminalCapabilities,
    pub home: Option<&'a Path>,
    pub in_tmux: bool,
}

pub fn execute_read_command<G, P, R>(
    request: &ParsedRequest,
    context: &RepoContext,
    config: &ResolvedConfig,
    runtime: &ReadCommandRuntime<'_, G, P, R>,
) -> Option<Result<CommandOutput, CliError>>
where
    G: GitSnapshotPort,
    P: PrStateLookup,
    R: ProcessRunner,
{
    if !matches!(
        request.command,
        Command::List { .. } | Command::Status { .. } | Command::Path { .. } | Command::Cd
    ) {
        return None;
    }
    Some(execute(request, context, config, runtime))
}

fn execute<G, P, R>(
    request: &ParsedRequest,
    context: &RepoContext,
    config: &ResolvedConfig,
    runtime: &ReadCommandRuntime<'_, G, P, R>,
) -> Result<CommandOutput, CliError>
where
    G: GitSnapshotPort,
    P: PrStateLookup,
    R: ProcessRunner,
{
    let mut registry =
        read_registry(runtime.git, &context.repo_root).map_err(MapToCliError::map_to_cli_error)?;
    if let Command::Status { branch } | Command::Path { branch } = &request.command {
        let selected = target::resolve(
            &registry,
            branch.as_deref(),
            request.common.worktree.as_deref(),
            &context.current_worktree_root,
        )?
        .clone();
        target::ensure_path(&context.repo_root)?;
        target::ensure_path(&selected.path)?;
        if matches!(request.command, Command::Path { .. }) {
            return Ok(CommandOutput {
                data: json!({ "branch": selected.branch, "path": selected.path }),
                human_stdout: format!("{}\n", selected.path.display()),
                human_stderr: String::new(),
                partial_error: None,
                warnings: Vec::new(),
            });
        }
        registry = vec![selected];
    }
    let base_branch = resolve_base_branch(
        runtime.git,
        &context.repo_root,
        config.git.base_branch.as_deref(),
        &config.git.base_remote,
    )
    .map_err(MapToCliError::map_to_cli_error)?;
    let collection_options = match request.command {
        Command::List { monitor: true } => SnapshotCollectionOptions::monitor(),
        _ => SnapshotCollectionOptions::default(),
    };
    let snapshot = SnapshotCollector::new(runtime.git, runtime.pr_lookup)
        .with_options(collection_options)
        .collect_registry(
            &context.repo_root,
            &base_branch,
            request.common.gh_enabled(config.github.enabled),
            &registry,
        )
        .map_err(MapToCliError::map_to_cli_error)?;
    ensure_json_representable_paths(context, &snapshot)?;
    let warning_text = render_warnings(&snapshot.warnings);

    let mut output = match &request.command {
        Command::List { .. } => {
            list_output(request, context, config, runtime, &snapshot, warning_text)
        }
        Command::Status { .. } => Ok(status_output(
            &snapshot.worktrees[0],
            runtime.home,
            warning_text,
        )),
        Command::Cd => cd_output(request, context, config, runtime, &snapshot, warning_text),
        _ => unreachable!("read command was checked before snapshot collection"),
    }?;
    output
        .warnings
        .extend(snapshot.warnings.iter().map(|warning| {
            CliError::new(
                match warning.code {
                    SnapshotWarningCode::InvalidLifecycle | SnapshotWarningCode::InvalidLock => {
                        ErrorCode::InvalidMetadata
                    }
                    SnapshotWarningCode::LifecycleObservationFailed => ErrorCode::InternalError,
                },
                &warning.message,
            )
            .with_details(std::collections::BTreeMap::from([
                ("warningCode".to_owned(), json!(warning.code)),
                ("branch".to_owned(), json!(warning.branch)),
                ("path".to_owned(), json!(warning.path)),
            ]))
        }));
    Ok(output)
}

fn ensure_json_representable_paths(
    context: &RepoContext,
    snapshot: &WorktreeSnapshot,
) -> Result<(), CliError> {
    for path in std::iter::once(&context.repo_root)
        .chain(snapshot.worktrees.iter().map(|worktree| &worktree.path))
    {
        target::ensure_path(path)?;
    }
    Ok(())
}

fn list_output<G, P, R>(
    request: &ParsedRequest,
    context: &RepoContext,
    config: &ResolvedConfig,
    runtime: &ReadCommandRuntime<'_, G, P, R>,
    snapshot: &WorktreeSnapshot,
    warning_text: String,
) -> Result<CommandOutput, CliError>
where
    G: GitSnapshotPort,
    P: PrStateLookup,
    R: ProcessRunner,
{
    let base_branch = snapshot
        .base_branch
        .as_deref()
        .expect("collector always records the resolved base branch");
    let managed_worktree_root =
        resolve_configured_path(&context.repo_root, Path::new(&config.paths.worktree_root));
    let data = json!({
        "baseBranch": snapshot.base_branch,
        "managedWorktreeRoot": managed_worktree_root,
        "worktrees": snapshot.worktrees,
    });
    if request.common.json {
        return Ok(CommandOutput {
            data,
            human_stdout: String::new(),
            human_stderr: warning_text,
            partial_error: None,
            warnings: Vec::new(),
        });
    }

    let mut rows = Vec::with_capacity(snapshot.worktrees.len());
    for worktree in &snapshot.worktrees {
        let target_ref = worktree.branch.as_deref().unwrap_or(&worktree.head);
        let (ahead, behind) =
            resolve_distance_from_base(runtime.git, &context.repo_root, base_branch, target_ref)
                .map_err(MapToCliError::map_to_cli_error)?;
        rows.push(to_table_row(
            worktree,
            base_branch,
            &context.current_worktree_root,
            runtime.home,
            ahead,
            behind,
        ));
    }
    let color = ColorPolicy {
        stream_is_terminal: runtime.terminal.stdout_tty,
        json: false,
        no_color: runtime.terminal.no_color,
    };
    let rendered = render_table(
        &rows,
        &TableRenderOptions {
            columns: config.list.table.columns.clone(),
            terminal_width: runtime.terminal.stdout_columns.map(usize::from),
            path_truncate: config.list.table.path.truncate,
            path_min_width: usize::from(config.list.table.path.min_width),
            full_path: request.common.full_path,
            color,
        },
    );
    Ok(CommandOutput {
        data,
        human_stdout: format!("{}\n", rendered.styled),
        human_stderr: warning_text,
        partial_error: None,
        warnings: Vec::new(),
    })
}

fn status_output(
    worktree: &WorktreeStatus,
    home: Option<&Path>,
    warning_text: String,
) -> CommandOutput {
    CommandOutput {
        data: json!({ "worktree": worktree }),
        human_stdout: format!(
            "branch: {}\npath: {}\ndirty: {}\nlocked: {}\n",
            worktree.branch.as_deref().unwrap_or("(detached)"),
            home_relative_path(&worktree.path, home),
            worktree.dirty,
            worktree.locked.value,
        ),
        human_stderr: warning_text,
        partial_error: None,
        warnings: Vec::new(),
    }
}

fn cd_output<G, P, R>(
    request: &ParsedRequest,
    context: &RepoContext,
    config: &ResolvedConfig,
    runtime: &ReadCommandRuntime<'_, G, P, R>,
    snapshot: &WorktreeSnapshot,
    warning_text: String,
) -> Result<CommandOutput, CliError>
where
    G: GitSnapshotPort,
    P: PrStateLookup,
    R: ProcessRunner,
{
    let base_branch = snapshot.base_branch.as_deref();
    let color = ColorPolicy {
        stream_is_terminal: runtime.terminal.stderr_tty,
        json: false,
        no_color: runtime.terminal.no_color,
    };
    let picker_worktrees = snapshot
        .worktrees
        .iter()
        .map(|worktree| to_picker_worktree(worktree, base_branch, &context.current_worktree_root))
        .collect::<Vec<_>>();
    let candidates = build_picker_candidates(&picker_worktrees, runtime.home, color);
    let prompt = request
        .common
        .prompt
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(&config.selector.cd.prompt);
    let mut extra_args = config.selector.cd.fzf.extra_args.clone();
    extra_args.extend(request.common.fzf_args.iter().cloned());
    let selection = runtime
        .fzf
        .select_path(&FzfRequest {
            candidates: &candidates,
            cwd: &context.repo_root,
            prompt,
            surface: config.selector.cd.surface,
            tmux_popup_opts: &config.selector.cd.tmux_popup_opts,
            extra_args: &extra_args,
            stderr_is_terminal: runtime.terminal.stderr_tty,
            in_tmux: runtime.in_tmux,
        })
        .map_err(MapToCliError::map_to_cli_error)?;
    match selection {
        FzfSelection::Selected(path) => Ok(CommandOutput {
            data: json!({ "path": path }),
            human_stdout: format!("{}\n", path.display()),
            human_stderr: warning_text,
            partial_error: None,
            warnings: Vec::new(),
        }),
        FzfSelection::Cancelled => Err(CliError::new(ErrorCode::Cancelled, "selection cancelled")),
    }
}

fn to_table_row(
    worktree: &WorktreeStatus,
    base_branch: &str,
    current_worktree_root: &Path,
    home: Option<&Path>,
    ahead: Option<i64>,
    behind: Option<i64>,
) -> ListTableRow {
    let is_base = worktree.branch.as_deref() == Some(base_branch);
    ListTableRow {
        branch: worktree.branch.clone(),
        current: worktree.path == current_worktree_root,
        dirty: worktree.dirty,
        merged: if is_base {
            MergedCellState::Base
        } else {
            match worktree.merged.overall {
                Some(true) => MergedCellState::Merged,
                Some(false) => MergedCellState::Unmerged,
                None => MergedCellState::Unknown,
            }
        },
        pr: if is_base {
            PrCellState::Base
        } else {
            match worktree.pr.status {
                Some(PrStatus::None) => PrCellState::None,
                Some(PrStatus::Open) => PrCellState::Open,
                Some(PrStatus::Merged) => PrCellState::Merged,
                Some(PrStatus::ClosedUnmerged) => PrCellState::ClosedUnmerged,
                Some(PrStatus::Unknown) | None => PrCellState::Unknown,
            }
        },
        locked: worktree.locked.value,
        ahead,
        behind,
        path: home_relative_path(&worktree.path, home),
    }
}

fn to_picker_worktree(
    worktree: &WorktreeStatus,
    base_branch: Option<&str>,
    current_worktree_root: &Path,
) -> PickerWorktree {
    let is_base = worktree.branch.as_deref() == base_branch;
    PickerWorktree {
        branch: worktree.branch.clone(),
        path: worktree.path.clone(),
        current: worktree.path == current_worktree_root,
        dirty: worktree.dirty,
        merged: if is_base {
            PickerMergedState::Base
        } else {
            match worktree.merged.overall {
                Some(true) => PickerMergedState::Merged,
                Some(false) => PickerMergedState::Unmerged,
                None => PickerMergedState::Unknown,
            }
        },
        locked: worktree.locked.value,
        remote: worktree.upstream.remote.clone(),
        ahead: worktree.upstream.ahead,
        behind: worktree.upstream.behind,
        lock_owner: worktree.locked.owner.clone(),
        lock_reason: worktree.locked.reason.clone(),
    }
}

fn resolve_configured_path(repo_root: &Path, configured: &Path) -> PathBuf {
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        repo_root.join(configured)
    }
}

fn render_warnings(warnings: &[SnapshotWarning]) -> String {
    warnings.iter().fold(String::new(), |mut output, warning| {
        let _ = writeln!(
            output,
            "Warning [{}]: {}",
            warning.code_string(),
            warning.message
        );
        output
    })
}

trait WarningCodeString {
    fn code_string(&self) -> &'static str;
}

impl WarningCodeString for SnapshotWarning {
    fn code_string(&self) -> &'static str {
        use crate::domain::worktree::SnapshotWarningCode;
        match self.code {
            SnapshotWarningCode::InvalidLifecycle => "INVALID_LIFECYCLE",
            SnapshotWarningCode::LifecycleObservationFailed => "LIFECYCLE_OBSERVATION_FAILED",
            SnapshotWarningCode::InvalidLock => "INVALID_LOCK",
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    use super::*;

    #[test]
    fn non_utf8_worktree_paths_are_typed_instead_of_panicking_during_json_building() {
        let invalid_path = PathBuf::from(OsString::from_vec(b"/repo/worktree-\xff".to_vec()));
        let context = RepoContext {
            repo_root: PathBuf::from("/repo"),
            current_worktree_root: PathBuf::from("/repo"),
            git_common_dir: PathBuf::from("/repo/.git"),
        };
        let snapshot = WorktreeSnapshot {
            repo_root: context.repo_root.clone(),
            base_branch: Some("main".to_owned()),
            worktrees: vec![WorktreeStatus {
                branch: Some("feature/non-utf8".to_owned()),
                path: invalid_path,
                head: "abc".to_owned(),
                dirty: false,
                locked: crate::domain::worktree::WorktreeLockState {
                    value: false,
                    reason: None,
                    owner: None,
                },
                merged: crate::domain::worktree::WorktreeMergedState {
                    by_ancestry: None,
                    by_pr: None,
                    overall: None,
                },
                pr: crate::domain::worktree::PrState::unknown(),
                upstream: crate::domain::worktree::WorktreeUpstreamState {
                    ahead: None,
                    behind: None,
                    remote: None,
                },
            }],
            warnings: Vec::new(),
        };

        let error = ensure_json_representable_paths(&context, &snapshot)
            .expect_err("non-UTF-8 path must be rejected before serde_json::json!");
        assert_eq!(error.code, ErrorCode::UnsupportedRepositoryLayout);
        assert_eq!(error.exit_code(), 4);
    }

    #[test]
    fn control_characters_are_rejected_before_path_or_cd_can_break_the_one_line_contract() {
        let context = RepoContext {
            repo_root: PathBuf::from("/repo"),
            current_worktree_root: PathBuf::from("/repo"),
            git_common_dir: PathBuf::from("/repo/.git"),
        };
        let mut snapshot = WorktreeSnapshot {
            repo_root: context.repo_root.clone(),
            base_branch: Some("main".to_owned()),
            worktrees: Vec::new(),
            warnings: Vec::new(),
        };
        for path in ["/repo/line\nbreak", "/repo/escape\u{1b}[31m"] {
            snapshot.worktrees = vec![WorktreeStatus {
                branch: Some("feature/control".to_owned()),
                path: PathBuf::from(path),
                head: "abc".to_owned(),
                dirty: false,
                locked: crate::domain::worktree::WorktreeLockState {
                    value: false,
                    reason: None,
                    owner: None,
                },
                merged: crate::domain::worktree::WorktreeMergedState {
                    by_ancestry: None,
                    by_pr: None,
                    overall: None,
                },
                pr: crate::domain::worktree::PrState::unknown(),
                upstream: crate::domain::worktree::WorktreeUpstreamState {
                    ahead: None,
                    behind: None,
                    remote: None,
                },
            }];
            let error = ensure_json_representable_paths(&context, &snapshot)
                .expect_err("control character path must be rejected");
            assert_eq!(error.code, ErrorCode::UnsupportedRepositoryLayout);
        }
    }
}
