//! Exact-stash transfer core for `absorb` and `unabsorb`.
//!
//! Callers must hold the repository mutation lock from prepare through rollback/apply. Hooks run
//! after [`stage_transfer`] and before either [`rollback_transfer_after_pre_hook_failure`] or
//! [`apply_transfer`]. Every stash operation is pinned to an object ID; stash stack positions are
//! used only to locate the reflog entry for that exact ID when dropping it.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::app::error_mapper::MapToCliError;
use crate::domain::error::{CliError, ErrorCode};
use crate::domain::path::ValidatedManagedPath;
use crate::domain::repo::RepoContext;
use crate::domain::worktree::{WorktreeSnapshot, WorktreeStatus};
use crate::ports::process::ProcessOutput;
use crate::ports::snapshot::GitSnapshotPort;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferInvocation {
    Interactive,
    NonInteractive {
        allow_agent: bool,
        allow_unsafe: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StashRetention {
    DropAfterApply,
    Keep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferDirection {
    Absorb,
    Unabsorb,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferOptions {
    pub invocation: TransferInvocation,
    /// Managed worktree name relative to the configured managed root (`--from` or `--to`).
    pub requested_worktree: Option<PathBuf>,
    pub retention: StashRetention,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferPlan {
    pub direction: TransferDirection,
    pub repo_root: PathBuf,
    pub branch: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub source_dirty: bool,
    pub retention: StashRetention,
    checkout_primary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedTransfer {
    pub plan: TransferPlan,
    pub stash_oid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferResult {
    pub direction: TransferDirection,
    pub branch: String,
    pub path: PathBuf,
    pub source_path: PathBuf,
    pub stashed: bool,
    pub stash_ref: Option<String>,
}

pub fn prepare_absorb<G>(
    git: &G,
    context: &RepoContext,
    snapshot: &WorktreeSnapshot,
    managed_root: &Path,
    branch: &str,
    options: &TransferOptions,
) -> Result<TransferPlan, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    validate_invocation(TransferDirection::Absorb, options.invocation)?;
    ensure_branch(branch)?;
    if git_worktree_dirty(git, &context.repo_root)? {
        return Err(error(
            ErrorCode::DirtyWorktree,
            "absorb requires clean primary worktree",
            [("repoRoot", json!(context.repo_root))],
        ));
    }
    let source = select_managed_worktree(
        snapshot,
        &context.repo_root,
        managed_root,
        branch,
        options.requested_worktree.as_deref(),
        "--from",
        "source",
    )?;
    let source_dirty = git_worktree_dirty(git, &source.path)?;
    Ok(TransferPlan {
        direction: TransferDirection::Absorb,
        repo_root: context.repo_root.clone(),
        branch: branch.to_owned(),
        source_path: source.path.clone(),
        target_path: context.repo_root.clone(),
        source_dirty,
        retention: options.retention,
        checkout_primary: true,
    })
}

pub fn prepare_unabsorb<G>(
    git: &G,
    context: &RepoContext,
    snapshot: &WorktreeSnapshot,
    managed_root: &Path,
    branch: &str,
    options: &TransferOptions,
) -> Result<TransferPlan, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    validate_invocation(TransferDirection::Unabsorb, options.invocation)?;
    ensure_branch(branch)?;
    let current_branch = current_branch(git, &context.repo_root)?;
    if current_branch != branch {
        return Err(error(
            ErrorCode::InvalidArgument,
            "unabsorb requires primary worktree to be on the target branch",
            [
                ("branch", json!(branch)),
                ("currentBranch", json!(current_branch)),
            ],
        ));
    }
    if !git_worktree_dirty(git, &context.repo_root)? {
        return Err(error(
            ErrorCode::DirtyWorktree,
            "unabsorb requires dirty primary worktree",
            [("repoRoot", json!(context.repo_root))],
        ));
    }
    let target = select_managed_worktree(
        snapshot,
        &context.repo_root,
        managed_root,
        branch,
        options.requested_worktree.as_deref(),
        "--to",
        "target",
    )?;
    if git_worktree_dirty(git, &target.path)? {
        return Err(error(
            ErrorCode::DirtyWorktree,
            "unabsorb requires clean target worktree",
            [("branch", json!(branch)), ("path", json!(target.path))],
        ));
    }
    Ok(TransferPlan {
        direction: TransferDirection::Unabsorb,
        repo_root: context.repo_root.clone(),
        branch: branch.to_owned(),
        source_path: context.repo_root.clone(),
        target_path: target.path.clone(),
        source_dirty: true,
        retention: options.retention,
        checkout_primary: false,
    })
}

/// Reversibly moves source changes into one exact stash before the pre-hook.
///
/// If resolving the new stash OID fails after `stash push`, this function identifies the one new
/// OID relative to the pre-push snapshot and restores it immediately. Apply/drop recovery failures
/// keep the stash object reachable and return its OID in error details.
pub fn stage_transfer<G>(git: &G, plan: TransferPlan) -> Result<StagedTransfer, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    if !plan.source_dirty {
        return Ok(StagedTransfer {
            plan,
            stash_oid: None,
        });
    }
    let before = list_stashes(git, &plan.repo_root)?;
    let message = format!(
        "vde-worktree {} {}",
        direction_name(plan.direction),
        plan.branch
    );
    run_git_checked(
        git,
        &plan.source_path,
        ["stash", "push", "-u", "-m", &message],
    )?;
    match resolve_stash_top(git, &plan.repo_root) {
        Ok(stash_oid) => match created_stash_is_unique(git, &plan.repo_root, &before, &stash_oid) {
            Ok(true) => Ok(StagedTransfer {
                plan,
                stash_oid: Some(stash_oid),
            }),
            Ok(false) => recover_unresolved_stage(
                git,
                &plan,
                &before,
                CliError::new(
                    ErrorCode::InternalError,
                    "stash push did not produce one uniquely identifiable new OID",
                ),
            ),
            Err(primary_error) => recover_unresolved_stage(git, &plan, &before, primary_error),
        },
        Err(primary_error) => recover_unresolved_stage(git, &plan, &before, primary_error),
    }
}

/// Restores source changes after a failed pre-hook and drops only the exact transfer stash.
pub fn rollback_transfer_after_pre_hook_failure<G>(
    git: &G,
    staged: &StagedTransfer,
) -> Result<(), CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let Some(stash_oid) = &staged.stash_oid else {
        return Ok(());
    };
    apply_exact_stash(
        git,
        &staged.plan.source_path,
        stash_oid,
        "failed to auto-restore transfer stash after pre-hook failure",
    )?;
    drop_exact_stash(git, &staged.plan.repo_root, stash_oid)
}

/// Applies only the staged OID. A hook-created stash may change `stash@{0}` without affecting this
/// operation or the subsequent exact-OID drop.
pub fn apply_transfer<G>(git: &G, staged: StagedTransfer) -> Result<TransferResult, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let plan = staged.plan;
    revalidate_before_apply(git, &plan)?;
    if plan.checkout_primary {
        run_git_checked(
            git,
            &plan.repo_root,
            ["checkout", "--ignore-other-worktrees", &plan.branch],
        )?;
    }
    if let Some(stash_oid) = &staged.stash_oid {
        apply_exact_stash(
            git,
            &plan.target_path,
            stash_oid,
            "failed to apply transfer stash to target worktree",
        )?;
    }

    let stash_ref = match (&staged.stash_oid, plan.retention) {
        (Some(stash_oid), StashRetention::DropAfterApply) => {
            drop_exact_stash(git, &plan.repo_root, stash_oid)?;
            None
        }
        (Some(stash_oid), StashRetention::Keep) => Some(
            find_stash_ref(git, &plan.repo_root, stash_oid)?
                .unwrap_or_else(|| stash_oid.to_owned()),
        ),
        (None, _) => None,
    };
    Ok(TransferResult {
        direction: plan.direction,
        branch: plan.branch,
        path: plan.target_path,
        source_path: plan.source_path,
        stashed: staged.stash_oid.is_some(),
        stash_ref,
    })
}

fn revalidate_before_apply<G>(git: &G, plan: &TransferPlan) -> Result<(), CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    if git_worktree_dirty(git, &plan.source_path)? {
        return Err(error(
            ErrorCode::DirtyWorktree,
            "transfer pre-hook left the staged source worktree dirty",
            [
                ("branch", json!(plan.branch)),
                ("path", json!(plan.source_path)),
            ],
        ));
    }
    match plan.direction {
        TransferDirection::Absorb => {
            if git_worktree_dirty(git, &plan.repo_root)? {
                return Err(error(
                    ErrorCode::DirtyWorktree,
                    "absorb pre-hook left the primary worktree dirty",
                    [("repoRoot", json!(plan.repo_root))],
                ));
            }
        }
        TransferDirection::Unabsorb => {
            let current = current_branch(git, &plan.repo_root)?;
            if current != plan.branch {
                return Err(error(
                    ErrorCode::InvalidArgument,
                    "unabsorb primary branch changed after preflight",
                    [
                        ("branch", json!(plan.branch)),
                        ("currentBranch", json!(current)),
                    ],
                ));
            }
            let target_branch = current_branch(git, &plan.target_path)?;
            if target_branch != plan.branch {
                return Err(error(
                    ErrorCode::InvalidArgument,
                    "unabsorb target branch changed after preflight",
                    [
                        ("branch", json!(plan.branch)),
                        ("targetBranch", json!(target_branch)),
                        ("path", json!(plan.target_path)),
                    ],
                ));
            }
            if git_worktree_dirty(git, &plan.target_path)? {
                return Err(error(
                    ErrorCode::DirtyWorktree,
                    "unabsorb pre-hook left the target worktree dirty",
                    [
                        ("branch", json!(plan.branch)),
                        ("path", json!(plan.target_path)),
                    ],
                ));
            }
        }
    }
    Ok(())
}

fn created_stash_is_unique<G>(
    git: &G,
    repo_root: &Path,
    before: &[StashEntry],
    resolved_oid: &str,
) -> Result<bool, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let before_oids = before
        .iter()
        .map(|entry| entry.oid.as_str())
        .collect::<BTreeSet<_>>();
    let after = list_stashes(git, repo_root)?;
    let new_oids = after
        .iter()
        .filter(|entry| !before_oids.contains(entry.oid.as_str()))
        .map(|entry| entry.oid.as_str())
        .collect::<BTreeSet<_>>();
    Ok(new_oids.len() == 1 && new_oids.contains(resolved_oid))
}

fn recover_unresolved_stage<G>(
    git: &G,
    plan: &TransferPlan,
    before: &[StashEntry],
    primary_error: CliError,
) -> Result<StagedTransfer, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let after = match list_stashes(git, &plan.repo_root) {
        Ok(after) => after,
        Err(recovery_error) => {
            return Err(stage_recovery_error(
                primary_error,
                None,
                "stash OID could not be resolved; the new stash remains preserved",
                Some(recovery_error),
            ));
        }
    };
    let before_oids = before
        .iter()
        .map(|entry| entry.oid.as_str())
        .collect::<BTreeSet<_>>();
    let new_oids = after
        .iter()
        .filter(|entry| !before_oids.contains(entry.oid.as_str()))
        .map(|entry| entry.oid.clone())
        .collect::<BTreeSet<_>>();
    let Some(stash_oid) = new_oids.iter().next().filter(|_| new_oids.len() == 1) else {
        return Err(stage_recovery_error(
            primary_error,
            None,
            "stash OID could not be uniquely identified; all stash entries remain preserved",
            None,
        ));
    };
    if let Err(recovery_error) = apply_exact_stash(
        git,
        &plan.source_path,
        stash_oid,
        "failed to restore source after stash OID resolution failure",
    ) {
        return Err(stage_recovery_error(
            primary_error,
            Some(stash_oid.as_str()),
            "source restore failed; exact stash remains preserved",
            Some(recovery_error),
        ));
    }
    if let Err(recovery_error) = drop_exact_stash(git, &plan.repo_root, stash_oid) {
        return Err(stage_recovery_error(
            primary_error,
            Some(stash_oid.as_str()),
            "source was restored but exact stash cleanup failed; both copies remain preserved",
            Some(recovery_error),
        ));
    }
    Err(stage_recovery_error(
        primary_error,
        Some(stash_oid.as_str()),
        "source changes were restored after stash OID resolution failure",
        None,
    ))
}

fn stage_recovery_error(
    mut primary: CliError,
    stash_oid: Option<&str>,
    recovery: &str,
    recovery_error: Option<CliError>,
) -> CliError {
    primary
        .details
        .insert("stageRecovery".to_owned(), json!(recovery));
    primary
        .details
        .insert("stashOid".to_owned(), json!(stash_oid));
    if let Some(recovery_error) = recovery_error {
        primary.details.insert(
            "stageRecoveryError".to_owned(),
            json!({
                "code": recovery_error.code,
                "message": recovery_error.message,
                "details": recovery_error.details,
            }),
        );
    }
    primary
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StashEntry {
    reference: String,
    oid: String,
}

fn list_stashes<G>(git: &G, repo_root: &Path) -> Result<Vec<StashEntry>, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = run_git_checked(git, repo_root, ["stash", "list", "--format=%gd%x09%H"])?;
    let text = String::from_utf8(output.stdout).map_err(|source| {
        error(
            ErrorCode::GitCommandFailed,
            "git stash list returned non-UTF-8 output",
            [("cause", json!(source.to_string()))],
        )
    })?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (reference, oid) = line.trim().split_once('\t').ok_or_else(|| {
                CliError::new(
                    ErrorCode::GitCommandFailed,
                    "git stash list returned malformed output",
                )
            })?;
            if reference.is_empty() || oid.is_empty() {
                return Err(CliError::new(
                    ErrorCode::GitCommandFailed,
                    "git stash list returned an empty reference or OID",
                ));
            }
            Ok(StashEntry {
                reference: reference.to_owned(),
                oid: oid.to_owned(),
            })
        })
        .collect()
}

fn resolve_stash_top<G>(git: &G, repo_root: &Path) -> Result<String, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = run_git_checked(git, repo_root, ["rev-parse", "--verify", "-q", "stash@{0}"])?;
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if oid.is_empty() {
        return Err(CliError::new(
            ErrorCode::InternalError,
            "failed to resolve created stash entry",
        ));
    }
    Ok(oid)
}

fn find_stash_ref<G>(git: &G, repo_root: &Path, stash_oid: &str) -> Result<Option<String>, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    Ok(list_stashes(git, repo_root)?
        .into_iter()
        .find(|entry| entry.oid == stash_oid)
        .map(|entry| entry.reference))
}

fn drop_exact_stash<G>(git: &G, repo_root: &Path, stash_oid: &str) -> Result<(), CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let Some(stash_ref) = find_stash_ref(git, repo_root, stash_oid)? else {
        return Ok(());
    };
    run_git_checked(git, repo_root, ["stash", "drop", &stash_ref])?;
    Ok(())
}

fn apply_exact_stash<G>(git: &G, cwd: &Path, stash_oid: &str, message: &str) -> Result<(), CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = git
        .run_git(cwd, ["stash", "apply", stash_oid])
        .map_err(MapToCliError::map_to_cli_error)?;
    if output.exit_code == Some(0) && !output.timed_out {
        return Ok(());
    }
    Err(error(
        ErrorCode::StashApplyFailed,
        message,
        [
            ("cwd", json!(cwd)),
            ("stashOid", json!(stash_oid)),
            ("stderr", json!(String::from_utf8_lossy(&output.stderr))),
        ],
    ))
}

fn select_managed_worktree<'a>(
    snapshot: &'a WorktreeSnapshot,
    repo_root: &Path,
    managed_root: &Path,
    branch: &str,
    requested_name: Option<&Path>,
    option_name: &'static str,
    role: &'static str,
) -> Result<&'a WorktreeStatus, CliError> {
    let mut candidates = Vec::new();
    for worktree in &snapshot.worktrees {
        if worktree.path == repo_root || worktree.branch.as_deref() != Some(branch) {
            continue;
        }
        let Ok(relative) = worktree.path.strip_prefix(managed_root) else {
            continue;
        };
        if ValidatedManagedPath::validate(managed_root, relative).is_ok() {
            candidates.push((worktree, relative.to_path_buf()));
        }
    }
    if let Some(requested_name) = requested_name {
        let validated = ValidatedManagedPath::validate(managed_root, requested_name)
            .map_err(MapToCliError::map_to_cli_error)?;
        return candidates
            .into_iter()
            .find(|(_, relative)| relative == validated.relative_path())
            .map(|(worktree, _)| worktree)
            .ok_or_else(|| {
                error(
                    ErrorCode::WorktreeNotFound,
                    format!("{role} worktree not found for requested managed name"),
                    [
                        ("branch", json!(branch)),
                        ("worktreeName", json!(requested_name)),
                        ("optionName", json!(option_name)),
                        ("role", json!(role)),
                    ],
                )
            });
    }
    match candidates.as_slice() {
        [] => Err(error(
            ErrorCode::WorktreeNotFound,
            format!("no managed {role} worktree found for branch: {branch}"),
            [("branch", json!(branch)), ("role", json!(role))],
        )),
        [(worktree, _)] => Ok(*worktree),
        _ => Err(error(
            ErrorCode::InvalidArgument,
            format!("multiple managed {role} worktrees found; use {option_name}"),
            [
                ("branch", json!(branch)),
                ("role", json!(role)),
                (
                    "candidates",
                    json!(
                        candidates
                            .iter()
                            .map(|(_, relative)| relative)
                            .collect::<Vec<_>>()
                    ),
                ),
            ],
        )),
    }
}

fn validate_invocation(
    direction: TransferDirection,
    invocation: TransferInvocation,
) -> Result<(), CliError> {
    let TransferInvocation::NonInteractive {
        allow_agent,
        allow_unsafe,
    } = invocation
    else {
        return Ok(());
    };
    let command = direction_name(direction);
    if !allow_agent {
        return Err(CliError::new(
            ErrorCode::UnsafeFlagRequired,
            format!("UNSAFE_FLAG_REQUIRED: {command} in non-TTY requires --allow-agent"),
        ));
    }
    if !allow_unsafe {
        return Err(CliError::new(
            ErrorCode::UnsafeFlagRequired,
            format!("UNSAFE_FLAG_REQUIRED: {command} in non-TTY requires --allow-unsafe"),
        ));
    }
    Ok(())
}

fn ensure_branch(branch: &str) -> Result<(), CliError> {
    if branch.trim().is_empty() {
        return Err(CliError::new(
            ErrorCode::InvalidArgument,
            "branch must be non-empty",
        ));
    }
    Ok(())
}

const fn direction_name(direction: TransferDirection) -> &'static str {
    match direction {
        TransferDirection::Absorb => "absorb",
        TransferDirection::Unabsorb => "unabsorb",
    }
}

fn current_branch<G>(git: &G, repo_root: &Path) -> Result<String, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = run_git_checked(git, repo_root, ["branch", "--show-current"])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_worktree_dirty<G>(git: &G, cwd: &Path) -> Result<bool, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = run_git_checked(git, cwd, ["status", "--porcelain"])?;
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn run_git_checked<G, I, S>(git: &G, cwd: &Path, args: I) -> Result<ProcessOutput, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    let output = git
        .run_git(cwd, &args)
        .map_err(MapToCliError::map_to_cli_error)?;
    if output.exit_code == Some(0) && !output.timed_out {
        return Ok(output);
    }
    Err(git_output_error(cwd, &args, &output))
}

fn git_output_error(cwd: &Path, args: &[OsString], output: &ProcessOutput) -> CliError {
    error(
        ErrorCode::GitCommandFailed,
        "git command failed",
        [
            ("cwd", json!(cwd)),
            (
                "argv",
                json!(
                    args.iter()
                        .map(|arg| arg.to_string_lossy())
                        .collect::<Vec<_>>()
                ),
            ),
            ("exitCode", json!(output.exit_code)),
            ("timedOut", json!(output.timed_out)),
            ("stderr", json!(String::from_utf8_lossy(&output.stderr))),
        ],
    )
}

fn error<const N: usize>(
    code: ErrorCode,
    message: impl Into<String>,
    details: [(&str, Value); N],
) -> CliError {
    CliError::new(code, message).with_details(
        details
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<BTreeMap<_, _>>(),
    )
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Mutex;

    use super::*;
    use crate::adapters::git_cli::{GitCli, GitCliError};
    use crate::adapters::process::StdProcessRunner;
    use crate::domain::worktree::{
        PrState, WorktreeLockState, WorktreeMergedState, WorktreeUpstreamState,
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Fault {
        StashPush,
        ResolveOid,
        StashApplySource,
        StashDrop,
        Checkout,
        StashApplyTarget,
    }

    #[derive(Debug)]
    enum FaultError {
        Git(GitCliError),
        Injected(Fault),
    }

    impl std::fmt::Display for FaultError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for FaultError {}

    impl MapToCliError for FaultError {
        fn map_to_cli_error(self) -> CliError {
            match self {
                Self::Git(source) => source.map_to_cli_error(),
                Self::Injected(fault) => CliError::new(
                    ErrorCode::GitCommandFailed,
                    format!("injected Git failure: {fault:?}"),
                ),
            }
        }
    }

    struct FaultGit {
        inner: GitCli<StdProcessRunner>,
        fault: Fault,
        source_path: PathBuf,
        target_path: PathBuf,
        fired: Mutex<bool>,
    }

    impl FaultGit {
        fn new(fault: Fault, source_path: &Path, target_path: &Path) -> Self {
            Self {
                inner: GitCli::new(StdProcessRunner),
                fault,
                source_path: source_path.to_path_buf(),
                target_path: target_path.to_path_buf(),
                fired: Mutex::new(false),
            }
        }

        fn should_fail(&self, cwd: &Path, args: &[OsString]) -> bool {
            let text = args
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>();
            match self.fault {
                Fault::StashPush => text.starts_with(&["stash".into(), "push".into()]),
                Fault::ResolveOid => text.starts_with(&["rev-parse".into(), "--verify".into()]),
                Fault::StashApplySource => {
                    cwd == self.source_path && text.starts_with(&["stash".into(), "apply".into()])
                }
                Fault::StashDrop => text.starts_with(&["stash".into(), "drop".into()]),
                Fault::Checkout => text.starts_with(&["checkout".into()]),
                Fault::StashApplyTarget => {
                    cwd == self.target_path && text.starts_with(&["stash".into(), "apply".into()])
                }
            }
        }
    }

    impl GitSnapshotPort for FaultGit {
        type Error = FaultError;

        fn run_git<I, S>(&self, cwd: &Path, args: I) -> Result<ProcessOutput, Self::Error>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let args = args
                .into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect::<Vec<_>>();
            let mut fired = self.fired.lock().unwrap();
            if !*fired && self.should_fail(cwd, &args) {
                *fired = true;
                return Err(FaultError::Injected(self.fault));
            }
            drop(fired);
            self.inner.execute(cwd, &args).map_err(FaultError::Git)
        }
    }

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn fixture(branch: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path();
        git(repo, &["init", "-b", "main"]);
        git(repo, &["config", "user.email", "test@example.com"]);
        git(repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("README"), "initial\n").unwrap();
        git(repo, &["add", "README"]);
        git(repo, &["commit", "-m", "initial"]);
        std::fs::write(repo.join(".git/info/exclude"), ".worktree/\n").unwrap();
        let target = repo.join(".worktree").join(branch);
        git(repo, &["branch", branch]);
        git(repo, &["worktree", "add", target.to_str().unwrap(), branch]);
        (directory, target)
    }

    fn status(path: &Path, branch: &str, dirty: bool) -> WorktreeStatus {
        WorktreeStatus {
            branch: Some(branch.to_owned()),
            path: path.to_path_buf(),
            head: "head".to_owned(),
            dirty,
            locked: WorktreeLockState {
                value: false,
                reason: None,
                owner: None,
            },
            merged: WorktreeMergedState {
                by_ancestry: None,
                by_pr: None,
                overall: None,
            },
            pr: PrState::none(),
            upstream: WorktreeUpstreamState {
                ahead: None,
                behind: None,
                remote: None,
            },
        }
    }

    fn snapshot(repo: &Path, branch: &str, linked: &Path) -> WorktreeSnapshot {
        WorktreeSnapshot {
            repo_root: repo.to_path_buf(),
            base_branch: Some("main".to_owned()),
            worktrees: vec![status(repo, "main", false), status(linked, branch, false)],
            warnings: Vec::new(),
        }
    }

    fn context(repo: &Path) -> RepoContext {
        RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: repo.to_path_buf(),
            git_common_dir: repo.join(".git"),
        }
    }

    fn options() -> TransferOptions {
        TransferOptions {
            invocation: TransferInvocation::Interactive,
            requested_worktree: None,
            retention: StashRetention::DropAfterApply,
        }
    }

    fn stash_oids(repo: &Path) -> Vec<String> {
        git(repo, &["stash", "list", "--format=%H"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn prepare_absorb_enforces_authorization_dirty_selection_and_ambiguity() {
        let (fixture, linked) = fixture("feature/transfer");
        let repo = fixture.path();
        let adapter = GitCli::new(StdProcessRunner);
        let snapshot = snapshot(repo, "feature/transfer", &linked);
        let denied = TransferOptions {
            invocation: TransferInvocation::NonInteractive {
                allow_agent: false,
                allow_unsafe: false,
            },
            ..options()
        };
        assert_eq!(
            prepare_absorb(
                &adapter,
                &context(repo),
                &snapshot,
                &repo.join(".worktree"),
                "feature/transfer",
                &denied,
            )
            .unwrap_err()
            .code,
            ErrorCode::UnsafeFlagRequired
        );

        std::fs::write(repo.join("primary-dirty"), "dirty").unwrap();
        assert_eq!(
            prepare_absorb(
                &adapter,
                &context(repo),
                &snapshot,
                &repo.join(".worktree"),
                "feature/transfer",
                &options(),
            )
            .unwrap_err()
            .code,
            ErrorCode::DirtyWorktree
        );
        std::fs::remove_file(repo.join("primary-dirty")).unwrap();

        let missing_selection = TransferOptions {
            requested_worktree: Some(PathBuf::from("feature/missing")),
            ..options()
        };
        assert_eq!(
            prepare_absorb(
                &adapter,
                &context(repo),
                &snapshot,
                &repo.join(".worktree"),
                "feature/transfer",
                &missing_selection,
            )
            .unwrap_err()
            .code,
            ErrorCode::WorktreeNotFound
        );

        let duplicate = repo.join(".worktree/feature/duplicate");
        std::fs::create_dir_all(&duplicate).unwrap();
        let mut ambiguous = snapshot.clone();
        ambiguous
            .worktrees
            .push(status(&duplicate, "feature/transfer", false));
        assert_eq!(
            prepare_absorb(
                &adapter,
                &context(repo),
                &ambiguous,
                &repo.join(".worktree"),
                "feature/transfer",
                &options(),
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn prepare_unabsorb_enforces_branch_primary_target_and_selection_safety() {
        let (fixture, linked) = fixture("feature/transfer");
        let repo = fixture.path();
        let adapter = GitCli::new(StdProcessRunner);
        let snapshot = snapshot(repo, "feature/transfer", &linked);
        assert_eq!(
            prepare_unabsorb(
                &adapter,
                &context(repo),
                &snapshot,
                &repo.join(".worktree"),
                "feature/transfer",
                &options(),
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
        git(
            repo,
            &["checkout", "--ignore-other-worktrees", "feature/transfer"],
        );
        assert_eq!(
            prepare_unabsorb(
                &adapter,
                &context(repo),
                &snapshot,
                &repo.join(".worktree"),
                "feature/transfer",
                &options(),
            )
            .unwrap_err()
            .code,
            ErrorCode::DirtyWorktree
        );

        std::fs::write(repo.join("primary-transfer"), "primary").unwrap();
        std::fs::write(linked.join("target-dirty"), "target").unwrap();
        assert_eq!(
            prepare_unabsorb(
                &adapter,
                &context(repo),
                &snapshot,
                &repo.join(".worktree"),
                "feature/transfer",
                &options(),
            )
            .unwrap_err()
            .code,
            ErrorCode::DirtyWorktree
        );
        let missing_selection = TransferOptions {
            requested_worktree: Some(PathBuf::from("feature/missing")),
            ..options()
        };
        assert_eq!(
            prepare_unabsorb(
                &adapter,
                &context(repo),
                &snapshot,
                &repo.join(".worktree"),
                "feature/transfer",
                &missing_selection,
            )
            .unwrap_err()
            .code,
            ErrorCode::WorktreeNotFound
        );
    }

    #[test]
    fn hook_created_stash_never_changes_the_absorb_oid_or_drop_target() {
        let (fixture, source) = fixture("feature/absorb");
        let repo = fixture.path();
        std::fs::write(source.join("from-source"), "source\n").unwrap();
        let adapter = GitCli::new(StdProcessRunner);
        let selected = TransferOptions {
            requested_worktree: Some(PathBuf::from("feature/absorb")),
            ..options()
        };
        let plan = prepare_absorb(
            &adapter,
            &context(repo),
            &snapshot(repo, "feature/absorb", &source),
            &repo.join(".worktree"),
            "feature/absorb",
            &selected,
        )
        .unwrap();
        let staged = stage_transfer(&adapter, plan).unwrap();
        let transfer_oid = staged.stash_oid.clone().unwrap();

        std::fs::write(repo.join("hook-file"), "hook\n").unwrap();
        git(repo, &["stash", "push", "-u", "-m", "hook stash"]);
        let hook_oid = stash_oids(repo)[0].clone();
        assert_ne!(hook_oid, transfer_oid);

        let result = apply_transfer(&adapter, staged).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.join("from-source")).unwrap(),
            "source\n"
        );
        assert_eq!(result.stash_ref, None);
        assert_eq!(stash_oids(repo), vec![hook_oid]);
    }

    #[test]
    fn hook_created_stash_never_changes_unabsorb_target_oid() {
        let (fixture, target) = fixture("feature/unabsorb");
        let repo = fixture.path();
        git(
            repo,
            &["checkout", "--ignore-other-worktrees", "feature/unabsorb"],
        );
        std::fs::write(repo.join("from-primary"), "primary\n").unwrap();
        let adapter = GitCli::new(StdProcessRunner);
        let selected = TransferOptions {
            requested_worktree: Some(PathBuf::from("feature/unabsorb")),
            ..options()
        };
        let plan = prepare_unabsorb(
            &adapter,
            &context(repo),
            &snapshot(repo, "feature/unabsorb", &target),
            &repo.join(".worktree"),
            "feature/unabsorb",
            &selected,
        )
        .unwrap();
        let staged = stage_transfer(&adapter, plan).unwrap();
        let transfer_oid = staged.stash_oid.clone().unwrap();

        std::fs::write(repo.join("hook-file"), "hook\n").unwrap();
        git(repo, &["stash", "push", "-u", "-m", "hook stash"]);
        let hook_oid = stash_oids(repo)[0].clone();
        assert_ne!(hook_oid, transfer_oid);

        apply_transfer(&adapter, staged).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("from-primary")).unwrap(),
            "primary\n"
        );
        assert_eq!(stash_oids(repo), vec![hook_oid]);
    }

    #[test]
    fn unabsorb_rejects_when_pre_hook_switches_target_branch() {
        let (fixture, target) = fixture("feature/unabsorb-switch");
        let repo = fixture.path();
        git(
            repo,
            &[
                "checkout",
                "--ignore-other-worktrees",
                "feature/unabsorb-switch",
            ],
        );
        std::fs::write(repo.join("from-primary"), "primary\n").unwrap();
        let adapter = GitCli::new(StdProcessRunner);
        let plan = prepare_unabsorb(
            &adapter,
            &context(repo),
            &snapshot(repo, "feature/unabsorb-switch", &target),
            &repo.join(".worktree"),
            "feature/unabsorb-switch",
            &options(),
        )
        .unwrap();
        let staged = stage_transfer(&adapter, plan).unwrap();
        git(&target, &["checkout", "--ignore-other-worktrees", "main"]);

        let error = apply_transfer(&adapter, staged.clone()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.details["targetBranch"], "main");
        assert!(!target.join("from-primary").exists());
        assert!(staged.stash_oid.is_some());
        assert_eq!(stash_oids(repo).len(), 1);
    }

    #[test]
    fn oid_resolution_failure_restores_source_and_removes_only_transfer_stash() {
        let (fixture, source) = fixture("feature/oid-failure");
        let repo = fixture.path();
        std::fs::write(source.join("recover"), "recover\n").unwrap();
        let normal = GitCli::new(StdProcessRunner);
        let plan = prepare_absorb(
            &normal,
            &context(repo),
            &snapshot(repo, "feature/oid-failure", &source),
            &repo.join(".worktree"),
            "feature/oid-failure",
            &options(),
        )
        .unwrap();
        let fault = FaultGit::new(Fault::ResolveOid, &source, repo);
        let error = stage_transfer(&fault, plan).unwrap_err();
        assert!(
            error.details["stageRecovery"]
                .as_str()
                .unwrap()
                .contains("restored")
        );
        assert_eq!(
            std::fs::read_to_string(source.join("recover")).unwrap(),
            "recover\n"
        );
        assert!(stash_oids(repo).is_empty());
    }

    #[test]
    fn stash_push_failure_preserves_source_and_does_not_create_a_stash() {
        let (fixture, source) = fixture("feature/push-failure");
        let repo = fixture.path();
        std::fs::write(source.join("recover"), "recover\n").unwrap();
        let normal = GitCli::new(StdProcessRunner);
        let plan = prepare_absorb(
            &normal,
            &context(repo),
            &snapshot(repo, "feature/push-failure", &source),
            &repo.join(".worktree"),
            "feature/push-failure",
            &options(),
        )
        .unwrap();
        let fault = FaultGit::new(Fault::StashPush, &source, repo);
        let error = stage_transfer(&fault, plan).unwrap_err();
        assert_eq!(error.code, ErrorCode::GitCommandFailed);
        assert_eq!(
            std::fs::read_to_string(source.join("recover")).unwrap(),
            "recover\n"
        );
        assert!(stash_oids(repo).is_empty());
    }

    #[test]
    fn rollback_apply_and_drop_failures_preserve_at_least_one_copy() {
        for fault_kind in [Fault::StashApplySource, Fault::StashDrop] {
            let (fixture, source) = fixture("feature/rollback");
            let repo = fixture.path();
            std::fs::write(source.join("recover"), "recover\n").unwrap();
            let normal = GitCli::new(StdProcessRunner);
            let plan = prepare_absorb(
                &normal,
                &context(repo),
                &snapshot(repo, "feature/rollback", &source),
                &repo.join(".worktree"),
                "feature/rollback",
                &options(),
            )
            .unwrap();
            let staged = stage_transfer(&normal, plan).unwrap();
            let fault = FaultGit::new(fault_kind, &source, repo);
            assert!(rollback_transfer_after_pre_hook_failure(&fault, &staged).is_err());
            let source_copy = source.join("recover").exists();
            let stash_copy = !stash_oids(repo).is_empty();
            assert!(
                source_copy || stash_copy,
                "fault {fault_kind:?} lost all copies"
            );
            if fault_kind == Fault::StashApplySource {
                assert!(!source_copy && stash_copy);
            } else {
                assert!(source_copy && stash_copy);
            }
        }
    }

    #[test]
    fn checkout_target_apply_and_drop_failures_preserve_transfer_changes() {
        for fault_kind in [Fault::Checkout, Fault::StashApplyTarget, Fault::StashDrop] {
            let (fixture, source) = fixture("feature/apply-fault");
            let repo = fixture.path();
            std::fs::write(source.join("payload"), "payload\n").unwrap();
            let normal = GitCli::new(StdProcessRunner);
            let plan = prepare_absorb(
                &normal,
                &context(repo),
                &snapshot(repo, "feature/apply-fault", &source),
                &repo.join(".worktree"),
                "feature/apply-fault",
                &options(),
            )
            .unwrap();
            let staged = stage_transfer(&normal, plan).unwrap();
            let fault = FaultGit::new(fault_kind, &source, repo);
            assert!(apply_transfer(&fault, staged).is_err());
            let target_copy = repo.join("payload").exists();
            let stash_copy = !stash_oids(repo).is_empty();
            assert!(
                target_copy || stash_copy,
                "fault {fault_kind:?} lost all copies"
            );
            if fault_kind == Fault::StashDrop {
                assert!(target_copy && stash_copy);
            } else {
                assert!(!target_copy && stash_copy);
            }
        }
    }
}
