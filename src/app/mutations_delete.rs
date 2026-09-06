//! Plan, revalidation, apply, and state-finalization cores for `del` and `gone`.
//!
//! Callers must hold the repository mutation lock from `prepare_*` through finalization. Hooks
//! may run only between prepare and revalidation. Every destructive boundary rechecks the latest
//! worktree snapshot and the exact metadata records observed during preflight.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::app::error_mapper::MapToCliError;
use crate::domain::error::{CliError, ErrorCode, ExecutionPhase, ExecutionReport, ExecutionState};
use crate::domain::worktree::{WorktreeSnapshot, WorktreeStatus};
use crate::ports::process::ProcessOutput;
use crate::ports::snapshot::GitSnapshotPort;
use crate::state::json_store::JsonRecordState;
use crate::state::lifecycle::{
    WorktreeLifecycleRecord, delete_worktree_lifecycle, read_worktree_lifecycle,
};
use crate::state::worktree_lock::{WorktreeLockRecord, delete_worktree_lock, read_worktree_lock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct DeleteForceOptions {
    pub force_dirty: bool,
    pub allow_unpushed: bool,
    pub force_unmerged: bool,
    pub force_locked: bool,
}

impl DeleteForceOptions {
    pub const fn any(self) -> bool {
        self.force_dirty || self.allow_unpushed || self.force_unmerged || self.force_locked
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteInvocation {
    pub interactive: bool,
    pub allow_unsafe: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MetadataFingerprint {
    lock: JsonRecordState<WorktreeLockRecord>,
    lifecycle: JsonRecordState<WorktreeLifecycleRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelPlan {
    pub repo_root: PathBuf,
    pub managed_worktree_root: PathBuf,
    pub branch: String,
    pub path: PathBuf,
    pub force: DeleteForceOptions,
    /// Exact HEAD certified by the latest merged PR; bound again during revalidation.
    verified_pr_head: Option<String>,
    metadata: MetadataFingerprint,
}

impl DelPlan {
    pub const fn requires_hooks(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevalidatedDelPlan(DelPlan);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct DeleteProgress {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub lifecycle_deleted: bool,
    pub lock_deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelGitApplied {
    plan: DelPlan,
    progress: DeleteProgress,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelResult {
    pub branch: String,
    pub path: PathBuf,
    pub progress: DeleteProgress,
}

/// Side-effect-free `del` preflight. The branch is resolved once so hooks cannot redirect the
/// operation. Invalid lock or lifecycle records always fail before Git, including forced deletes.
pub fn prepare_del(
    repo_root: &Path,
    current_worktree_root: &Path,
    managed_worktree_root: &Path,
    snapshot: &WorktreeSnapshot,
    requested_branch: Option<&str>,
    force: DeleteForceOptions,
    invocation: DeleteInvocation,
) -> Result<DelPlan, CliError> {
    if force.any() && !invocation.interactive && !invocation.allow_unsafe {
        return Err(CliError::new(
            ErrorCode::UnsafeFlagRequired,
            "UNSAFE_FLAG_REQUIRED: force flags in non-TTY mode require --allow-unsafe",
        ));
    }
    let target = match requested_branch {
        Some(branch) if branch.trim().is_empty() => {
            return Err(CliError::new(
                ErrorCode::InvalidArgument,
                "branch must be non-empty",
            ));
        }
        Some(branch) => worktree_by_branch(snapshot, branch)?,
        None => {
            let current = worktree_by_path(snapshot, current_worktree_root)?;
            let branch = validate_delete_target(repo_root, managed_worktree_root, current)?;
            worktree_by_branch(snapshot, branch)?
        }
    };
    let branch = validate_delete_target(repo_root, managed_worktree_root, target)?;
    let metadata = read_valid_metadata(repo_root, branch)?;
    validate_del_safety(target, force)?;
    validate_authoritative_lock(target, &metadata, force.force_locked)?;
    Ok(DelPlan {
        repo_root: repo_root.to_path_buf(),
        managed_worktree_root: managed_worktree_root.to_path_buf(),
        branch: branch.to_owned(),
        path: target.path.clone(),
        force,
        verified_pr_head: verified_pr_head(target),
        metadata,
    })
}

/// Rechecks every safety guard after the pre-hook and binds apply to the original branch/path.
pub fn revalidate_del(
    plan: &DelPlan,
    latest: &WorktreeSnapshot,
) -> Result<RevalidatedDelPlan, CliError> {
    let target = worktree_by_branch(latest, &plan.branch)?;
    if target.path != plan.path {
        return Err(error(
            ErrorCode::SafetyRejected,
            "del target changed after preflight",
            [
                ("branch", json!(plan.branch)),
                ("expectedPath", json!(plan.path)),
                ("actualPath", json!(target.path)),
            ],
        ));
    }
    validate_delete_target(&plan.repo_root, &plan.managed_worktree_root, target)?;
    let current_metadata = read_valid_metadata(&plan.repo_root, &plan.branch)?;
    validate_del_safety(target, plan.force)?;
    validate_authoritative_lock(target, &current_metadata, plan.force.force_locked)?;
    if current_metadata != plan.metadata {
        return Err(error(
            ErrorCode::LockConflict,
            "deletion metadata changed after preflight",
            [("branch", json!(plan.branch)), ("path", json!(plan.path))],
        ));
    }
    let mut refreshed = plan.clone();
    refreshed.verified_pr_head = verified_pr_head(target);
    Ok(RevalidatedDelPlan(refreshed))
}

/// Applies only the Git portion of `del`. A failure includes the completed phases, allowing the
/// caller to report a partially removed worktree without losing recovery information.
pub fn apply_del_git<G>(git: &G, plan: RevalidatedDelPlan) -> Result<DelGitApplied, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let plan = plan.0;
    let mut progress = DeleteProgress::default();
    let mut remove_args = vec![
        OsString::from("worktree"),
        OsString::from("remove"),
        plan.path.as_os_str().to_owned(),
    ];
    if plan.force.force_dirty || plan.force.force_locked {
        remove_args.push(OsString::from("--force"));
    }
    // Git requires force twice to remove a native `git worktree lock`. Safety has already been
    // revalidated, so `force_locked` cannot implicitly bypass the independent dirty guard.
    if plan.force.force_locked {
        remove_args.push(OsString::from("--force"));
    }
    run_git_checked(git, &plan.repo_root, &remove_args)
        .map_err(|error| deletion_failure(error, "worktreeRemove", progress))?;
    progress.worktree_removed = true;

    let delete_mode = if plan.force.any() || plan.verified_pr_head.is_some() {
        "-D"
    } else {
        "-d"
    };
    let branch_args = [
        OsString::from("branch"),
        OsString::from(delete_mode),
        OsString::from(&plan.branch),
    ];
    run_git_checked(git, &plan.repo_root, &branch_args)
        .map_err(|error| deletion_failure(error, "branchDelete", progress))?;
    progress.branch_deleted = true;
    Ok(DelGitApplied { plan, progress })
}

pub trait DeleteMutationState {
    fn delete_lifecycle(&self, repo_root: &Path, branch: &str) -> Result<(), CliError>;
    fn delete_lock(&self, repo_root: &Path, branch: &str) -> Result<(), CliError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemDeleteMutationState;

impl DeleteMutationState for FilesystemDeleteMutationState {
    fn delete_lifecycle(&self, repo_root: &Path, branch: &str) -> Result<(), CliError> {
        delete_worktree_lifecycle(repo_root, branch).map_err(MapToCliError::map_to_cli_error)
    }

    fn delete_lock(&self, repo_root: &Path, branch: &str) -> Result<(), CliError> {
        delete_worktree_lock(repo_root, branch).map_err(MapToCliError::map_to_cli_error)
    }
}

pub fn finalize_del_state(
    applied: DelGitApplied,
    state: &impl DeleteMutationState,
) -> Result<DelResult, CliError> {
    let DelGitApplied { plan, mut progress } = applied;
    state
        .delete_lifecycle(&plan.repo_root, &plan.branch)
        .map_err(|error| deletion_failure(error, "lifecycleDelete", progress))?;
    progress.lifecycle_deleted = true;
    state
        .delete_lock(&plan.repo_root, &plan.branch)
        .map_err(|error| deletion_failure(error, "lockDelete", progress))?;
    progress.lock_deleted = true;
    Ok(DelResult {
        branch: plan.branch,
        path: plan.path,
        progress,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoneCandidate {
    pub branch: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GonePlan {
    pub repo_root: PathBuf,
    pub managed_worktree_root: PathBuf,
    pub dry_run: bool,
    pub candidates: Vec<GoneCandidate>,
}

impl GonePlan {
    pub const fn requires_hooks(&self) -> bool {
        !self.dry_run
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoneFailure {
    pub execution: ExecutionReport,
    pub branch: String,
    pub path: PathBuf,
    pub phase: String,
    pub code: String,
    pub message: String,
    pub details: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoneResult {
    pub dry_run: bool,
    pub candidates: Vec<String>,
    pub deleted: Vec<String>,
    pub failed: Vec<GoneFailure>,
}

/// Git-applied `gone` transaction awaiting metadata finalization.
///
/// The token makes the Git/state boundary explicit for the application pipeline. Candidates whose
/// Git phases failed are already represented in `result.failed`; `pending` contains only candidates
/// whose worktree and branch were removed and whose metadata must still be finalized.
#[derive(Clone, Debug, PartialEq)]
pub struct GoneGitApplied {
    result: GoneResult,
    pending: Vec<DelGitApplied>,
}

/// Builds a stable candidate set. Dirty, locked, non-merged, primary, detached, and unmanaged
/// worktrees are excluded. Corrupt metadata is an error rather than an implicit exclusion.
pub fn prepare_gone(
    repo_root: &Path,
    managed_worktree_root: &Path,
    snapshot: &WorktreeSnapshot,
    dry_run: bool,
) -> Result<GonePlan, CliError> {
    let mut candidates = Vec::new();
    let mut branch_counts = BTreeMap::<&str, usize>::new();
    for worktree in &snapshot.worktrees {
        if let Some(branch) = worktree.branch.as_deref() {
            *branch_counts.entry(branch).or_default() += 1;
        }
    }
    for target in &snapshot.worktrees {
        let Some(branch) = target.branch.as_deref() else {
            continue;
        };
        // A branch checked out by multiple worktrees is never an implicit cleanup candidate.
        // The user must resolve the ambiguity explicitly before deletion can be considered.
        if branch_counts.get(branch) != Some(&1) {
            continue;
        }
        if target.path == repo_root || !path_is_managed(&target.path, managed_worktree_root) {
            continue;
        }
        // Metadata corruption must never be interpreted as permission to delete.
        let metadata = read_valid_metadata(repo_root, branch)?;
        if target.dirty
            || target.locked.value
            || matches!(metadata.lock, JsonRecordState::Valid(_))
            || target.merged.overall != Some(true)
        {
            continue;
        }
        candidates.push(GoneCandidate {
            branch: branch.to_owned(),
            path: target.path.clone(),
        });
    }
    candidates.sort_by(|left, right| {
        left.branch
            .cmp(&right.branch)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(GonePlan {
        repo_root: repo_root.to_path_buf(),
        managed_worktree_root: managed_worktree_root.to_path_buf(),
        dry_run,
        candidates,
    })
}

pub fn gone_dry_run_result(plan: &GonePlan) -> GoneResult {
    GoneResult {
        dry_run: true,
        candidates: candidate_branches(plan),
        deleted: Vec::new(),
        failed: Vec::new(),
    }
}

pub trait GoneSnapshotProvider {
    fn collect_latest(&self, candidate: &GoneCandidate) -> Result<WorktreeSnapshot, CliError>;
}

impl<F> GoneSnapshotProvider for F
where
    F: Fn(&GoneCandidate) -> Result<WorktreeSnapshot, CliError>,
{
    fn collect_latest(&self, candidate: &GoneCandidate) -> Result<WorktreeSnapshot, CliError> {
        self(candidate)
    }
}

/// Applies the Git portion of `gone` one candidate at a time. A fresh snapshot and all guards are
/// checked before each deletion. Failures are collected and the remaining candidates continue.
/// Metadata is deliberately untouched until [`finalize_gone_state`].
pub fn apply_gone_git<G, S>(git: &G, snapshots: &S, plan: &GonePlan) -> GoneGitApplied
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
    S: GoneSnapshotProvider,
{
    if plan.dry_run {
        return GoneGitApplied {
            result: gone_dry_run_result(plan),
            pending: Vec::new(),
        };
    }
    let mut result = GoneResult {
        dry_run: false,
        candidates: candidate_branches(plan),
        deleted: Vec::new(),
        failed: Vec::new(),
    };
    let mut pending = Vec::new();
    for candidate in &plan.candidates {
        let attempt = snapshots
            .collect_latest(candidate)
            .and_then(|latest| revalidate_gone_candidate(plan, candidate, &latest))
            .and_then(|validated| apply_del_git(git, validated));
        match attempt {
            Ok(applied) => pending.push(applied),
            Err(error) => push_gone_failure(&mut result, candidate, error, "revalidation"),
        }
    }
    GoneGitApplied { result, pending }
}

/// Finalizes lifecycle/lock metadata for every candidate whose Git phases succeeded.
/// State failures remain candidate-local and do not prevent later finalizations.
pub fn finalize_gone_state(
    applied: GoneGitApplied,
    state: &impl DeleteMutationState,
) -> GoneResult {
    let GoneGitApplied {
        mut result,
        pending,
    } = applied;
    for candidate in pending {
        let identity = GoneCandidate {
            branch: candidate.plan.branch.clone(),
            path: candidate.plan.path.clone(),
        };
        match finalize_del_state(candidate, state) {
            Ok(deleted) => result.deleted.push(deleted.branch),
            Err(error) => push_gone_failure(&mut result, &identity, error, "stateFinalize"),
        }
    }
    result
}

fn push_gone_failure(
    result: &mut GoneResult,
    candidate: &GoneCandidate,
    error: CliError,
    default_phase: &'static str,
) {
    let error = if error.execution.phase == ExecutionPhase::Unknown {
        error.at_phase(ExecutionPhase::Preflight, ExecutionState::NotStarted, &[])
    } else {
        error
    };
    let phase = error
        .details
        .get("failedPhase")
        .and_then(Value::as_str)
        .unwrap_or(default_phase)
        .to_owned();
    result.failed.push(GoneFailure {
        branch: candidate.branch.clone(),
        path: candidate.path.clone(),
        phase,
        code: error.code.to_string(),
        message: error.message,
        details: error.details,
        execution: error.execution,
    });
}

fn candidate_branches(plan: &GonePlan) -> Vec<String> {
    plan.candidates
        .iter()
        .map(|candidate| candidate.branch.clone())
        .collect()
}

fn revalidate_gone_candidate(
    plan: &GonePlan,
    candidate: &GoneCandidate,
    latest: &WorktreeSnapshot,
) -> Result<RevalidatedDelPlan, CliError> {
    let target = worktree_by_branch(latest, &candidate.branch)?;
    if target.path != candidate.path {
        return Err(error(
            ErrorCode::SafetyRejected,
            "gone target changed after candidate discovery",
            [
                ("branch", json!(candidate.branch)),
                ("expectedPath", json!(candidate.path)),
                ("actualPath", json!(target.path)),
            ],
        ));
    }
    validate_delete_target(&plan.repo_root, &plan.managed_worktree_root, target)?;
    if target.dirty {
        return Err(delete_guard_error(ErrorCode::DirtyWorktree, target));
    }
    if target.locked.value {
        return Err(delete_guard_error(ErrorCode::LockedWorktree, target));
    }
    if target.merged.overall != Some(true) {
        return Err(delete_guard_error(ErrorCode::UnmergedWorktree, target));
    }
    let metadata = read_valid_metadata(&plan.repo_root, &candidate.branch)?;
    validate_authoritative_lock(target, &metadata, false)?;
    Ok(RevalidatedDelPlan(DelPlan {
        repo_root: plan.repo_root.clone(),
        managed_worktree_root: plan.managed_worktree_root.clone(),
        branch: candidate.branch.clone(),
        path: candidate.path.clone(),
        force: DeleteForceOptions::default(),
        verified_pr_head: verified_pr_head(target),
        metadata,
    }))
}

fn verified_pr_head(target: &WorktreeStatus) -> Option<String> {
    (target.merged.by_pr == Some(true)
        && target.pr.head_oid.as_deref() == Some(target.head.as_str()))
    .then(|| target.head.clone())
}

fn validate_delete_target<'a>(
    repo_root: &Path,
    managed_worktree_root: &Path,
    target: &'a WorktreeStatus,
) -> Result<&'a str, CliError> {
    let branch = target.branch.as_deref().ok_or_else(|| {
        error(
            ErrorCode::DetachedHead,
            "cannot delete detached worktree without branch",
            [("path", json!(target.path))],
        )
    })?;
    if target.path == repo_root {
        return Err(error(
            ErrorCode::InvalidArgument,
            "cannot delete the primary worktree",
            [("path", json!(target.path))],
        ));
    }
    if !path_is_managed(&target.path, managed_worktree_root) {
        return Err(error(
            ErrorCode::WorktreeNotFound,
            "target branch is not in managed worktree root",
            [
                ("branch", json!(branch)),
                ("path", json!(target.path)),
                ("managedWorktreeRoot", json!(managed_worktree_root)),
            ],
        ));
    }
    Ok(branch)
}

/// Enumerate independently observable rejection reasons without mutating metadata.
pub fn inspect_delete_guards(
    repo_root: &Path,
    managed_root: &Path,
    snapshot: &WorktreeSnapshot,
    target: &WorktreeStatus,
    mut force: DeleteForceOptions,
    gone: bool,
) -> Vec<CliError> {
    let mut errors = Vec::new();
    if let Err(error) = validate_delete_target(repo_root, managed_root, target) {
        errors.push(error);
    }
    if let Some(branch) = &target.branch {
        if let Err(error) = worktree_by_branch(snapshot, branch) {
            errors.push(error);
        }
        if let Err(error) = read_valid_metadata(repo_root, branch) {
            errors.push(error);
        }
    }
    if gone {
        force.allow_unpushed = true;
    }
    errors.extend(del_safety_errors(target, force));
    errors
}

fn del_safety_errors(target: &WorktreeStatus, force: DeleteForceOptions) -> Vec<CliError> {
    [
        (target.dirty && !force.force_dirty, ErrorCode::DirtyWorktree),
        (
            target.locked.value && !force.force_locked,
            ErrorCode::LockedWorktree,
        ),
        (
            target.merged.overall != Some(true) && !force.force_unmerged,
            ErrorCode::UnmergedWorktree,
        ),
        (
            target.upstream.ahead.is_none_or(|ahead| ahead > 0) && !force.allow_unpushed,
            ErrorCode::UnpushedWorktree,
        ),
    ]
    .into_iter()
    .filter(|(rejected, _)| *rejected)
    .map(|(_, code)| delete_guard_error(code, target))
    .collect()
}

fn validate_del_safety(target: &WorktreeStatus, force: DeleteForceOptions) -> Result<(), CliError> {
    let errors = del_safety_errors(target, force);
    if let Some(mut first) = errors.first().cloned() {
        first.details.insert(
            "rejections".to_owned(),
            json!(
                errors
                    .iter()
                    .map(crate::presentation::json::ErrorPayload::from)
                    .collect::<Vec<_>>()
            ),
        );
        return Err(first);
    }
    Ok(())
}

fn delete_guard_error(code: ErrorCode, target: &WorktreeStatus) -> CliError {
    let message = match code {
        ErrorCode::DirtyWorktree => "worktree has uncommitted changes",
        ErrorCode::LockedWorktree => "worktree is locked",
        ErrorCode::UnmergedWorktree => "worktree is not merged (or merge state is unknown)",
        ErrorCode::UnpushedWorktree => "worktree has unpushed commits (or push state is unknown)",
        _ => "worktree deletion was rejected",
    };
    error(
        code,
        message,
        [
            ("branch", json!(target.branch)),
            ("path", json!(target.path)),
            ("locked", json!(target.locked)),
            ("merged", json!(target.merged)),
            ("upstream", json!(target.upstream)),
        ],
    )
}

fn validate_authoritative_lock(
    target: &WorktreeStatus,
    metadata: &MetadataFingerprint,
    force_locked: bool,
) -> Result<(), CliError> {
    if matches!(metadata.lock, JsonRecordState::Valid(_)) && !force_locked {
        return Err(delete_guard_error(ErrorCode::LockedWorktree, target));
    }
    Ok(())
}

fn read_valid_metadata(repo_root: &Path, branch: &str) -> Result<MetadataFingerprint, CliError> {
    let lock = read_worktree_lock(repo_root, branch);
    if let JsonRecordState::Invalid { reason } = &lock.state {
        return Err(error(
            ErrorCode::LockConflict,
            "lock metadata is invalid; deletion is refused",
            [
                ("branch", json!(branch)),
                ("path", json!(lock.path)),
                ("reason", json!(reason)),
            ],
        ));
    }
    let lifecycle = read_worktree_lifecycle(repo_root, branch);
    if let JsonRecordState::Invalid { reason } = &lifecycle.state {
        return Err(error(
            ErrorCode::LockConflict,
            "lifecycle metadata is invalid; deletion is refused",
            [
                ("branch", json!(branch)),
                ("path", json!(lifecycle.path)),
                ("reason", json!(reason)),
            ],
        ));
    }
    Ok(MetadataFingerprint {
        lock: lock.state,
        lifecycle: lifecycle.state,
    })
}

fn worktree_by_branch<'a>(
    snapshot: &'a WorktreeSnapshot,
    branch: &str,
) -> Result<&'a WorktreeStatus, CliError> {
    crate::app::target::resolve(&snapshot.worktrees, Some(branch), None, &snapshot.repo_root)
}

fn worktree_by_path<'a>(
    snapshot: &'a WorktreeSnapshot,
    path: &Path,
) -> Result<&'a WorktreeStatus, CliError> {
    snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.path == path)
        .ok_or_else(|| {
            error(
                ErrorCode::WorktreeNotFound,
                "no worktree found for current location",
                [("currentWorktreeRoot", json!(path))],
            )
        })
}

fn path_is_managed(path: &Path, managed_root: &Path) -> bool {
    path != managed_root && path.starts_with(managed_root)
}

fn run_git_checked<G>(git: &G, cwd: &Path, args: &[OsString]) -> Result<ProcessOutput, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = git
        .run_git(cwd, args)
        .map_err(MapToCliError::map_to_cli_error)?;
    if output.exit_code == Some(0) && !output.timed_out {
        return Ok(output);
    }
    Err(
        CliError::new(ErrorCode::GitCommandFailed, "git command failed").with_details(
            BTreeMap::from([
                ("cwd".to_owned(), json!(cwd)),
                (
                    "argv".to_owned(),
                    json!(
                        args.iter()
                            .map(|argument| argument.to_string_lossy())
                            .collect::<Vec<_>>()
                    ),
                ),
                ("exitCode".to_owned(), json!(output.exit_code)),
                ("timedOut".to_owned(), json!(output.timed_out)),
                (
                    "stdout".to_owned(),
                    json!(String::from_utf8_lossy(&output.stdout)),
                ),
                (
                    "stderr".to_owned(),
                    json!(String::from_utf8_lossy(&output.stderr)),
                ),
            ]),
        ),
    )
}

fn deletion_failure(
    mut error: CliError,
    failed_phase: &'static str,
    progress: DeleteProgress,
) -> CliError {
    error
        .details
        .insert("failedPhase".to_owned(), json!(failed_phase));
    error.details.insert("progress".to_owned(), json!(progress));
    for (step, done) in [
        ("worktreeRemove", progress.worktree_removed),
        ("branchDelete", progress.branch_deleted),
        ("lifecycleDelete", progress.lifecycle_deleted),
        ("lockDelete", progress.lock_deleted),
    ] {
        if done {
            error.execution.completed.push(step.to_owned());
        }
    }
    let state = if error.execution.completed.is_empty() {
        ExecutionState::Unknown
    } else {
        ExecutionState::Partial
    };
    let phase = if matches!(failed_phase, "lifecycleDelete" | "lockDelete") {
        ExecutionPhase::Finalize
    } else {
        ExecutionPhase::Apply
    };
    error
        .at_phase(phase, state, &[])
        .with_recovery("remainingStep", json!(failed_phase))
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
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::fs;
    use std::process::Command;
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::*;
    use crate::adapters::git_cli::GitCli;
    use crate::adapters::process::StdProcessRunner;
    use crate::domain::worktree::{
        PrState, WorktreeLockState, WorktreeMergedState, WorktreeUpstreamState,
    };

    #[derive(Debug)]
    struct FakeGitError;

    impl fmt::Display for FakeGitError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("fake Git error")
        }
    }

    impl std::error::Error for FakeGitError {}

    impl MapToCliError for FakeGitError {
        fn map_to_cli_error(self) -> CliError {
            CliError::new(ErrorCode::GitCommandFailed, self.to_string())
        }
    }

    #[derive(Default)]
    struct RecordingGit {
        calls: Mutex<Vec<Vec<OsString>>>,
        fail_call: Option<usize>,
    }

    impl GitSnapshotPort for RecordingGit {
        type Error = FakeGitError;

        fn run_git<I, S>(&self, _cwd: &Path, args: I) -> Result<ProcessOutput, Self::Error>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            let args = args
                .into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect::<Vec<_>>();
            let mut calls = self.calls.lock().unwrap();
            calls.push(args);
            let exit_code = if self.fail_call == Some(calls.len()) {
                Some(1)
            } else {
                Some(0)
            };
            Ok(ProcessOutput {
                exit_code,
                timed_out: false,
                stdout: Vec::new(),
                stderr: b"injected".to_vec(),
                ..Default::default()
            })
        }
    }

    #[derive(Default)]
    struct FaultState {
        fail_lifecycle: bool,
        fail_lock: bool,
    }

    impl DeleteMutationState for FaultState {
        fn delete_lifecycle(&self, _repo_root: &Path, _branch: &str) -> Result<(), CliError> {
            if self.fail_lifecycle {
                Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected lifecycle failure",
                ))
            } else {
                Ok(())
            }
        }

        fn delete_lock(&self, _repo_root: &Path, _branch: &str) -> Result<(), CliError> {
            if self.fail_lock {
                Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected lock failure",
                ))
            } else {
                Ok(())
            }
        }
    }

    fn status(branch: Option<&str>, path: &Path) -> WorktreeStatus {
        WorktreeStatus {
            branch: branch.map(str::to_owned),
            path: path.to_path_buf(),
            head: "abc".to_owned(),
            dirty: false,
            locked: WorktreeLockState {
                value: false,
                reason: None,
                owner: None,
            },
            merged: WorktreeMergedState {
                by_ancestry: Some(true),
                by_pr: None,
                overall: Some(true),
            },
            pr: PrState::none(),
            upstream: WorktreeUpstreamState {
                ahead: Some(0),
                behind: Some(0),
                remote: Some("origin/topic".to_owned()),
            },
        }
    }

    fn snapshot(repo: &Path, worktrees: Vec<WorktreeStatus>) -> WorktreeSnapshot {
        WorktreeSnapshot {
            repo_root: repo.to_path_buf(),
            base_branch: Some("main".to_owned()),
            worktrees,
            warnings: Vec::new(),
        }
    }

    fn plan_fixture() -> (TempDir, PathBuf, PathBuf, WorktreeSnapshot) {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let managed = repo.join(".worktree");
        let target = managed.join("topic");
        fs::create_dir_all(&target).unwrap();
        let snapshot = snapshot(
            &repo,
            vec![status(Some("main"), &repo), status(Some("topic"), &target)],
        );
        (temp, repo, managed, snapshot)
    }

    fn safe_plan() -> (TempDir, DelPlan, WorktreeSnapshot) {
        let (temp, repo, managed, snapshot) = plan_fixture();
        let plan = prepare_del(
            &repo,
            &managed.join("topic"),
            &managed,
            &snapshot,
            Some("topic"),
            DeleteForceOptions::default(),
            DeleteInvocation {
                interactive: true,
                allow_unsafe: false,
            },
        )
        .unwrap();
        (temp, plan, snapshot)
    }

    #[test]
    fn del_rejects_each_safety_guard_and_unknown_states() {
        let (_temp, repo, managed, base) = plan_fixture();
        let target_path = managed.join("topic");
        for (expected, mutate) in [
            (ErrorCode::DirtyWorktree, 0_u8),
            (ErrorCode::LockedWorktree, 1),
            (ErrorCode::UnmergedWorktree, 2),
            (ErrorCode::UnpushedWorktree, 3),
            (ErrorCode::UnmergedWorktree, 4),
            (ErrorCode::UnpushedWorktree, 5),
        ] {
            let mut current = base.clone();
            let target = &mut current.worktrees[1];
            match mutate {
                0 => target.dirty = true,
                1 => target.locked.value = true,
                2 => target.merged.overall = Some(false),
                3 => target.upstream.ahead = Some(1),
                4 => target.merged.overall = None,
                5 => target.upstream.ahead = None,
                _ => unreachable!(),
            }
            let error = prepare_del(
                &repo,
                &target_path,
                &managed,
                &current,
                Some("topic"),
                DeleteForceOptions::default(),
                DeleteInvocation {
                    interactive: true,
                    allow_unsafe: false,
                },
            )
            .unwrap_err();
            assert_eq!(error.code, expected);
        }
    }

    #[test]
    fn each_delete_force_option_bypasses_only_its_matching_guard() {
        for (index, matching_force, unrelated_force, expected_without_match) in [
            (
                0,
                DeleteForceOptions {
                    force_dirty: true,
                    ..DeleteForceOptions::default()
                },
                DeleteForceOptions {
                    allow_unpushed: true,
                    ..DeleteForceOptions::default()
                },
                ErrorCode::DirtyWorktree,
            ),
            (
                1,
                DeleteForceOptions {
                    force_locked: true,
                    ..DeleteForceOptions::default()
                },
                DeleteForceOptions {
                    force_dirty: true,
                    ..DeleteForceOptions::default()
                },
                ErrorCode::LockedWorktree,
            ),
            (
                2,
                DeleteForceOptions {
                    force_unmerged: true,
                    ..DeleteForceOptions::default()
                },
                DeleteForceOptions {
                    force_dirty: true,
                    ..DeleteForceOptions::default()
                },
                ErrorCode::UnmergedWorktree,
            ),
            (
                3,
                DeleteForceOptions {
                    allow_unpushed: true,
                    ..DeleteForceOptions::default()
                },
                DeleteForceOptions {
                    force_dirty: true,
                    ..DeleteForceOptions::default()
                },
                ErrorCode::UnpushedWorktree,
            ),
        ] {
            let (_temp, repo, managed, mut current) = plan_fixture();
            let target = &mut current.worktrees[1];
            match index {
                0 => target.dirty = true,
                1 => target.locked.value = true,
                2 => target.merged.overall = Some(false),
                3 => target.upstream.ahead = Some(1),
                _ => unreachable!(),
            }
            let invocation = DeleteInvocation {
                interactive: true,
                allow_unsafe: false,
            };
            let allowed = prepare_del(
                &repo,
                &managed.join("topic"),
                &managed,
                &current,
                Some("topic"),
                matching_force,
                invocation,
            );
            assert!(allowed.is_ok(), "matching force at index {index}");
            let denied = prepare_del(
                &repo,
                &managed.join("topic"),
                &managed,
                &current,
                Some("topic"),
                unrelated_force,
                invocation,
            )
            .unwrap_err();
            assert_eq!(denied.code, expected_without_match);
        }
    }

    #[test]
    fn force_locked_uses_git_double_force_without_bypassing_dirty_preflight() {
        let (_temp, repo, managed, mut current) = plan_fixture();
        current.worktrees[1].locked.value = true;
        let force = DeleteForceOptions {
            force_locked: true,
            ..DeleteForceOptions::default()
        };
        let invocation = DeleteInvocation {
            interactive: true,
            allow_unsafe: false,
        };
        let plan = prepare_del(
            &repo,
            &managed.join("topic"),
            &managed,
            &current,
            Some("topic"),
            force,
            invocation,
        )
        .unwrap();
        let git = RecordingGit::default();
        apply_del_git(&git, revalidate_del(&plan, &current).unwrap()).unwrap();
        let calls = git.calls.lock().unwrap();
        assert_eq!(
            calls[0],
            [
                OsString::from("worktree"),
                OsString::from("remove"),
                managed.join("topic").into_os_string(),
                OsString::from("--force"),
                OsString::from("--force"),
            ]
        );

        current.worktrees[1].dirty = true;
        let error = prepare_del(
            &repo,
            &managed.join("topic"),
            &managed,
            &current,
            Some("topic"),
            force,
            invocation,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::DirtyWorktree);
    }

    #[test]
    fn del_rejects_primary_detached_unmanaged_and_non_tty_force() {
        let (_temp, repo, managed, mut current) = plan_fixture();
        let invocation = DeleteInvocation {
            interactive: true,
            allow_unsafe: false,
        };
        let primary = prepare_del(
            &repo,
            &repo,
            &managed,
            &current,
            Some("main"),
            DeleteForceOptions::default(),
            invocation,
        )
        .unwrap_err();
        assert_eq!(primary.code, ErrorCode::InvalidArgument);

        current.worktrees[1].branch = None;
        let detached = prepare_del(
            &repo,
            &managed.join("topic"),
            &managed,
            &current,
            None,
            DeleteForceOptions::default(),
            invocation,
        )
        .unwrap_err();
        assert_eq!(detached.code, ErrorCode::DetachedHead);

        current.worktrees[1] = status(Some("topic"), &repo.join("elsewhere"));
        let unmanaged = prepare_del(
            &repo,
            &repo.join("elsewhere"),
            &managed,
            &current,
            Some("topic"),
            DeleteForceOptions::default(),
            invocation,
        )
        .unwrap_err();
        assert_eq!(unmanaged.code, ErrorCode::WorktreeNotFound);

        let unsafe_error = prepare_del(
            &repo,
            &repo.join("elsewhere"),
            &managed,
            &current,
            Some("topic"),
            DeleteForceOptions {
                force_dirty: true,
                ..DeleteForceOptions::default()
            },
            DeleteInvocation {
                interactive: false,
                allow_unsafe: false,
            },
        )
        .unwrap_err();
        assert_eq!(unsafe_error.code, ErrorCode::UnsafeFlagRequired);
    }

    #[test]
    fn del_rejects_ambiguous_branch_and_gone_excludes_shared_branch() {
        let (_temp, repo, managed, mut current) = plan_fixture();
        current
            .worktrees
            .push(status(Some("topic"), &managed.join("topic-copy")));
        let error = prepare_del(
            &repo,
            &managed.join("topic"),
            &managed,
            &current,
            Some("topic"),
            DeleteForceOptions::default(),
            DeleteInvocation {
                interactive: true,
                allow_unsafe: false,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.details["candidates"].as_array().unwrap().len(), 2);

        let implicit_error = prepare_del(
            &repo,
            &managed.join("topic"),
            &managed,
            &current,
            None,
            DeleteForceOptions::default(),
            DeleteInvocation {
                interactive: true,
                allow_unsafe: false,
            },
        )
        .unwrap_err();
        assert_eq!(implicit_error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            implicit_error.details["candidates"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let gone = prepare_gone(&repo, &managed, &current, true).unwrap();
        assert!(
            gone.candidates
                .iter()
                .all(|candidate| candidate.branch != "topic")
        );
    }

    #[test]
    fn gone_revalidation_rejects_branch_that_becomes_ambiguous() {
        let (_temp, repo, managed, current) = plan_fixture();
        let plan = prepare_gone(&repo, &managed, &current, false).unwrap();
        let mut latest = current.clone();
        latest
            .worktrees
            .push(status(Some("topic"), &managed.join("topic-copy")));
        let result = finalize_gone_state(
            apply_gone_git(
                &RecordingGit::default(),
                &|_: &GoneCandidate| Ok(latest.clone()),
                &plan,
            ),
            &FaultState::default(),
        );
        assert!(result.deleted.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].code, "INVALID_ARGUMENT");
        assert_eq!(result.failed[0].phase, "revalidation");
    }

    #[test]
    fn metadata_change_during_hook_window_is_a_lock_conflict() {
        let (_temp, plan, snapshot) = safe_plan();
        fs::create_dir_all(plan.repo_root.join(".vde/worktree/locks")).unwrap();
        fs::write(
            crate::state::worktree_lock::worktree_lock_file_path(&plan.repo_root, "topic"),
            b"not-json",
        )
        .unwrap();
        let error = revalidate_del(&plan, &snapshot).unwrap_err();
        assert_eq!(error.code, ErrorCode::LockConflict);
    }

    #[test]
    fn invalid_lifecycle_is_rejected_before_git_even_with_force() {
        let (_temp, repo, managed, current) = plan_fixture();
        let path = crate::state::lifecycle::lifecycle_file_path(&repo, "topic");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"not-json").unwrap();
        let error = prepare_del(
            &repo,
            &managed.join("topic"),
            &managed,
            &current,
            Some("topic"),
            DeleteForceOptions {
                force_dirty: true,
                allow_unpushed: true,
                force_unmerged: true,
                force_locked: true,
            },
            DeleteInvocation {
                interactive: false,
                allow_unsafe: true,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::LockConflict);
    }

    #[test]
    fn del_faults_report_exact_completed_phase() {
        let (_temp, plan, snapshot) = safe_plan();
        let validated = revalidate_del(&plan, &snapshot).unwrap();
        let error = apply_del_git(
            &RecordingGit {
                fail_call: Some(2),
                ..RecordingGit::default()
            },
            validated,
        )
        .unwrap_err();
        assert_eq!(error.details["failedPhase"], "branchDelete");
        assert_eq!(error.execution.state, ExecutionState::Partial);
        assert_eq!(error.execution.completed, ["worktreeRemove"]);
        assert_eq!(error.execution.recovery["remainingStep"], "branchDelete");
        assert_eq!(error.details["progress"]["worktreeRemoved"], true);
        assert_eq!(error.details["progress"]["branchDeleted"], false);

        let (_temp, plan, snapshot) = safe_plan();
        let applied = apply_del_git(
            &RecordingGit::default(),
            revalidate_del(&plan, &snapshot).unwrap(),
        )
        .unwrap();
        let error = finalize_del_state(
            applied,
            &FaultState {
                fail_lifecycle: true,
                fail_lock: false,
            },
        )
        .unwrap_err();
        assert_eq!(error.details["failedPhase"], "lifecycleDelete");
        assert_eq!(error.execution.phase, ExecutionPhase::Finalize);
        assert_eq!(
            error.execution.completed,
            ["worktreeRemove", "branchDelete"]
        );
        assert_eq!(error.details["progress"]["branchDeleted"], true);
        assert_eq!(error.details["progress"]["lifecycleDeleted"], false);

        let (_temp, plan, snapshot) = safe_plan();
        let applied = apply_del_git(
            &RecordingGit::default(),
            revalidate_del(&plan, &snapshot).unwrap(),
        )
        .unwrap();
        let error = finalize_del_state(
            applied,
            &FaultState {
                fail_lifecycle: false,
                fail_lock: true,
            },
        )
        .unwrap_err();
        assert_eq!(error.details["failedPhase"], "lockDelete");
        assert_eq!(error.details["progress"]["lifecycleDeleted"], true);
        assert_eq!(error.details["progress"]["lockDeleted"], false);
    }

    #[test]
    fn gone_is_stably_sorted_excludes_unsafe_and_dry_run_has_no_hooks() {
        let (_temp, repo, managed, mut current) = plan_fixture();
        let a = managed.join("a");
        let z = managed.join("z");
        let dirty = managed.join("dirty");
        let locked = managed.join("locked");
        current.worktrees.extend([
            status(Some("z"), &z),
            status(Some("a"), &a),
            status(Some("dirty"), &dirty),
            status(Some("locked"), &locked),
            status(Some("external"), &repo.join("external")),
            status(None, &managed.join("detached")),
        ]);
        current.worktrees[4].dirty = true;
        current.worktrees[5].locked.value = true;
        let plan = prepare_gone(&repo, &managed, &current, true).unwrap();
        assert!(!plan.requires_hooks());
        assert_eq!(
            plan.candidates
                .iter()
                .map(|candidate| candidate.branch.as_str())
                .collect::<Vec<_>>(),
            ["a", "topic", "z"]
        );
        let result = gone_dry_run_result(&plan);
        assert!(result.deleted.is_empty());
        assert!(result.failed.is_empty());
    }

    #[test]
    fn gone_revalidates_each_candidate_and_continues_after_failure() {
        let (_temp, repo, managed, current) = plan_fixture();
        let second_path = managed.join("z");
        let mut initial = current.clone();
        initial.worktrees.push(status(Some("z"), &second_path));
        let plan = prepare_gone(&repo, &managed, &initial, false).unwrap();
        assert!(plan.requires_hooks());
        let calls = Mutex::new(0_u8);
        let provider = |_: &GoneCandidate| {
            let mut calls = calls.lock().unwrap();
            *calls += 1;
            let mut latest = initial.clone();
            if *calls == 1 {
                latest
                    .worktrees
                    .iter_mut()
                    .find(|worktree| worktree.branch.as_deref() == Some("topic"))
                    .unwrap()
                    .dirty = true;
            }
            Ok(latest)
        };
        let result = finalize_gone_state(
            apply_gone_git(&RecordingGit::default(), &provider, &plan),
            &FaultState::default(),
        );
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].branch, "topic");
        assert_eq!(result.failed[0].code, "DIRTY_WORKTREE");
        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.deleted[0], "z");
    }

    #[test]
    fn gone_collects_git_failure_and_continues() {
        let (_temp, repo, managed, mut current) = plan_fixture();
        let second_path = managed.join("z");
        current.worktrees.push(status(Some("z"), &second_path));
        let plan = prepare_gone(&repo, &managed, &current, false).unwrap();
        let result = finalize_gone_state(
            apply_gone_git(
                &RecordingGit {
                    fail_call: Some(1),
                    ..RecordingGit::default()
                },
                &|_: &GoneCandidate| Ok(current.clone()),
                &plan,
            ),
            &FaultState::default(),
        );
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.failed[0].details["failedPhase"], "worktreeRemove");
    }

    #[test]
    fn gone_exposes_git_to_state_boundary_and_reports_state_failure_phase() {
        let (_temp, repo, managed, current) = plan_fixture();
        let plan = prepare_gone(&repo, &managed, &current, false).unwrap();
        let git_applied = apply_gone_git(
            &RecordingGit::default(),
            &|_: &GoneCandidate| Ok(current.clone()),
            &plan,
        );
        assert!(git_applied.result.deleted.is_empty());
        assert!(git_applied.result.failed.is_empty());
        assert_eq!(git_applied.pending.len(), 1);
        assert!(git_applied.pending[0].progress.branch_deleted);
        assert!(!git_applied.pending[0].progress.lifecycle_deleted);

        let result = finalize_gone_state(
            git_applied,
            &FaultState {
                fail_lifecycle: true,
                fail_lock: false,
            },
        );
        assert!(result.deleted.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].branch, "topic");
        assert_eq!(result.failed[0].phase, "lifecycleDelete");
        assert_eq!(result.failed[0].details["progress"]["branchDeleted"], true);
        assert_eq!(
            result.failed[0].details["progress"]["lifecycleDeleted"],
            false
        );
    }

    #[test]
    fn gone_failure_schema_covers_every_git_and_state_phase() {
        for (fail_call, expected_phase) in [(1, "worktreeRemove"), (2, "branchDelete")] {
            let (_temp, repo, managed, current) = plan_fixture();
            let plan = prepare_gone(&repo, &managed, &current, false).unwrap();
            let result = finalize_gone_state(
                apply_gone_git(
                    &RecordingGit {
                        fail_call: Some(fail_call),
                        ..RecordingGit::default()
                    },
                    &|_: &GoneCandidate| Ok(current.clone()),
                    &plan,
                ),
                &FaultState::default(),
            );
            assert_eq!(result.failed[0].phase, expected_phase);
            let serialized = serde_json::to_value(&result.failed[0]).unwrap();
            for key in ["branch", "path", "phase", "code", "message", "details"] {
                assert!(serialized.get(key).is_some(), "missing {key}");
            }
        }

        for (state, expected_phase) in [
            (
                FaultState {
                    fail_lifecycle: true,
                    fail_lock: false,
                },
                "lifecycleDelete",
            ),
            (
                FaultState {
                    fail_lifecycle: false,
                    fail_lock: true,
                },
                "lockDelete",
            ),
        ] {
            let (_temp, repo, managed, current) = plan_fixture();
            let plan = prepare_gone(&repo, &managed, &current, false).unwrap();
            let result = finalize_gone_state(
                apply_gone_git(
                    &RecordingGit::default(),
                    &|_: &GoneCandidate| Ok(current.clone()),
                    &plan,
                ),
                &state,
            );
            assert_eq!(result.failed[0].phase, expected_phase);
        }
    }

    fn run(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn real_git_del_removes_worktree_branch_and_metadata() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let managed = repo.join(".worktree");
        let target = managed.join("topic");
        fs::create_dir_all(&repo).unwrap();
        run(&repo, &["init", "-b", "main"]);
        fs::write(repo.join("README"), "base\n").unwrap();
        run(&repo, &["add", "README"]);
        run(&repo, &["commit", "-m", "base"]);
        run(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "topic",
                target.to_str().unwrap(),
                "main",
            ],
        );
        let current = snapshot(
            &repo,
            vec![status(Some("main"), &repo), status(Some("topic"), &target)],
        );
        let plan = prepare_del(
            &repo,
            &target,
            &managed,
            &current,
            Some("topic"),
            DeleteForceOptions::default(),
            DeleteInvocation {
                interactive: true,
                allow_unsafe: false,
            },
        )
        .unwrap();
        let git = GitCli::new(StdProcessRunner);
        let result = finalize_del_state(
            apply_del_git(&git, revalidate_del(&plan, &current).unwrap()).unwrap(),
            &FilesystemDeleteMutationState,
        )
        .unwrap();
        assert_eq!(result.branch, "topic");
        assert!(!target.exists());
        let output = git
            .run_git(&repo, ["show-ref", "--verify", "refs/heads/topic"])
            .unwrap();
        assert_ne!(output.exit_code, Some(0));
    }
}
