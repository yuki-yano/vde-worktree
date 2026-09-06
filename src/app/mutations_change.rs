//! Plan/apply cores for mutation commands that change an existing worktree.
//!
//! The `prepare_*` functions are intentionally side-effect free. The repository mutation lock
//! must be held by the caller from prepare through apply, including hook execution between them.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::app::error_mapper::MapToCliError;
use crate::domain::error::{CliError, ErrorCode, ExecutionPhase, ExecutionState};
use crate::domain::path::{ValidatedManagedPath, ValidatedPathOperationError};
use crate::domain::repo::RepoContext;
use crate::domain::worktree::{WorktreeSnapshot, WorktreeStatus};
use crate::ports::process::ProcessOutput;
use crate::ports::snapshot::GitSnapshotPort;
use crate::state::json_store::JsonRecordState;
use crate::state::lifecycle::{
    LifecycleObservationGuard, merge_lifecycle_observation, read_worktree_lifecycle,
};
use crate::state::metadata_transaction::{
    MetadataRenameOutcome, MetadataRenameRequest, MetadataTransactionError, PreparedMetadataRename,
    commit_metadata_rename_locked, mark_metadata_rename_branch_renamed,
    mark_metadata_rename_worktree_moved, prepare_metadata_rename, refresh_staged_lifecycle,
    rollback_staged_metadata_rename, stage_metadata_rename,
};
use crate::state::worktree_lock::{
    WorktreeLockRecord, WorktreeLockUpdate, delete_worktree_lock, read_worktree_lock,
    upsert_worktree_lock, worktree_lock_file_path,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MvResult {
    pub branch: String,
    pub path: PathBuf,
    pub metadata: Option<MetadataRenameOutcome>,
}

#[derive(Clone, Debug)]
pub enum MvPlan {
    Noop(MvResult),
    Apply(Box<MvApplyPlan>),
}

#[derive(Clone, Debug)]
pub struct MvApplyPlan {
    pub old_branch: String,
    pub new_branch: String,
    pub current_path: PathBuf,
    pub target_path: PathBuf,
    validated_target: ValidatedManagedPath,
    metadata: PreparedMetadataRename,
}

#[derive(Clone, Debug)]
pub enum StagedMvPlan {
    Noop(MvResult),
    Apply(Box<MvApplyPlan>),
}

#[derive(Debug)]
pub enum MvGitApplied {
    Noop(MvResult),
    Apply {
        plan: Box<MvApplyPlan>,
        target_path: PathBuf,
        observation_guard: LifecycleObservationGuard,
    },
}

impl MvPlan {
    pub const fn requires_hooks(&self) -> bool {
        matches!(self, Self::Apply(_))
    }

    pub fn branch(&self) -> &str {
        match self {
            Self::Noop(result) => &result.branch,
            Self::Apply(plan) => &plan.new_branch,
        }
    }

    pub fn target_path(&self) -> &Path {
        match self {
            Self::Noop(result) => &result.path,
            Self::Apply(plan) => &plan.target_path,
        }
    }
}

pub fn prepare_mv<G>(
    git: &G,
    context: &RepoContext,
    managed_root: &Path,
    snapshot: &WorktreeSnapshot,
    new_branch: &str,
    target_path: &Path,
) -> Result<MvPlan, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    ensure_non_empty_branch(new_branch)?;
    let current = current_worktree(snapshot, &context.current_worktree_root)?;
    let old_branch = current.branch.as_deref().ok_or_else(|| {
        error(
            ErrorCode::DetachedHead,
            "mv requires a branch checkout (detached HEAD is not supported)",
            [("path", json!(current.path))],
        )
    })?;
    if current.path == context.repo_root {
        return Err(error(
            ErrorCode::InvalidArgument,
            "mv cannot move the primary worktree",
            [("path", json!(current.path))],
        ));
    }
    if old_branch == new_branch {
        return Ok(MvPlan::Noop(MvResult {
            branch: new_branch.to_owned(),
            path: current.path.clone(),
            metadata: None,
        }));
    }
    validate_branch_ref(git, &context.repo_root, new_branch)?;
    if snapshot
        .worktrees
        .iter()
        .any(|worktree| worktree.branch.as_deref() == Some(new_branch))
    {
        return Err(error(
            ErrorCode::BranchAlreadyAttached,
            format!("branch is already attached to another worktree: {new_branch}"),
            [("branch", json!(new_branch))],
        ));
    }
    if local_branch_exists(git, &context.repo_root, new_branch)? {
        return Err(error(
            ErrorCode::BranchAlreadyExists,
            format!("branch already exists locally: {new_branch}"),
            [("branch", json!(new_branch))],
        ));
    }
    let validated_target =
        validate_managed_target(managed_root, Path::new(new_branch), target_path)?;
    ensure_target_path_empty(target_path)?;
    let base_branch = snapshot.base_branch.as_deref().ok_or_else(|| {
        CliError::new(
            ErrorCode::InvalidArgument,
            "mv requires a resolved base branch before mutation",
        )
    })?;
    let metadata = prepare_metadata_rename(MetadataRenameRequest {
        repo_root: &context.repo_root,
        from_branch: old_branch,
        to_branch: new_branch,
        source_path: &current.path,
        target_path,
        managed_root,
        target_relative_path: Path::new(new_branch),
        base_branch,
        observed_diverged_head: (current.merged.by_ancestry == Some(false))
            .then_some(current.head.as_str()),
    })
    .map_err(map_metadata_error)?;

    Ok(MvPlan::Apply(Box::new(MvApplyPlan {
        old_branch: old_branch.to_owned(),
        new_branch: new_branch.to_owned(),
        current_path: current.path.clone(),
        target_path: target_path.to_path_buf(),
        validated_target,
        metadata,
    })))
}

pub fn stage_mv_for_hook(plan: MvPlan) -> Result<StagedMvPlan, CliError> {
    match plan {
        MvPlan::Noop(result) => Ok(StagedMvPlan::Noop(result)),
        MvPlan::Apply(plan) => {
            stage_metadata_rename(&plan.metadata).map_err(map_metadata_error)?;
            Ok(StagedMvPlan::Apply(plan))
        }
    }
}

pub fn rollback_mv_after_pre_hook_failure(staged: &StagedMvPlan) -> Result<(), CliError> {
    if let StagedMvPlan::Apply(plan) = staged {
        rollback_staged_metadata_rename(&plan.metadata).map_err(map_metadata_error)?;
    }
    Ok(())
}

pub fn apply_mv_git<G>(
    git: &G,
    repo_root: &Path,
    staged: StagedMvPlan,
) -> Result<MvGitApplied, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let mut plan = match staged {
        StagedMvPlan::Noop(result) => return Ok(MvGitApplied::Noop(result)),
        StagedMvPlan::Apply(plan) => plan,
    };
    let observation_guard =
        LifecycleObservationGuard::acquire(repo_root).map_err(MapToCliError::map_to_cli_error)?;
    refresh_staged_lifecycle(&mut plan.metadata, &observation_guard).map_err(map_metadata_error)?;
    // Hooks run between prepare and apply, so repeat cheap conflict checks before renaming.
    let preflight = (|| {
        if local_branch_exists(git, repo_root, &plan.new_branch)? {
            return Err(error(
                ErrorCode::BranchAlreadyExists,
                format!("branch already exists locally: {}", plan.new_branch),
                [("branch", json!(plan.new_branch))],
            ));
        }
        let target_path = revalidate_managed_target(&plan.validated_target, &plan.target_path)?;
        ensure_target_path_empty(&target_path)?;
        Ok(target_path)
    })();
    let target_path = match preflight {
        Ok(path) => path,
        Err(error) => {
            let _ = rollback_staged_metadata_rename(&plan.metadata);
            return Err(error);
        }
    };
    if let Err(error) = run_git_checked(
        git,
        &plan.current_path,
        ["branch", "-m", &plan.old_branch, &plan.new_branch],
    ) {
        let _ = rollback_staged_metadata_rename(&plan.metadata);
        return Err(error);
    }
    if let Err(error) =
        mark_metadata_rename_branch_renamed(&plan.metadata).map_err(map_metadata_error)
    {
        return Err(compensate_failed_mv_after_branch_rename(
            git, repo_root, &plan, error,
        ));
    }
    if let Err(error) = create_target_parent(&target_path) {
        return Err(compensate_failed_mv_after_branch_rename(
            git, repo_root, &plan, error,
        ));
    }
    if let Err(error) = run_git_checked_os(
        git,
        repo_root,
        &[
            OsString::from("worktree"),
            OsString::from("move"),
            plan.current_path.as_os_str().to_owned(),
            target_path.as_os_str().to_owned(),
        ],
    ) {
        return Err(compensate_failed_mv_after_branch_rename(
            git, repo_root, &plan, error,
        ));
    }
    if let Err(error) =
        mark_metadata_rename_worktree_moved(&plan.metadata).map_err(map_metadata_error)
    {
        let compensated = compensate_applied_mv(git, repo_root, &plan, &target_path, error);
        return Err(compensated);
    }
    Ok(MvGitApplied::Apply {
        plan,
        target_path,
        observation_guard,
    })
}

pub fn finalize_mv_state(applied: MvGitApplied) -> Result<MvResult, CliError> {
    match applied {
        MvGitApplied::Noop(result) => Ok(result),
        MvGitApplied::Apply {
            plan,
            target_path,
            observation_guard,
        } => {
            let metadata = commit_metadata_rename_locked(plan.metadata, &observation_guard)
                .map_err(map_metadata_error)?;
            Ok(MvResult {
                branch: plan.new_branch,
                path: target_path,
                metadata: Some(metadata),
            })
        }
    }
}

fn compensate_failed_mv_after_branch_rename<G>(
    git: &G,
    repo_root: &Path,
    plan: &MvApplyPlan,
    mut original: CliError,
) -> CliError
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let mut failures = Vec::new();
    let branch_restored = match run_git_checked(
        git,
        &plan.current_path,
        ["branch", "-m", &plan.new_branch, &plan.old_branch],
    ) {
        Ok(_) => true,
        Err(error) => {
            failures.push(error.message);
            false
        }
    };
    if branch_restored
        && let Err(error) =
            rollback_staged_metadata_rename(&plan.metadata).map_err(map_metadata_error)
    {
        failures.push(error.message);
    }
    if !failures.is_empty() {
        original
            .details
            .insert("rollbackFailures".to_owned(), json!(failures));
        original
            .details
            .insert("repoRoot".to_owned(), json!(repo_root));
    }
    annotate_mv_rollback(original, plan, &failures)
}

fn compensate_applied_mv<G>(
    git: &G,
    repo_root: &Path,
    plan: &MvApplyPlan,
    target_path: &Path,
    mut original: CliError,
) -> CliError
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let mut failures = Vec::new();
    if let Err(error) = run_git_checked_os(
        git,
        repo_root,
        &[
            OsString::from("worktree"),
            OsString::from("move"),
            target_path.as_os_str().to_owned(),
            plan.current_path.as_os_str().to_owned(),
        ],
    ) {
        failures.push(error.message);
    } else if let Err(error) = run_git_checked(
        git,
        &plan.current_path,
        ["branch", "-m", &plan.new_branch, &plan.old_branch],
    ) {
        failures.push(error.message);
    }
    if failures.is_empty()
        && let Err(error) =
            rollback_staged_metadata_rename(&plan.metadata).map_err(map_metadata_error)
    {
        failures.push(error.message);
    }
    if !failures.is_empty() {
        original
            .details
            .insert("rollbackFailures".to_owned(), json!(failures));
    }
    annotate_mv_rollback(original, plan, &failures)
}

fn annotate_mv_rollback(
    mut original: CliError,
    plan: &MvApplyPlan,
    failures: &[String],
) -> CliError {
    original.execution.state = if failures.is_empty() {
        ExecutionState::RolledBack
    } else {
        ExecutionState::RecoveryRequired
    };
    if failures.is_empty() {
        original
            .execution
            .completed
            .push("rollbackRename".to_owned());
    } else {
        original
            .execution
            .recovery
            .insert("sourcePath".to_owned(), json!(plan.current_path));
        original
            .execution
            .recovery
            .insert("oldBranch".to_owned(), json!(plan.old_branch));
        original
            .execution
            .recovery
            .insert("newBranch".to_owned(), json!(plan.new_branch));
        original
            .execution
            .recovery
            .insert("rollbackFailures".to_owned(), json!(failures));
    }
    original
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractPlan {
    pub repo_root: PathBuf,
    pub managed_root: PathBuf,
    pub branch: String,
    pub base_branch: String,
    pub target_path: PathBuf,
    pub dirty: bool,
    validated_target: ValidatedManagedPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedExtractPlan {
    pub plan: ExtractPlan,
    pub stash_oid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractResult {
    pub branch: String,
    pub path: PathBuf,
    pub stash_oid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractGitApplied {
    plan: ExtractPlan,
    stash_oid: Option<String>,
    target_path: PathBuf,
}

pub fn prepare_extract<G>(
    git: &G,
    context: &RepoContext,
    managed_root: &Path,
    snapshot: &WorktreeSnapshot,
    base_branch: &str,
    target_path: &Path,
    allow_stash: bool,
) -> Result<ExtractPlan, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    if context.current_worktree_root != context.repo_root {
        return Err(error(
            ErrorCode::InvalidArgument,
            "extract currently supports only the primary worktree",
            [("sourcePath", json!(context.current_worktree_root))],
        ));
    }
    let source = current_worktree(snapshot, &context.current_worktree_root)?;
    if source.path != context.repo_root {
        return Err(error(
            ErrorCode::InvalidArgument,
            "extract currently supports only the primary worktree",
            [("sourcePath", json!(source.path))],
        ));
    }
    let branch = source.branch.as_deref().ok_or_else(|| {
        error(
            ErrorCode::DetachedHead,
            "extract requires current branch checkout",
            [("path", json!(source.path))],
        )
    })?;
    ensure_non_empty_branch(base_branch)?;
    if branch == base_branch {
        return Err(error(
            ErrorCode::InvalidArgument,
            "extract cannot target the base branch",
            [
                ("branch", json!(branch)),
                ("baseBranch", json!(base_branch)),
            ],
        ));
    }
    if !local_branch_exists(git, &context.repo_root, base_branch)? {
        return Err(error(
            ErrorCode::WorktreeNotFound,
            format!("local base branch was not found: {base_branch}"),
            [("branch", json!(base_branch))],
        ));
    }
    if let Some(attached) = snapshot.worktrees.iter().find(|worktree| {
        worktree.branch.as_deref() == Some(base_branch) && worktree.path != context.repo_root
    }) {
        return Err(error(
            ErrorCode::BranchInUse,
            format!("base branch '{base_branch}' is checked out in another worktree"),
            [
                ("branch", json!(base_branch)),
                ("path", json!(attached.path)),
            ],
        ));
    }
    let validated_target = validate_managed_target(managed_root, Path::new(branch), target_path)?;
    ensure_target_path_empty(target_path)?;
    if source.dirty && !allow_stash {
        return Err(CliError::new(
            ErrorCode::DirtyWorktree,
            "extract requires clean worktree unless --stash is specified",
        ));
    }
    let lifecycle = read_worktree_lifecycle(&context.repo_root, branch);
    if let JsonRecordState::Invalid { reason } = lifecycle.state {
        return Err(error(
            ErrorCode::InternalError,
            "extract cannot update invalid lifecycle metadata",
            [("path", json!(lifecycle.path)), ("reason", json!(reason))],
        ));
    }

    Ok(ExtractPlan {
        repo_root: context.repo_root.clone(),
        managed_root: managed_root.to_path_buf(),
        branch: branch.to_owned(),
        base_branch: base_branch.to_owned(),
        target_path: target_path.to_path_buf(),
        dirty: source.dirty,
        validated_target,
    })
}

/// Stashes dirty primary-worktree state before the pre-hook is invoked.
pub fn stage_extract_for_hook<G>(git: &G, plan: ExtractPlan) -> Result<StagedExtractPlan, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let stash_oid = if plan.dirty {
        run_git_checked(
            git,
            &plan.repo_root,
            [
                "stash",
                "push",
                "-u",
                "-m",
                &format!("vde-worktree extract {}", plan.branch),
            ],
        )?;
        let oid = match resolve_stash_top(git, &plan.repo_root) {
            Ok(oid) => oid,
            Err(error) => {
                return Err(restore_extract_stage_failure(
                    git,
                    &plan,
                    "stash@{0}",
                    None,
                    error,
                ));
            }
        };
        if let Err(source) = fs::create_dir_all(&plan.managed_root) {
            let error = io_cli_error(plan.managed_root.clone(), &source);
            return Err(restore_extract_stage_failure(
                git,
                &plan,
                &oid,
                Some(&oid),
                error,
            ));
        }
        Some(oid)
    } else {
        None
    };
    Ok(StagedExtractPlan { plan, stash_oid })
}

/// Restores staged changes when the pre-hook fails. The stash is dropped only after apply succeeds.
pub fn restore_extract_after_pre_hook_failure<G>(
    git: &G,
    staged: &StagedExtractPlan,
) -> Result<(), CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let Some(stash_oid) = &staged.stash_oid else {
        return Ok(());
    };
    apply_stash(git, &staged.plan.repo_root, stash_oid, true)?;
    drop_stash_by_oid(git, &staged.plan.repo_root, stash_oid)
}

pub fn apply_extract_git<G>(
    git: &G,
    staged: StagedExtractPlan,
) -> Result<ExtractGitApplied, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let plan = staged.plan;
    let stash_oid = staged.stash_oid;
    let base_exists =
        local_branch_exists(git, &plan.repo_root, &plan.base_branch).map_err(|error| {
            extract_recovery_details(error, &plan, stash_oid.as_deref(), &plan.branch)
        })?;
    if !base_exists {
        return Err(extract_recovery_details(
            error(
                ErrorCode::WorktreeNotFound,
                format!("local base branch was not found: {}", plan.base_branch),
                [("branch", json!(plan.base_branch))],
            ),
            &plan,
            stash_oid.as_deref(),
            &plan.branch,
        ));
    }
    let target_path = revalidate_managed_target(&plan.validated_target, &plan.target_path)
        .map_err(|error| {
            extract_recovery_details(error, &plan, stash_oid.as_deref(), &plan.branch)
        })?;
    ensure_target_path_empty(&target_path).map_err(|error| {
        extract_recovery_details(error, &plan, stash_oid.as_deref(), &plan.branch)
    })?;
    if git_worktree_dirty(git, &plan.repo_root).map_err(|error| {
        extract_recovery_details(error, &plan, stash_oid.as_deref(), &plan.branch)
    })? {
        return Err(extract_recovery_details(
            CliError::new(
                ErrorCode::DirtyWorktree,
                "extract pre-hook left the primary worktree dirty",
            ),
            &plan,
            stash_oid.as_deref(),
            &plan.branch,
        ));
    }
    run_git_checked(git, &plan.repo_root, ["checkout", &plan.base_branch]).map_err(|error| {
        extract_recovery_details(error, &plan, stash_oid.as_deref(), &plan.branch)
    })?;
    if let Err(error) = create_target_parent(&target_path) {
        return Err(compensate_extract_after_checkout(
            git,
            &plan,
            &target_path,
            stash_oid.as_deref(),
            error,
        ));
    }
    if let Err(error) = run_git_checked_os(
        git,
        &plan.repo_root,
        &[
            OsString::from("worktree"),
            OsString::from("add"),
            target_path.as_os_str().to_owned(),
            OsString::from(&plan.branch),
        ],
    ) {
        return Err(compensate_extract_after_checkout(
            git,
            &plan,
            &target_path,
            stash_oid.as_deref(),
            error,
        ));
    }
    if let Some(stash_oid) = &stash_oid
        && let Err(error) = apply_stash(git, &target_path, stash_oid, false)
    {
        return Err(compensate_extract_after_checkout(
            git,
            &plan,
            &target_path,
            Some(stash_oid),
            error,
        ));
    }
    Ok(ExtractGitApplied {
        plan,
        stash_oid,
        target_path,
    })
}

pub fn finalize_extract_state<G>(
    git: &G,
    applied: ExtractGitApplied,
) -> Result<ExtractResult, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    if let Err(mut error) = merge_lifecycle_observation(
        &applied.plan.repo_root,
        &applied.plan.branch,
        &applied.plan.base_branch,
        None,
    )
    .map_err(MapToCliError::map_to_cli_error)
    {
        if let Some(stash_oid) = &applied.stash_oid {
            error
                .details
                .insert("recoveryStashOid".to_owned(), json!(stash_oid));
        }
        error.details.insert(
            "recoveryWorktreePath".to_owned(),
            json!(applied.target_path),
        );
        return Err(error
            .at_phase(
                ExecutionPhase::Finalize,
                ExecutionState::RecoveryRequired,
                &["apply"],
            )
            .with_recovery("stashOid", json!(applied.stash_oid))
            .with_recovery("worktreePath", json!(applied.target_path)));
    }
    if let Some(stash_oid) = &applied.stash_oid {
        drop_stash_by_oid(git, &applied.plan.repo_root, stash_oid).map_err(|error| {
            error
                .at_phase(
                    ExecutionPhase::Finalize,
                    ExecutionState::RecoveryRequired,
                    &["apply", "persistLifecycle"],
                )
                .with_recovery("stashOid", json!(stash_oid))
                .with_recovery("worktreePath", json!(applied.target_path))
        })?;
    }
    Ok(ExtractResult {
        branch: applied.plan.branch,
        path: applied.target_path,
        stash_oid: applied.stash_oid,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsePlan {
    pub repo_root: PathBuf,
    pub branch: String,
    pub shared_worktree: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UseResult {
    pub branch: String,
    pub path: PathBuf,
    pub shared: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseOptions {
    pub invocation: UseInvocation,
    pub sharing: UseSharing,
}

impl Default for UseOptions {
    fn default() -> Self {
        Self {
            invocation: UseInvocation::NonInteractive {
                allow_agent: false,
                allow_unsafe: false,
            },
            sharing: UseSharing::Reject,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseInvocation {
    Interactive,
    NonInteractive {
        allow_agent: bool,
        allow_unsafe: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseSharing {
    Reject,
    Allow,
}

pub fn prepare_use<G>(
    git: &G,
    context: &RepoContext,
    snapshot: &WorktreeSnapshot,
    branch: &str,
    options: UseOptions,
) -> Result<UsePlan, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    ensure_non_empty_branch(branch)?;
    if let UseInvocation::NonInteractive {
        allow_agent,
        allow_unsafe,
    } = options.invocation
    {
        if !allow_agent {
            return Err(CliError::new(
                ErrorCode::UnsafeFlagRequired,
                "UNSAFE_FLAG_REQUIRED: use in non-TTY requires --allow-agent",
            ));
        }
        if !allow_unsafe {
            return Err(CliError::new(
                ErrorCode::UnsafeFlagRequired,
                "UNSAFE_FLAG_REQUIRED: use in non-TTY mode requires --allow-unsafe",
            ));
        }
    }
    let primary = snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.path == context.repo_root)
        .ok_or_else(|| {
            error(
                ErrorCode::WorktreeNotFound,
                "primary worktree was not found",
                [("path", json!(context.repo_root))],
            )
        })?;
    if primary.dirty {
        return Err(error(
            ErrorCode::DirtyWorktree,
            "use requires clean primary worktree",
            [("repoRoot", json!(context.repo_root))],
        ));
    }
    if !local_branch_exists(git, &context.repo_root, branch)? {
        return Err(error(
            ErrorCode::WorktreeNotFound,
            format!("local branch was not found: {branch}"),
            [("branch", json!(branch))],
        ));
    }
    let shared = snapshot.worktrees.iter().find(|worktree| {
        worktree.branch.as_deref() == Some(branch) && worktree.path != context.repo_root
    });
    if let Some(shared) = shared
        && options.sharing == UseSharing::Reject
    {
        return Err(error(
            ErrorCode::BranchInUse,
            format!("branch '{branch}' is already checked out in another worktree"),
            [("branch", json!(branch)), ("path", json!(shared.path))],
        ));
    }

    Ok(UsePlan {
        repo_root: context.repo_root.clone(),
        branch: branch.to_owned(),
        shared_worktree: shared.map(|worktree| worktree.path.clone()),
    })
}

pub fn apply_use<G>(git: &G, plan: UsePlan) -> Result<UseResult, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let shared = plan.shared_worktree.is_some();
    if git_worktree_dirty(git, &plan.repo_root)? {
        return Err(error(
            ErrorCode::DirtyWorktree,
            "use requires clean primary worktree",
            [("repoRoot", json!(plan.repo_root))],
        ));
    }
    if !local_branch_exists(git, &plan.repo_root, &plan.branch)? {
        return Err(error(
            ErrorCode::WorktreeNotFound,
            format!("local branch was not found: {}", plan.branch),
            [("branch", json!(plan.branch))],
        ));
    }
    if shared {
        run_git_checked(
            git,
            &plan.repo_root,
            ["checkout", "--ignore-other-worktrees", &plan.branch],
        )?;
    } else {
        run_git_checked(git, &plan.repo_root, ["checkout", &plan.branch])?;
    }
    Ok(UseResult {
        branch: plan.branch,
        path: plan.repo_root,
        shared,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockPlan {
    pub repo_root: PathBuf,
    pub branch: String,
    pub reason: String,
    pub owner: String,
    pub host: String,
    pub pid: u32,
}

pub fn prepare_lock(
    repo_root: &Path,
    snapshot: &WorktreeSnapshot,
    branch: &str,
    reason: &str,
    owner: &str,
    host: &str,
    pid: u32,
) -> Result<LockPlan, CliError> {
    ensure_non_empty_branch(branch)?;
    for (name, value) in [("reason", reason), ("owner", owner), ("host", host)] {
        if value.trim().is_empty() {
            return Err(error(
                ErrorCode::InvalidArgument,
                format!("{name} must be non-empty"),
                [(name, json!(value))],
            ));
        }
    }
    if !snapshot
        .worktrees
        .iter()
        .any(|worktree| worktree.branch.as_deref() == Some(branch))
    {
        return Err(error(
            ErrorCode::WorktreeNotFound,
            format!("worktree not found for branch: {branch}"),
            [("branch", json!(branch))],
        ));
    }
    let existing = read_worktree_lock(repo_root, branch);
    match existing.state {
        JsonRecordState::Invalid { reason } => {
            return Err(error(
                ErrorCode::LockConflict,
                "cannot update lock with invalid metadata; remove the lock first",
                [
                    ("branch", json!(branch)),
                    ("path", json!(existing.path)),
                    ("reason", json!(reason)),
                ],
            ));
        }
        JsonRecordState::Valid(record) if record.owner != owner => {
            return Err(error(
                ErrorCode::LockConflict,
                "lock is owned by another owner",
                [("branch", json!(branch)), ("owner", json!(record.owner))],
            ));
        }
        JsonRecordState::Missing | JsonRecordState::Valid(_) => {}
    }
    Ok(LockPlan {
        repo_root: repo_root.to_path_buf(),
        branch: branch.to_owned(),
        reason: reason.to_owned(),
        owner: owner.to_owned(),
        host: host.to_owned(),
        pid,
    })
}

pub fn apply_lock(plan: &LockPlan) -> Result<WorktreeLockRecord, CliError> {
    upsert_worktree_lock(
        &plan.repo_root,
        &plan.branch,
        WorktreeLockUpdate {
            reason: &plan.reason,
            owner: &plan.owner,
            host: &plan.host,
            pid: plan.pid,
        },
    )
    .map_err(MapToCliError::map_to_cli_error)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnlockPlan {
    Noop {
        branch: String,
    },
    RemoveValid {
        repo_root: PathBuf,
        branch: String,
        expected: WorktreeLockRecord,
    },
    RemoveInvalid {
        repo_root: PathBuf,
        path: PathBuf,
        branch: String,
    },
}

pub fn prepare_unlock(
    repo_root: &Path,
    branch: &str,
    owner: &str,
    force: bool,
) -> Result<UnlockPlan, CliError> {
    ensure_non_empty_branch(branch)?;
    let existing = read_worktree_lock(repo_root, branch);
    match existing.state {
        JsonRecordState::Missing => Ok(UnlockPlan::Noop {
            branch: branch.to_owned(),
        }),
        JsonRecordState::Invalid { .. } if force => Ok(UnlockPlan::RemoveInvalid {
            repo_root: repo_root.to_path_buf(),
            path: existing.path,
            branch: branch.to_owned(),
        }),
        JsonRecordState::Invalid { reason } => Err(error(
            ErrorCode::LockConflict,
            "lock metadata is invalid; use --force to unlock",
            [
                ("branch", json!(branch)),
                ("path", json!(existing.path)),
                ("reason", json!(reason)),
            ],
        )),
        JsonRecordState::Valid(record) if record.owner != owner && !force => Err(error(
            ErrorCode::LockConflict,
            "lock is owned by another owner; use --force to unlock",
            [("branch", json!(branch)), ("owner", json!(record.owner))],
        )),
        JsonRecordState::Valid(record) => Ok(UnlockPlan::RemoveValid {
            repo_root: repo_root.to_path_buf(),
            branch: branch.to_owned(),
            expected: record,
        }),
    }
}

pub fn apply_unlock(plan: UnlockPlan) -> Result<(), CliError> {
    match plan {
        UnlockPlan::Noop { .. } => Ok(()),
        UnlockPlan::RemoveValid {
            repo_root,
            branch,
            expected,
        } => {
            let current = read_worktree_lock(&repo_root, &branch);
            if current.state != JsonRecordState::Valid(expected) {
                return Err(error(
                    ErrorCode::LockConflict,
                    "lock metadata changed after unlock preflight",
                    [("branch", json!(branch)), ("path", json!(current.path))],
                ));
            }
            delete_worktree_lock(&repo_root, &branch).map_err(MapToCliError::map_to_cli_error)
        }
        UnlockPlan::RemoveInvalid {
            repo_root,
            path,
            branch,
        } => {
            // The path is captured by typed preflight under the repository lock; recomputing and
            // comparing it prevents this exceptional force path from becoming arbitrary deletion.
            let expected = worktree_lock_file_path(&repo_root, &branch);
            if expected != path {
                return Err(CliError::new(
                    ErrorCode::InternalError,
                    "invalid forced unlock target",
                ));
            }
            let current = read_worktree_lock(&repo_root, &branch);
            match current.state {
                JsonRecordState::Missing => return Ok(()),
                JsonRecordState::Invalid { .. } if current.path == path => {}
                JsonRecordState::Invalid { .. } | JsonRecordState::Valid(_) => {
                    return Err(error(
                        ErrorCode::LockConflict,
                        "lock metadata changed after forced unlock preflight",
                        [("branch", json!(branch)), ("path", json!(current.path))],
                    ));
                }
            }
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(io_cli_error(path, &source)),
            }
        }
    }
}

fn current_worktree<'a>(
    snapshot: &'a WorktreeSnapshot,
    current_path: &Path,
) -> Result<&'a WorktreeStatus, CliError> {
    snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.path == current_path)
        .ok_or_else(|| {
            error(
                ErrorCode::WorktreeNotFound,
                "no worktree found for current location",
                [("currentWorktreeRoot", json!(current_path))],
            )
        })
}

fn ensure_non_empty_branch(branch: &str) -> Result<(), CliError> {
    if branch.trim().is_empty() {
        return Err(CliError::new(
            ErrorCode::InvalidArgument,
            "branch must be non-empty",
        ));
    }
    Ok(())
}

fn validate_branch_ref<G>(git: &G, repo_root: &Path, branch: &str) -> Result<(), CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = git
        .run_git(repo_root, ["check-ref-format", "--branch", branch])
        .map_err(MapToCliError::map_to_cli_error)?;
    if output.exit_code == Some(0) && !output.timed_out {
        return Ok(());
    }
    Err(error(
        ErrorCode::InvalidArgument,
        format!("invalid branch name: {branch}"),
        [
            ("branch", json!(branch)),
            ("stderr", json!(String::from_utf8_lossy(&output.stderr))),
        ],
    ))
}

fn local_branch_exists<G>(git: &G, repo_root: &Path, branch: &str) -> Result<bool, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = git
        .run_git(
            repo_root,
            [
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )
        .map_err(MapToCliError::map_to_cli_error)?;
    match (output.exit_code, output.timed_out) {
        (Some(0), false) => Ok(true),
        (Some(1), false) => Ok(false),
        _ => Err(git_output_error(
            repo_root,
            ["show-ref", "--verify", "--quiet"],
            &output,
        )),
    }
}

fn git_worktree_dirty<G>(git: &G, cwd: &Path) -> Result<bool, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = run_git_checked(git, cwd, ["status", "--porcelain"])?;
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn ensure_target_path_empty(path: &Path) -> Result<(), CliError> {
    match fs::metadata(path) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_cli_error(path.to_path_buf(), &source)),
        Ok(metadata) if !metadata.is_dir() => Err(error(
            ErrorCode::TargetPathNotEmpty,
            format!("target path is not an empty directory: {}", path.display()),
            [("path", json!(path))],
        )),
        Ok(_) => {
            let mut entries = fs::read_dir(path).map_err(|source| io_cli_error(path, &source))?;
            if entries
                .next()
                .transpose()
                .map_err(|source| io_cli_error(path, &source))?
                .is_some()
            {
                return Err(error(
                    ErrorCode::TargetPathNotEmpty,
                    format!("target path is not empty: {}", path.display()),
                    [("path", json!(path))],
                ));
            }
            Ok(())
        }
    }
}

fn validate_managed_target(
    managed_root: &Path,
    relative_path: &Path,
    expected_path: &Path,
) -> Result<ValidatedManagedPath, CliError> {
    let validated = ValidatedManagedPath::validate(managed_root, relative_path)
        .map_err(MapToCliError::map_to_cli_error)?;
    let _ = validated
        .with_revalidated_path(|path| Ok::<PathBuf, io::Error>(path.to_path_buf()))
        .map_err(map_validated_path_error)?;
    let expected_from_root = managed_root.join(relative_path);
    if expected_from_root != expected_path {
        return Err(error(
            ErrorCode::InvalidArgument,
            "target path does not match the validated managed path",
            [
                ("expectedPath", json!(expected_from_root)),
                ("actualPath", json!(expected_path)),
            ],
        ));
    }
    Ok(validated)
}

fn revalidate_managed_target(
    validated: &ValidatedManagedPath,
    _expected_path: &Path,
) -> Result<PathBuf, CliError> {
    validated
        .with_revalidated_path(|path| Ok::<PathBuf, io::Error>(path.to_path_buf()))
        .map_err(map_validated_path_error)
}

fn map_validated_path_error(error: ValidatedPathOperationError<io::Error>) -> CliError {
    match error {
        ValidatedPathOperationError::Containment(error) => error.map_to_cli_error(),
        ValidatedPathOperationError::Operation(error) => {
            CliError::new(ErrorCode::InternalError, error.to_string())
        }
    }
}

fn create_target_parent(path: &Path) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        CliError::new(
            ErrorCode::InvalidArgument,
            "target path must have a parent directory",
        )
    })?;
    fs::create_dir_all(parent).map_err(|source| io_cli_error(parent, &source))
}

fn restore_extract_stage_failure<G>(
    git: &G,
    plan: &ExtractPlan,
    stash_reference: &str,
    stash_oid: Option<&str>,
    mut original: CliError,
) -> CliError
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let restore = apply_stash(git, &plan.repo_root, stash_reference, true).and_then(|()| {
        if let Some(stash_oid) = stash_oid {
            drop_stash_by_oid(git, &plan.repo_root, stash_oid)
        } else {
            run_git_checked(git, &plan.repo_root, ["stash", "drop", stash_reference]).map(|_| ())
        }
    });
    match restore {
        Ok(()) => {
            original.execution.state = ExecutionState::RolledBack;
            original
                .execution
                .completed
                .push("restoreSource".to_owned());
            original
                .details
                .insert("autoRestoreCompleted".to_owned(), json!(true));
        }
        Err(error) => {
            original.execution.state = ExecutionState::RecoveryRequired;
            original
                .execution
                .recovery
                .insert("stashRef".to_owned(), json!(stash_reference));
            original
                .execution
                .recovery
                .insert("worktreePath".to_owned(), json!(plan.repo_root));
            original
                .details
                .insert("autoRestoreFailed".to_owned(), json!(true));
            original
                .details
                .insert("autoRestoreError".to_owned(), json!(error.message));
            original
                .details
                .insert("recoveryStashRef".to_owned(), json!(stash_reference));
        }
    }
    original
}

fn compensate_extract_after_checkout<G>(
    git: &G,
    plan: &ExtractPlan,
    target_path: &Path,
    stash_oid: Option<&str>,
    original: CliError,
) -> CliError
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let mut failures = Vec::new();
    if target_path.exists()
        && let Err(error) = run_git_checked_os(
            git,
            &plan.repo_root,
            &[
                OsString::from("worktree"),
                OsString::from("remove"),
                OsString::from("--force"),
                target_path.as_os_str().to_owned(),
            ],
        )
    {
        failures.push(error.message);
    }
    let branch_restored = match run_git_checked(git, &plan.repo_root, ["checkout", &plan.branch]) {
        Ok(_) => true,
        Err(error) => {
            failures.push(error.message);
            false
        }
    };
    if branch_restored && let Some(stash_oid) = stash_oid {
        if let Err(error) = apply_stash(git, &plan.repo_root, stash_oid, true) {
            failures.push(error.message);
        } else if let Err(error) = drop_stash_by_oid(git, &plan.repo_root, stash_oid) {
            failures.push(error.message);
        }
    }

    let mut error = extract_recovery_details(original, plan, stash_oid, &plan.base_branch);
    if failures.is_empty() {
        error.execution.state = ExecutionState::RolledBack;
        error.execution.recovery.clear();
        error.execution.completed.push("rollbackExtract".to_owned());
        error
            .details
            .insert("autoRestoreCompleted".to_owned(), json!(true));
        error
            .details
            .insert("currentBranch".to_owned(), json!(plan.branch));
    } else {
        error
            .details
            .insert("autoRestoreFailed".to_owned(), json!(true));
        error
            .details
            .insert("rollbackFailures".to_owned(), json!(failures));
    }
    error
}

fn extract_recovery_details(
    mut error: CliError,
    plan: &ExtractPlan,
    stash_oid: Option<&str>,
    current_branch: &str,
) -> CliError {
    error
        .details
        .insert("originalBranch".to_owned(), json!(plan.branch));
    error
        .details
        .insert("currentBranch".to_owned(), json!(current_branch));
    error
        .details
        .insert("targetPath".to_owned(), json!(plan.target_path));
    if let Some(stash_oid) = stash_oid {
        error
            .details
            .insert("stashOid".to_owned(), json!(stash_oid));
    }
    error
        .at_phase(ExecutionPhase::Apply, ExecutionState::RecoveryRequired, &[])
        .with_recovery("stashOid", json!(stash_oid))
        .with_recovery("worktreePath", json!(plan.target_path))
        .with_recovery("originalBranch", json!(plan.branch))
        .with_recovery("currentBranch", json!(current_branch))
}

fn resolve_stash_top<G>(git: &G, cwd: &Path) -> Result<String, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = run_git_checked(git, cwd, ["rev-parse", "--verify", "-q", "stash@{0}"])?;
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if oid.is_empty() {
        return Err(CliError::new(
            ErrorCode::InternalError,
            "failed to resolve created stash entry",
        ));
    }
    Ok(oid)
}

fn apply_stash<G>(git: &G, cwd: &Path, stash_oid: &str, auto_restore: bool) -> Result<(), CliError>
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
        if auto_restore {
            "failed to auto-restore stashed changes after pre-hook failure"
        } else {
            "failed to apply stash to extracted worktree"
        },
        [("cwd", json!(cwd)), ("stashOid", json!(stash_oid))],
    ))
}

fn drop_stash_by_oid<G>(git: &G, cwd: &Path, stash_oid: &str) -> Result<(), CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
{
    let output = run_git_checked(git, cwd, ["stash", "list", "--format=%gd%x09%H"])?;
    let list = String::from_utf8_lossy(&output.stdout);
    let stash_ref = list.lines().find_map(|line| {
        let (stash_ref, oid) = line.trim().split_once('\t')?;
        (oid == stash_oid && !stash_ref.is_empty()).then_some(stash_ref)
    });
    if let Some(stash_ref) = stash_ref {
        run_git_checked(git, cwd, ["stash", "drop", stash_ref])?;
    }
    Ok(())
}

fn run_git_checked<G, I, S>(git: &G, cwd: &Path, args: I) -> Result<ProcessOutput, CliError>
where
    G: GitSnapshotPort,
    G::Error: MapToCliError,
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    run_git_checked_os(git, cwd, &args)
}

fn run_git_checked_os<G>(git: &G, cwd: &Path, args: &[OsString]) -> Result<ProcessOutput, CliError>
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
    Err(git_output_error_os(cwd, args, &output))
}

fn git_output_error<I, S>(cwd: &Path, args: I, output: &ProcessOutput) -> CliError
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    git_output_error_os(cwd, &args, output)
}

fn git_output_error_os(cwd: &Path, args: &[OsString], output: &ProcessOutput) -> CliError {
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
            ("stdout", json!(String::from_utf8_lossy(&output.stdout))),
            ("stderr", json!(String::from_utf8_lossy(&output.stderr))),
        ],
    )
}

fn map_metadata_error(source: MetadataTransactionError) -> CliError {
    let message = source.to_string();
    match source {
        MetadataTransactionError::LifecycleObservation(source) => {
            MapToCliError::map_to_cli_error(source)
        }
        MetadataTransactionError::InvalidBranchNames
        | MetadataTransactionError::InvalidBaseBranch
        | MetadataTransactionError::InvalidWorktreePaths => {
            CliError::new(ErrorCode::InvalidArgument, message)
        }
        MetadataTransactionError::InvalidMetadata { path, reason, .. } => error(
            ErrorCode::LockConflict,
            message,
            [("path", json!(path)), ("reason", json!(reason))],
        ),
        MetadataTransactionError::TargetExists { path, .. }
        | MetadataTransactionError::PendingTransaction(path) => {
            error(ErrorCode::LockConflict, message, [("path", json!(path))])
        }
        MetadataTransactionError::InvalidJournal { path, reason } => error(
            ErrorCode::InternalError,
            message,
            [("path", json!(path)), ("reason", json!(reason))],
        ),
        MetadataTransactionError::RecoveryConflict { path, .. } => {
            error(ErrorCode::InternalError, message, [("path", json!(path))])
        }
        MetadataTransactionError::Io { path, source } => io_cli_error(path, &source),
        MetadataTransactionError::Timestamp(source) => error(
            ErrorCode::InternalError,
            message,
            [("cause", json!(source.to_string()))],
        ),
        MetadataTransactionError::InjectedCrash { .. } => {
            CliError::new(ErrorCode::InternalError, message)
        }
    }
}

fn io_cli_error(path: impl Into<PathBuf>, source: &io::Error) -> CliError {
    error(
        ErrorCode::InternalError,
        "filesystem operation failed",
        [
            ("path", json!(path.into())),
            ("cause", json!(source.to_string())),
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
    use super::*;
    use crate::adapters::git_cli::{GitCli, GitCliError};
    use crate::adapters::process::StdProcessRunner;
    use crate::domain::worktree::{
        PrState, WorktreeLockState, WorktreeMergedState, WorktreeUpstreamState,
    };
    use crate::ports::process::ProcessOutput as GitProcessOutput;
    use crate::ports::snapshot::GitSnapshotPort;
    use std::convert::Infallible;
    use std::ffi::{OsStr, OsString};
    use std::process::Command;
    use std::sync::Mutex;

    impl MapToCliError for Infallible {
        fn map_to_cli_error(self) -> CliError {
            match self {}
        }
    }

    #[derive(Default)]
    struct RejectingStashGit {
        calls: Mutex<Vec<Vec<OsString>>>,
    }

    impl GitSnapshotPort for RejectingStashGit {
        type Error = Infallible;

        fn run_git<I, S>(&self, _cwd: &Path, args: I) -> Result<GitProcessOutput, Self::Error>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.calls.lock().unwrap().push(
                args.into_iter()
                    .map(|arg| arg.as_ref().to_owned())
                    .collect(),
            );
            Ok(GitProcessOutput {
                stdout: Vec::new(),
                stderr: b"conflict".to_vec(),
                exit_code: Some(1),
                timed_out: false,
            })
        }
    }

    struct RejectStashOidResolution {
        inner: GitCli<StdProcessRunner>,
    }

    impl GitSnapshotPort for RejectStashOidResolution {
        type Error = GitCliError;

        fn run_git<I, S>(&self, cwd: &Path, args: I) -> Result<GitProcessOutput, Self::Error>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let args = args
                .into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect::<Vec<_>>();
            if args.first().is_some_and(|arg| arg == "rev-parse")
                && args.last().is_some_and(|arg| arg == "stash@{0}")
            {
                return Ok(GitProcessOutput {
                    stdout: Vec::new(),
                    stderr: b"injected stash OID resolution failure".to_vec(),
                    exit_code: Some(1),
                    timed_out: false,
                });
            }
            self.inner.execute(cwd, &args)
        }
    }

    fn git(cwd: &Path, args: &[&str]) {
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
    }

    fn fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), &["init", "-b", "main"]);
        git(
            directory.path(),
            &["config", "user.email", "test@example.com"],
        );
        git(directory.path(), &["config", "user.name", "Test"]);
        fs::write(directory.path().join("README"), "initial\n").unwrap();
        git(directory.path(), &["add", "README"]);
        git(directory.path(), &["commit", "-m", "initial"]);
        directory
    }

    fn status(path: &Path, branch: Option<&str>, dirty: bool) -> WorktreeStatus {
        WorktreeStatus {
            branch: branch.map(str::to_owned),
            path: path.to_path_buf(),
            head: "deadbeef".to_owned(),
            dirty,
            locked: WorktreeLockState {
                value: false,
                reason: None,
                owner: None,
            },
            merged: WorktreeMergedState {
                by_ancestry: Some(false),
                by_pr: None,
                overall: Some(false),
            },
            pr: PrState::none(),
            upstream: WorktreeUpstreamState {
                ahead: None,
                behind: None,
                remote: None,
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

    #[test]
    fn mv_rejects_primary_and_handles_same_name_without_git_or_hooks() {
        let repo = Path::new("/repo");
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: repo.to_path_buf(),
            git_common_dir: repo.join(".git"),
        };
        let primary_snapshot = snapshot(repo, vec![status(repo, Some("main"), false)]);
        let adapter = GitCli::new(StdProcessRunner);
        let error = prepare_mv(
            &adapter,
            &context,
            Path::new("/repo/.worktree"),
            &primary_snapshot,
            "renamed",
            Path::new("/new"),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);

        let managed = Path::new("/repo/.worktree/feature/a");
        let context = RepoContext {
            current_worktree_root: managed.to_path_buf(),
            ..context
        };
        let snapshot = snapshot(repo, vec![status(managed, Some("feature/a"), false)]);
        let noop = prepare_mv(
            &adapter,
            &context,
            Path::new("/repo/.worktree"),
            &snapshot,
            "feature/a",
            Path::new("/ignored"),
        )
        .unwrap();
        assert!(!noop.requires_hooks());
        assert_eq!(noop.target_path(), managed);
    }

    #[test]
    fn mv_renames_real_branch_path_and_metadata() {
        let fixture = fixture();
        let repo = fixture.path();
        let managed_root = repo.join(".worktree");
        let old_path = repo.join(".worktree/feature/old");
        let new_path = repo.join(".worktree/feature/new");
        git(repo, &["branch", "feature/old"]);
        git(
            repo,
            &["worktree", "add", old_path.to_str().unwrap(), "feature/old"],
        );
        merge_lifecycle_observation(repo, "feature/old", "main", None).unwrap();
        upsert_worktree_lock(
            repo,
            "feature/old",
            WorktreeLockUpdate {
                reason: "busy",
                owner: "alice",
                host: "host",
                pid: 1,
            },
        )
        .unwrap();
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: old_path.clone(),
            git_common_dir: repo.join(".git"),
        };
        let snapshot = snapshot(repo, vec![status(&old_path, Some("feature/old"), false)]);
        let adapter = GitCli::new(StdProcessRunner);
        let plan = prepare_mv(
            &adapter,
            &context,
            &managed_root,
            &snapshot,
            "feature/new",
            &new_path,
        )
        .unwrap();
        let staged = stage_mv_for_hook(plan).unwrap();
        assert_eq!(
            fs::read_dir(repo.join(".vde/worktree/state/metadata-transactions"))
                .unwrap()
                .count(),
            1,
            "journal must exist before Git mutation"
        );
        let applied = apply_mv_git(&adapter, repo, staged).unwrap();
        assert!(matches!(
            read_worktree_lock(repo, "feature/old").state,
            JsonRecordState::Valid(_)
        ));
        assert!(matches!(
            read_worktree_lock(repo, "feature/new").state,
            JsonRecordState::Missing
        ));
        let result = finalize_mv_state(applied).unwrap();
        assert_eq!(
            result.path.canonicalize().unwrap(),
            new_path.canonicalize().unwrap()
        );
        assert!(result.metadata.unwrap().lock_moved);
        assert!(matches!(
            read_worktree_lock(repo, "feature/new").state,
            JsonRecordState::Valid(_)
        ));
        assert!(matches!(
            read_worktree_lock(repo, "feature/old").state,
            JsonRecordState::Missing
        ));
        let JsonRecordState::Valid(lifecycle) = read_worktree_lifecycle(repo, "feature/new").state
        else {
            panic!("renamed lifecycle is missing");
        };
        assert!(lifecycle.ever_diverged);
        assert_eq!(lifecycle.last_diverged_head.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn mv_preflight_rejects_detached_branch_ref_and_path_collisions() {
        let fixture = fixture();
        let repo = fixture.path();
        let managed_root = repo.join(".worktree");
        let old_path = repo.join(".worktree/feature/old");
        git(repo, &["branch", "feature/old"]);
        git(
            repo,
            &["worktree", "add", old_path.to_str().unwrap(), "feature/old"],
        );
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: old_path.clone(),
            git_common_dir: repo.join(".git"),
        };
        let adapter = GitCli::new(StdProcessRunner);

        let detached = snapshot(repo, vec![status(&old_path, None, false)]);
        assert_eq!(
            prepare_mv(
                &adapter,
                &context,
                &managed_root,
                &detached,
                "feature/new",
                &managed_root.join("feature/new")
            )
            .unwrap_err()
            .code,
            ErrorCode::DetachedHead
        );

        let attached_path = repo.join(".worktree/feature/new");
        let attached = snapshot(
            repo,
            vec![
                status(&old_path, Some("feature/old"), false),
                status(&attached_path, Some("feature/new"), false),
            ],
        );
        assert_eq!(
            prepare_mv(
                &adapter,
                &context,
                &managed_root,
                &attached,
                "feature/new",
                &attached_path,
            )
            .unwrap_err()
            .code,
            ErrorCode::BranchAlreadyAttached
        );

        git(repo, &["branch", "feature/existing"]);
        let ordinary = snapshot(repo, vec![status(&old_path, Some("feature/old"), false)]);
        assert_eq!(
            prepare_mv(
                &adapter,
                &context,
                &managed_root,
                &ordinary,
                "feature/existing",
                &managed_root.join("feature/existing")
            )
            .unwrap_err()
            .code,
            ErrorCode::BranchAlreadyExists
        );

        let nonempty = managed_root.join("feature/new");
        fs::create_dir_all(nonempty.parent().unwrap()).unwrap();
        fs::create_dir(&nonempty).unwrap();
        fs::write(nonempty.join("file"), "occupied").unwrap();
        assert_eq!(
            prepare_mv(
                &adapter,
                &context,
                &managed_root,
                &ordinary,
                "feature/new",
                &nonempty,
            )
            .unwrap_err()
            .code,
            ErrorCode::TargetPathNotEmpty
        );
    }

    #[cfg(unix)]
    #[test]
    fn mv_target_rejects_traversal_and_symlink_escape() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        let repo = fixture.path();
        let managed_root = repo.join(".worktree");
        fs::create_dir_all(&managed_root).unwrap();
        let old_path = managed_root.join("feature/old");
        git(repo, &["branch", "feature/old"]);
        git(
            repo,
            &["worktree", "add", old_path.to_str().unwrap(), "feature/old"],
        );
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: old_path.clone(),
            git_common_dir: repo.join(".git"),
        };
        let snapshot = snapshot(repo, vec![status(&old_path, Some("feature/old"), false)]);
        let adapter = GitCli::new(StdProcessRunner);

        let traversal = managed_root.join("../escape");
        assert_eq!(
            prepare_mv(
                &adapter,
                &context,
                &managed_root,
                &snapshot,
                "../escape",
                &traversal,
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );

        let outside = repo.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, managed_root.join("link")).unwrap();
        let escaped = managed_root.join("link/escape");
        assert_eq!(
            prepare_mv(
                &adapter,
                &context,
                &managed_root,
                &snapshot,
                "link/escape",
                &escaped,
            )
            .unwrap_err()
            .code,
            ErrorCode::PathOutsideRepo
        );
    }

    #[test]
    fn extract_stash_can_be_restored_after_pre_hook_failure() {
        let fixture = fixture();
        let repo = fixture.path();
        let managed_root = repo.join(".worktree");
        fs::create_dir_all(&managed_root).unwrap();
        git(repo, &["checkout", "-b", "feature/extract"]);
        fs::write(repo.join("dirty.txt"), "dirty\n").unwrap();
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: repo.to_path_buf(),
            git_common_dir: repo.join(".git"),
        };
        let snapshot = snapshot(repo, vec![status(repo, Some("feature/extract"), true)]);
        let adapter = GitCli::new(StdProcessRunner);
        assert_eq!(
            prepare_extract(
                &adapter,
                &context,
                &managed_root,
                &snapshot,
                "main",
                &repo.join(".worktree/feature/extract"),
                false,
            )
            .unwrap_err()
            .code,
            ErrorCode::DirtyWorktree
        );
        let plan = prepare_extract(
            &adapter,
            &context,
            &managed_root,
            &snapshot,
            "main",
            &repo.join(".worktree/feature/extract"),
            true,
        )
        .unwrap();
        let staged = stage_extract_for_hook(&adapter, plan).unwrap();
        assert!(!repo.join("dirty.txt").exists());
        restore_extract_after_pre_hook_failure(&adapter, &staged).unwrap();
        assert_eq!(
            fs::read_to_string(repo.join("dirty.txt")).unwrap(),
            "dirty\n"
        );
        let stash = adapter.execute_checked(repo, ["stash", "list"]).unwrap();
        assert!(stash.stdout.is_empty());
    }

    #[test]
    fn extract_stage_restores_changes_when_stash_oid_resolution_fails() {
        let fixture = fixture();
        let repo = fixture.path();
        git(repo, &["checkout", "-b", "feature/oid-failure"]);
        fs::write(repo.join("tracked"), "base\n").unwrap();
        git(repo, &["add", "tracked"]);
        git(repo, &["commit", "-m", "tracked"]);
        fs::write(repo.join("tracked"), "dirty\n").unwrap();
        fs::write(repo.join("untracked"), "dirty untracked\n").unwrap();
        let managed_root = repo.join(".worktree");
        fs::create_dir(&managed_root).unwrap();
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: repo.to_path_buf(),
            git_common_dir: repo.join(".git"),
        };
        let plan = prepare_extract(
            &GitCli::new(StdProcessRunner),
            &context,
            &managed_root,
            &snapshot(repo, vec![status(repo, Some("feature/oid-failure"), true)]),
            "main",
            &managed_root.join("feature/oid-failure"),
            true,
        )
        .unwrap();
        let rejecting = RejectStashOidResolution {
            inner: GitCli::new(StdProcessRunner),
        };

        let error = stage_extract_for_hook(&rejecting, plan).unwrap_err();
        assert_eq!(
            error.details.get("autoRestoreCompleted"),
            Some(&json!(true))
        );
        assert_eq!(fs::read_to_string(repo.join("tracked")).unwrap(), "dirty\n");
        assert_eq!(
            fs::read_to_string(repo.join("untracked")).unwrap(),
            "dirty untracked\n"
        );
        let stash = GitCli::new(StdProcessRunner)
            .execute_checked(repo, ["stash", "list"])
            .unwrap();
        assert!(stash.stdout.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn extract_target_rejects_mismatched_absolute_and_symlink_escape() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        let repo = fixture.path();
        git(repo, &["checkout", "-b", "feature/extract"]);
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: repo.to_path_buf(),
            git_common_dir: repo.join(".git"),
        };
        let snapshot = snapshot(repo, vec![status(repo, Some("feature/extract"), false)]);
        let adapter = GitCli::new(StdProcessRunner);
        let managed_root = repo.join(".worktree");
        fs::create_dir(&managed_root).unwrap();

        assert_eq!(
            prepare_extract(
                &adapter,
                &context,
                &managed_root,
                &snapshot,
                "main",
                &repo.join("outside"),
                false,
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );

        let outside = repo.join("outside-root");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, managed_root.join("feature")).unwrap();
        assert_eq!(
            prepare_extract(
                &adapter,
                &context,
                &managed_root,
                &snapshot,
                "main",
                &managed_root.join("feature/extract"),
                false,
            )
            .unwrap_err()
            .code,
            ErrorCode::PathOutsideRepo
        );
    }

    #[test]
    fn extract_stash_apply_failure_is_typed_and_preserves_stash_for_recovery() {
        let git = RejectingStashGit::default();
        let temp = tempfile::tempdir().unwrap();
        let managed_root = temp.path().join(".worktree");
        fs::create_dir(&managed_root).unwrap();
        let validated_target =
            ValidatedManagedPath::validate(&managed_root, Path::new("feature/extract")).unwrap();
        let staged = StagedExtractPlan {
            plan: ExtractPlan {
                repo_root: temp.path().to_path_buf(),
                managed_root: managed_root.clone(),
                branch: "feature/extract".to_owned(),
                base_branch: "main".to_owned(),
                target_path: managed_root.join("feature/extract"),
                dirty: true,
                validated_target,
            },
            stash_oid: Some("0123456789abcdef".to_owned()),
        };
        let error = restore_extract_after_pre_hook_failure(&git, &staged).unwrap_err();
        assert_eq!(error.code, ErrorCode::StashApplyFailed);
        let calls = git.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "failed apply must not drop the stash");
        assert_eq!(calls[0][..2], ["stash", "apply"]);
    }

    #[test]
    fn extract_moves_primary_branch_and_applies_stash_by_oid() {
        let fixture = fixture();
        let repo = fixture.path();
        git(repo, &["checkout", "-b", "feature/extract"]);
        fs::write(repo.join("dirty.txt"), "dirty\n").unwrap();
        let target = repo.join(".worktree/feature/extract");
        let managed_root = repo.join(".worktree");
        fs::create_dir_all(&managed_root).unwrap();
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: repo.to_path_buf(),
            git_common_dir: repo.join(".git"),
        };
        let snapshot = snapshot(repo, vec![status(repo, Some("feature/extract"), true)]);
        let adapter = GitCli::new(StdProcessRunner);
        let plan = prepare_extract(
            &adapter,
            &context,
            &managed_root,
            &snapshot,
            "main",
            &target,
            true,
        )
        .unwrap();
        let staged = stage_extract_for_hook(&adapter, plan).unwrap();
        let stash_oid = staged.stash_oid.clone().unwrap();
        let applied = apply_extract_git(&adapter, staged).unwrap();
        assert!(matches!(
            read_worktree_lifecycle(repo, "feature/extract").state,
            JsonRecordState::Missing
        ));
        let retained_stash = adapter.execute_checked(repo, ["stash", "list"]).unwrap();
        assert!(!retained_stash.stdout.is_empty());
        let result = finalize_extract_state(&adapter, applied).unwrap();
        assert_eq!(result.stash_oid.as_deref(), Some(stash_oid.as_str()));
        assert_eq!(
            fs::read_to_string(target.join("dirty.txt")).unwrap(),
            "dirty\n"
        );
        let head = adapter
            .execute_checked(repo, ["branch", "--show-current"])
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), "main");
        let stash = adapter.execute_checked(repo, ["stash", "list"]).unwrap();
        assert!(stash.stdout.is_empty());
    }

    #[test]
    fn use_enforces_non_tty_dirty_and_shared_guards_before_apply() {
        let fixture = fixture();
        let repo = fixture.path();
        git(repo, &["branch", "feature/use"]);
        let shared = repo.join(".worktree/feature/use");
        let context = RepoContext {
            repo_root: repo.to_path_buf(),
            current_worktree_root: repo.to_path_buf(),
            git_common_dir: repo.join(".git"),
        };
        let adapter = GitCli::new(StdProcessRunner);
        let clean = snapshot(
            repo,
            vec![
                status(repo, Some("main"), false),
                status(&shared, Some("feature/use"), false),
            ],
        );
        let error = prepare_use(
            &adapter,
            &context,
            &clean,
            "feature/use",
            UseOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafeFlagRequired);
        let error = prepare_use(
            &adapter,
            &context,
            &clean,
            "feature/use",
            UseOptions {
                invocation: UseInvocation::NonInteractive {
                    allow_agent: true,
                    allow_unsafe: true,
                },
                ..UseOptions::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::BranchInUse);

        let dirty = snapshot(repo, vec![status(repo, Some("main"), true)]);
        let error = prepare_use(
            &adapter,
            &context,
            &dirty,
            "feature/use",
            UseOptions {
                invocation: UseInvocation::Interactive,
                sharing: UseSharing::Allow,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::DirtyWorktree);

        let ordinary = snapshot(repo, vec![status(repo, Some("main"), false)]);
        let plan = prepare_use(
            &adapter,
            &context,
            &ordinary,
            "feature/use",
            UseOptions {
                invocation: UseInvocation::Interactive,
                sharing: UseSharing::Reject,
            },
        )
        .unwrap();
        let result = apply_use(&adapter, plan).unwrap();
        assert!(!result.shared);
        let head = adapter
            .execute_checked(repo, ["branch", "--show-current"])
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), "feature/use");
    }

    #[test]
    fn lock_requires_target_and_force_unlock_removes_only_typed_invalid_path() {
        let fixture = fixture();
        let repo = fixture.path();
        let empty = snapshot(repo, vec![status(repo, Some("main"), false)]);
        let error = prepare_lock(repo, &empty, "missing", "busy", "alice", "host", 1).unwrap_err();
        assert_eq!(error.code, ErrorCode::WorktreeNotFound);

        let lock = prepare_lock(repo, &empty, "main", "busy", "alice", "host", 1).unwrap();
        apply_lock(&lock).unwrap();
        let conflict = prepare_unlock(repo, "main", "bob", false).unwrap_err();
        assert_eq!(conflict.code, ErrorCode::LockConflict);

        let path = worktree_lock_file_path(repo, "main");
        fs::write(&path, "not json\n").unwrap();
        let conflict = prepare_unlock(repo, "main", "alice", false).unwrap_err();
        assert_eq!(conflict.code, ErrorCode::LockConflict);
        let stale_plan = prepare_unlock(repo, "main", "alice", true).unwrap();
        assert!(matches!(stale_plan, UnlockPlan::RemoveInvalid { .. }));
        fs::remove_file(&path).unwrap();
        upsert_worktree_lock(
            repo,
            "main",
            WorktreeLockUpdate {
                reason: "replacement",
                owner: "alice",
                host: "host",
                pid: 2,
            },
        )
        .unwrap();
        assert_eq!(
            apply_unlock(stale_plan).unwrap_err().code,
            ErrorCode::LockConflict
        );
        assert!(
            path.exists(),
            "stale force plan must not delete a valid lock"
        );

        fs::write(&path, "not json again\n").unwrap();
        let current_plan = prepare_unlock(repo, "main", "alice", true).unwrap();
        apply_unlock(current_plan).unwrap();
        assert!(!path.exists());
    }
}
