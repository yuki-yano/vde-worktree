//! Crash-safe lock/lifecycle metadata transitions used by `mv`.
//!
//! # Locking
//!
//! Every public operation in this module must run while the caller holds the repository mutation
//! lock. The module deliberately does not acquire that lock itself, so Git mutation and metadata
//! preflight can share one lock lifetime without lock-order inversion.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::domain::path::ValidatedManagedPath;

use super::json_store::{
    JsonRecordState, read_json_record, write_json_atomically, write_json_atomically_new,
};
use super::lifecycle::{
    LifecycleError, LifecycleObservationGuard, WorktreeLifecycleRecord, lifecycle_file_path,
    read_worktree_lifecycle,
};
use super::worktree_lock::{
    WorktreeLockRecord, branch_to_worktree_id, read_worktree_lock, worktree_lock_file_path,
};

const TRANSACTION_SCHEMA_VERSION: u8 = 2;
const TRANSACTION_ROOT: &str = ".vde/worktree/state/metadata-transactions";
const JOURNAL_FILE: &str = "journal.json";
const STAGED_LOCK_FILE: &str = "staged-lock.json";
const STAGED_LIFECYCLE_FILE: &str = "staged-lifecycle.json";
const BACKUP_LOCK_FILE: &str = "backup-lock.json";
const BACKUP_LIFECYCLE_FILE: &str = "backup-lifecycle.json";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum JournalPhase {
    Prepared,
    BranchRenamed,
    WorktreeMoved,
    CommitForward,
    Committed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MetadataTransactionJournal {
    schema_version: u8,
    transaction_id: String,
    from_branch: String,
    to_branch: String,
    source_path: PathBuf,
    target_path: PathBuf,
    managed_root: PathBuf,
    target_relative_path: PathBuf,
    source_lock_existed: bool,
    source_lifecycle_existed: bool,
    phase: JournalPhase,
}

/// Durable points at which tests or callers may simulate process termination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataTransactionStep {
    ArtifactsStaged,
    PreparedJournalWritten,
    TargetLockInstalled,
    TargetLifecycleInstalled,
    CommitForwardRecorded,
    SourceLockRemoved,
    SourceLifecycleRemoved,
    CommittedJournalWritten,
    CleanupComplete,
}

pub trait MetadataTransactionFaultInjector {
    fn after_step(&mut self, step: MetadataTransactionStep) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoMetadataTransactionFault;

impl MetadataTransactionFaultInjector for NoMetadataTransactionFault {
    fn after_step(&mut self, _step: MetadataTransactionStep) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct PreparedMetadataRename {
    repo_root: PathBuf,
    transaction_id: String,
    from_branch: String,
    to_branch: String,
    source_path: PathBuf,
    target_path: PathBuf,
    managed_root: PathBuf,
    target_relative_path: PathBuf,
    source_lock: Option<WorktreeLockRecord>,
    source_lifecycle: Option<WorktreeLifecycleRecord>,
    target_lock: Option<WorktreeLockRecord>,
    target_lifecycle: WorktreeLifecycleRecord,
}

impl PreparedMetadataRename {
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    pub fn from_branch(&self) -> &str {
        &self.from_branch
    }

    pub fn to_branch(&self) -> &str {
        &self.to_branch
    }

    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub fn target_path(&self) -> &Path {
        &self.target_path
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataRenameOutcome {
    pub transaction_id: String,
    pub lock_moved: bool,
    pub lifecycle_created: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataRecoveryResolution {
    RolledBack,
    Committed,
    OrphanRemoved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataRecoveryOutcome {
    pub transaction_id: String,
    pub resolution: MetadataRecoveryResolution,
}

#[derive(Clone, Copy, Debug)]
pub struct MetadataRenameRequest<'a> {
    pub repo_root: &'a Path,
    pub from_branch: &'a str,
    pub to_branch: &'a str,
    pub source_path: &'a Path,
    pub target_path: &'a Path,
    pub managed_root: &'a Path,
    pub target_relative_path: &'a Path,
    pub base_branch: &'a str,
    pub observed_diverged_head: Option<&'a str>,
}

#[derive(Debug, Error)]
pub enum MetadataTransactionError {
    #[error(transparent)]
    LifecycleObservation(#[from] LifecycleError),
    #[error("metadata rename requires distinct non-empty branch names")]
    InvalidBranchNames,
    #[error("base branch must be non-empty")]
    InvalidBaseBranch,
    #[error("metadata rename requires distinct absolute source and target worktree paths")]
    InvalidWorktreePaths,
    #[error("invalid {kind} metadata at {path}: {reason}")]
    InvalidMetadata {
        kind: &'static str,
        path: PathBuf,
        reason: String,
    },
    #[error("target {kind} metadata already exists at {path}")]
    TargetExists { kind: &'static str, path: PathBuf },
    #[error("pending metadata transaction already exists at {0}")]
    PendingTransaction(PathBuf),
    #[error("invalid metadata transaction journal at {path}: {reason}")]
    InvalidJournal { path: PathBuf, reason: String },
    #[error("metadata recovery conflict for {kind} at {path}")]
    RecoveryConflict { kind: &'static str, path: PathBuf },
    #[error("metadata transaction I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to format metadata timestamp: {0}")]
    Timestamp(#[from] time::error::Format),
    #[error("injected crash after {step:?}: {message}")]
    InjectedCrash {
        step: MetadataTransactionStep,
        message: String,
    },
}

/// Performs read-only validation and builds a metadata transition plan.
///
/// Call this before mutating Git. Invalid source or target records are rejected here, while both
/// source records are still untouched. A missing lock is an intentional no-op. A missing lifecycle
/// record is represented by a new v2 record using `base_branch` and the optional observation.
///
/// # Locking
///
/// The repository mutation lock must remain held from this preflight through Git mutation and
/// [`commit_metadata_rename`].
pub fn prepare_metadata_rename(
    request: MetadataRenameRequest<'_>,
) -> Result<PreparedMetadataRename, MetadataTransactionError> {
    let safe_target = validate_metadata_request(&request)?;
    let MetadataRenameRequest {
        repo_root,
        from_branch,
        to_branch,
        source_path,
        target_path: _,
        managed_root,
        target_relative_path,
        base_branch,
        observed_diverged_head,
    } = request;
    let transaction_id = transaction_id(from_branch, to_branch);
    let directory = transaction_directory(repo_root, &transaction_id);
    if directory.exists() {
        return Err(MetadataTransactionError::PendingTransaction(directory));
    }

    let source_lock_read = read_worktree_lock(repo_root, from_branch);
    let source_lock =
        valid_or_missing("source lock", source_lock_read.path, source_lock_read.state)?;
    let target_lock_read = read_worktree_lock(repo_root, to_branch);
    reject_existing("target lock", target_lock_read.path, target_lock_read.state)?;

    let source_lifecycle_read = read_worktree_lifecycle(repo_root, from_branch);
    let source_lifecycle = valid_or_missing(
        "source lifecycle",
        source_lifecycle_read.path,
        source_lifecycle_read.state,
    )?;
    let target_lifecycle_read = read_worktree_lifecycle(repo_root, to_branch);
    reject_existing(
        "target lifecycle",
        target_lifecycle_read.path,
        target_lifecycle_read.state,
    )?;

    let now = timestamp()?;
    let observed = observed_diverged_head.filter(|value| !value.is_empty());
    let target_lock = source_lock.as_ref().map(|record| WorktreeLockRecord {
        schema_version: 1,
        branch: to_branch.to_owned(),
        worktree_id: branch_to_worktree_id(to_branch),
        reason: record.reason.clone(),
        owner: record.owner.clone(),
        host: record.host.clone(),
        pid: record.pid,
        created_at: record.created_at.clone(),
        updated_at: now.clone(),
    });
    let target_lifecycle = WorktreeLifecycleRecord {
        schema_version: 2,
        branch: to_branch.to_owned(),
        worktree_id: branch_to_worktree_id(to_branch),
        base_branch: base_branch.to_owned(),
        ever_diverged: source_lifecycle
            .as_ref()
            .is_some_and(|record| record.ever_diverged)
            || observed.is_some(),
        last_diverged_head: observed
            .map(str::to_owned)
            .or_else(|| source_lifecycle.as_ref()?.last_diverged_head.clone()),
        created_at: source_lifecycle
            .as_ref()
            .map_or_else(|| now.clone(), |record| record.created_at.clone()),
        updated_at: now,
    };

    Ok(PreparedMetadataRename {
        repo_root: repo_root.to_path_buf(),
        transaction_id,
        from_branch: from_branch.to_owned(),
        to_branch: to_branch.to_owned(),
        source_path: source_path.to_path_buf(),
        target_path: safe_target,
        managed_root: managed_root.to_path_buf(),
        target_relative_path: target_relative_path.to_path_buf(),
        source_lock,
        source_lifecycle,
        target_lock,
        target_lifecycle,
    })
}

fn validate_metadata_request(
    request: &MetadataRenameRequest<'_>,
) -> Result<PathBuf, MetadataTransactionError> {
    if request.from_branch.trim().is_empty()
        || request.to_branch.trim().is_empty()
        || request.from_branch == request.to_branch
    {
        return Err(MetadataTransactionError::InvalidBranchNames);
    }
    if request.base_branch.trim().is_empty() {
        return Err(MetadataTransactionError::InvalidBaseBranch);
    }
    if !request.source_path.is_absolute()
        || !request.target_path.is_absolute()
        || request.source_path == request.target_path
        || request.target_relative_path != Path::new(request.to_branch)
    {
        return Err(MetadataTransactionError::InvalidWorktreePaths);
    }
    let validated =
        ValidatedManagedPath::validate(request.managed_root, request.target_relative_path)
            .map_err(|_| MetadataTransactionError::InvalidWorktreePaths)?;
    let safe_target = validated
        .with_revalidated_path(|path| Ok::<PathBuf, io::Error>(path.to_path_buf()))
        .map_err(|_| MetadataTransactionError::InvalidWorktreePaths)?;
    if !same_location_or_equivalent_missing(
        &safe_target,
        request.target_path,
        request.managed_root,
        request.target_relative_path,
    ) {
        return Err(MetadataTransactionError::InvalidWorktreePaths);
    }
    Ok(safe_target)
}

/// Commits a prepared metadata rename with durable journal checkpoints.
///
/// The caller must still hold the same repository mutation lock used during preflight.
pub fn commit_metadata_rename(
    plan: PreparedMetadataRename,
) -> Result<MetadataRenameOutcome, MetadataTransactionError> {
    let guard = LifecycleObservationGuard::acquire(&plan.repo_root)?;
    commit_metadata_rename_locked(plan, &guard)
}

pub(crate) fn commit_metadata_rename_locked(
    plan: PreparedMetadataRename,
    guard: &LifecycleObservationGuard,
) -> Result<MetadataRenameOutcome, MetadataTransactionError> {
    commit_metadata_rename_with_injector_locked(plan, guard, &mut NoMetadataTransactionFault)
}

/// Durably stages metadata and writes the prepared journal before any Git mutation starts.
pub fn stage_metadata_rename(
    plan: &PreparedMetadataRename,
) -> Result<(), MetadataTransactionError> {
    let paths = TransactionPaths::new(&plan.repo_root, &plan.transaction_id);
    create_transaction_directory(&paths.directory)?;
    stage_plan(plan, &paths)?;
    let journal = MetadataTransactionJournal {
        schema_version: TRANSACTION_SCHEMA_VERSION,
        transaction_id: plan.transaction_id.clone(),
        from_branch: plan.from_branch.clone(),
        to_branch: plan.to_branch.clone(),
        source_path: plan.source_path.clone(),
        target_path: plan.target_path.clone(),
        managed_root: plan.managed_root.clone(),
        target_relative_path: plan.target_relative_path.clone(),
        source_lock_existed: plan.source_lock.is_some(),
        source_lifecycle_existed: plan.source_lifecycle.is_some(),
        phase: JournalPhase::Prepared,
    };
    write_at(&paths.journal, &journal)
}

pub(crate) fn refresh_staged_lifecycle(
    plan: &mut PreparedMetadataRename,
    _guard: &LifecycleObservationGuard,
) -> Result<(), MetadataTransactionError> {
    let read = read_worktree_lifecycle(&plan.repo_root, &plan.from_branch);
    let current = valid_or_missing("source lifecycle", read.path, read.state)?;
    if let Some(record) = &current {
        plan.target_lifecycle.ever_diverged |= record.ever_diverged;
        plan.target_lifecycle.last_diverged_head = record
            .last_diverged_head
            .clone()
            .or(plan.target_lifecycle.last_diverged_head.clone());
        plan.target_lifecycle
            .created_at
            .clone_from(&record.created_at);
    }
    plan.target_lifecycle.updated_at = timestamp()?;
    plan.source_lifecycle = current;

    let paths = TransactionPaths::new(&plan.repo_root, &plan.transaction_id);
    write_at(&paths.staged_lifecycle, &plan.target_lifecycle)?;
    if let Some(record) = &plan.source_lifecycle {
        write_at(&paths.backup_lifecycle, record)?;
    } else if paths.backup_lifecycle.exists() {
        fs::remove_file(&paths.backup_lifecycle)
            .map_err(|source| io_error(paths.backup_lifecycle.clone(), source))?;
    }
    let mut journal = read_required_journal(&paths)?;
    if journal.phase != JournalPhase::Prepared {
        return Err(MetadataTransactionError::InvalidJournal {
            path: paths.journal,
            reason: "lifecycle refresh requires a prepared transaction".to_owned(),
        });
    }
    journal.source_lifecycle_existed = plan.source_lifecycle.is_some();
    write_at(&paths.journal, &journal)
}

/// Records that Git now uses the target branch while the worktree still occupies its source path.
pub fn mark_metadata_rename_branch_renamed(
    plan: &PreparedMetadataRename,
) -> Result<(), MetadataTransactionError> {
    let paths = TransactionPaths::new(&plan.repo_root, &plan.transaction_id);
    let mut journal = read_required_journal(&paths)?;
    if journal.phase != JournalPhase::Prepared {
        return Err(MetadataTransactionError::InvalidJournal {
            path: paths.journal,
            reason: "branch-renamed marker requires a prepared transaction".to_owned(),
        });
    }
    journal.phase = JournalPhase::BranchRenamed;
    write_at(&paths.journal, &journal)
}

/// Records that Git moved the renamed worktree to the validated target path.
pub fn mark_metadata_rename_worktree_moved(
    plan: &PreparedMetadataRename,
) -> Result<(), MetadataTransactionError> {
    let paths = TransactionPaths::new(&plan.repo_root, &plan.transaction_id);
    let mut journal = read_required_journal(&paths)?;
    if journal.phase != JournalPhase::BranchRenamed {
        return Err(MetadataTransactionError::InvalidJournal {
            path: paths.journal,
            reason: "worktree-moved marker requires a branch-renamed transaction".to_owned(),
        });
    }
    journal.phase = JournalPhase::WorktreeMoved;
    write_at(&paths.journal, &journal)
}

/// Removes a staged transaction after Git was not changed or was fully compensated.
pub fn rollback_staged_metadata_rename(
    plan: &PreparedMetadataRename,
) -> Result<(), MetadataTransactionError> {
    let paths = TransactionPaths::new(&plan.repo_root, &plan.transaction_id);
    if !paths.directory.exists() {
        return Ok(());
    }
    let journal = read_required_journal(&paths)?;
    rollback_prepared(&plan.repo_root, &journal, &paths)?;
    cleanup_transaction_directory(&paths.directory)
}

/// Commits a prepared rename while reporting every durable transition to `injector`.
///
/// This is primarily useful for fault-injection tests. As with [`commit_metadata_rename`], the
/// caller must still hold the same repository mutation lock used during preflight.
pub fn commit_metadata_rename_with_injector(
    plan: PreparedMetadataRename,
    injector: &mut dyn MetadataTransactionFaultInjector,
) -> Result<MetadataRenameOutcome, MetadataTransactionError> {
    let guard = LifecycleObservationGuard::acquire(&plan.repo_root)?;
    commit_metadata_rename_with_injector_locked(plan, &guard, injector)
}

fn commit_metadata_rename_with_injector_locked(
    plan: PreparedMetadataRename,
    _guard: &LifecycleObservationGuard,
    injector: &mut dyn MetadataTransactionFaultInjector,
) -> Result<MetadataRenameOutcome, MetadataTransactionError> {
    let paths = TransactionPaths::new(&plan.repo_root, &plan.transaction_id);
    let mut journal = if paths.directory.exists() {
        read_required_journal(&paths)?
    } else {
        create_transaction_directory(&paths.directory)?;
        stage_plan(&plan, &paths)?;
        inject(injector, MetadataTransactionStep::ArtifactsStaged)?;
        let journal = MetadataTransactionJournal {
            schema_version: TRANSACTION_SCHEMA_VERSION,
            transaction_id: plan.transaction_id.clone(),
            from_branch: plan.from_branch.clone(),
            to_branch: plan.to_branch.clone(),
            source_path: plan.source_path.clone(),
            target_path: plan.target_path.clone(),
            managed_root: plan.managed_root.clone(),
            target_relative_path: plan.target_relative_path.clone(),
            source_lock_existed: plan.source_lock.is_some(),
            source_lifecycle_existed: plan.source_lifecycle.is_some(),
            phase: JournalPhase::Prepared,
        };
        write_at(&paths.journal, &journal)?;
        inject(injector, MetadataTransactionStep::PreparedJournalWritten)?;
        journal
    };
    if !matches!(
        journal.phase,
        JournalPhase::Prepared | JournalPhase::BranchRenamed | JournalPhase::WorktreeMoved
    ) {
        return Err(MetadataTransactionError::InvalidJournal {
            path: paths.journal,
            reason: "metadata commit requires a pre-commit Git phase".to_owned(),
        });
    }

    if let Some(record) = &plan.target_lock {
        install_new_record(
            "lock",
            &worktree_lock_file_path(&plan.repo_root, &plan.to_branch),
            record,
        )?;
    }
    inject(injector, MetadataTransactionStep::TargetLockInstalled)?;
    install_new_record(
        "lifecycle",
        &lifecycle_file_path(&plan.repo_root, &plan.to_branch),
        &plan.target_lifecycle,
    )?;
    inject(injector, MetadataTransactionStep::TargetLifecycleInstalled)?;

    journal.phase = JournalPhase::CommitForward;
    write_at(&paths.journal, &journal)?;
    inject(injector, MetadataTransactionStep::CommitForwardRecorded)?;

    remove_source_record(
        "source lock",
        &worktree_lock_file_path(&plan.repo_root, &plan.from_branch),
        plan.source_lock.as_ref(),
    )?;
    inject(injector, MetadataTransactionStep::SourceLockRemoved)?;
    remove_source_record(
        "source lifecycle",
        &lifecycle_file_path(&plan.repo_root, &plan.from_branch),
        plan.source_lifecycle.as_ref(),
    )?;
    inject(injector, MetadataTransactionStep::SourceLifecycleRemoved)?;

    journal.phase = JournalPhase::Committed;
    write_at(&paths.journal, &journal)?;
    inject(injector, MetadataTransactionStep::CommittedJournalWritten)?;
    cleanup_transaction_directory(&paths.directory)?;
    inject(injector, MetadataTransactionStep::CleanupComplete)?;

    Ok(MetadataRenameOutcome {
        transaction_id: plan.transaction_id,
        lock_moved: plan.source_lock.is_some(),
        lifecycle_created: plan.source_lifecycle.is_none(),
    })
}

/// Recovers every transaction journal under the repository metadata directory.
///
/// Prepared transactions roll back to the source branch. Once `commitForward` is durable,
/// recovery finishes the target branch. This function is idempotent and requires the repository
/// mutation lock to be held by the caller.
pub fn recover_pending_metadata_transactions(
    repo_root: &Path,
) -> Result<Vec<MetadataRecoveryOutcome>, MetadataTransactionError> {
    let root = transaction_root(repo_root);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let _observation_guard = LifecycleObservationGuard::acquire(repo_root)?;
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(io_error(root, source)),
    };
    let mut directories = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| io_error(root.clone(), source))
        })
        .collect::<Result<Vec<_>, _>>()?;
    directories.sort();

    let mut outcomes = Vec::with_capacity(directories.len());
    for directory in directories {
        if !directory.is_dir() {
            continue;
        }
        let paths = TransactionPaths::from_directory(directory.clone());
        let journal_read = read_json_record::<MetadataTransactionJournal>(&paths.journal);
        let journal = match journal_read.state {
            JsonRecordState::Missing => {
                let id = directory
                    .file_name()
                    .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
                cleanup_transaction_directory(&directory)?;
                outcomes.push(MetadataRecoveryOutcome {
                    transaction_id: id,
                    resolution: MetadataRecoveryResolution::OrphanRemoved,
                });
                continue;
            }
            JsonRecordState::Invalid { reason } => {
                return Err(MetadataTransactionError::InvalidJournal {
                    path: paths.journal,
                    reason,
                });
            }
            JsonRecordState::Valid(journal) => journal,
        };
        validate_journal(&journal, &directory)?;
        revalidate_journal_target(repo_root, &journal)?;
        let resolution = match journal.phase {
            JournalPhase::Prepared | JournalPhase::BranchRenamed | JournalPhase::WorktreeMoved => {
                match reconcile_git_transition(repo_root, &journal)? {
                    GitRecoveryDirection::Rollback => {
                        rollback_prepared(repo_root, &journal, &paths)?;
                        MetadataRecoveryResolution::RolledBack
                    }
                    GitRecoveryDirection::Forward => {
                        finish_forward(repo_root, &journal, &paths)?;
                        MetadataRecoveryResolution::Committed
                    }
                }
            }
            JournalPhase::CommitForward | JournalPhase::Committed => {
                finish_forward(repo_root, &journal, &paths)?;
                MetadataRecoveryResolution::Committed
            }
        };
        cleanup_transaction_directory(&directory)?;
        outcomes.push(MetadataRecoveryOutcome {
            transaction_id: journal.transaction_id,
            resolution,
        });
    }
    Ok(outcomes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitRecoveryDirection {
    Rollback,
    Forward,
}

/// Reconciles both crash windows around branch rename and worktree move before metadata advances.
fn reconcile_git_transition(
    repo_root: &Path,
    journal: &MetadataTransactionJournal,
) -> Result<GitRecoveryDirection, MetadataTransactionError> {
    let from = local_branch_exists(repo_root, &journal.from_branch);
    let to = local_branch_exists(repo_root, &journal.to_branch);
    match (from, to) {
        (None, None) => Ok(GitRecoveryDirection::Rollback),
        (Some(true), Some(false)) => {
            let attached = attached_worktree_path(repo_root, &journal.from_branch)?;
            if attached
                .as_deref()
                .is_some_and(|path| same_location(path, &journal.source_path))
            {
                Ok(GitRecoveryDirection::Rollback)
            } else {
                Err(git_recovery_error(
                    repo_root,
                    journal,
                    "source branch is not attached at the journal source path",
                ))
            }
        }
        (Some(false), Some(true)) => {
            let attached = attached_worktree_path(repo_root, &journal.to_branch)?;
            match attached {
                Some(path) if same_location(&path, &journal.target_path) => {
                    Ok(GitRecoveryDirection::Forward)
                }
                Some(path) if same_location(&path, &journal.source_path) => {
                    finish_journaled_worktree_move(repo_root, journal)?;
                    Ok(GitRecoveryDirection::Forward)
                }
                _ => Err(git_recovery_error(
                    repo_root,
                    journal,
                    "target branch is not attached at either journaled path",
                )),
            }
        }
        (Some(from), Some(to)) => Err(MetadataTransactionError::InvalidJournal {
            path: transaction_directory(repo_root, &journal.transaction_id).join(JOURNAL_FILE),
            reason: format!(
                "ambiguous Git branch state during recovery (sourceExists={from}, targetExists={to})"
            ),
        }),
        _ => Err(MetadataTransactionError::InvalidJournal {
            path: transaction_directory(repo_root, &journal.transaction_id).join(JOURNAL_FILE),
            reason: "could not resolve both Git branch refs during recovery".to_owned(),
        }),
    }
}

fn attached_worktree_path(
    repo_root: &Path,
    branch: &str,
) -> Result<Option<PathBuf>, MetadataTransactionError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .map_err(|source| io_error(repo_root.to_path_buf(), source))?;
    if !output.status.success() {
        return Err(git_recovery_error_text(
            repo_root,
            format!(
                "git worktree list failed during recovery: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    let expected = format!("refs/heads/{branch}").into_bytes();
    let mut path = None;
    for field in output.stdout.split(|byte| *byte == 0) {
        if let Some(raw) = field.strip_prefix(b"worktree ") {
            path = Some(path_from_bytes(raw));
        } else if let Some(reference) = field.strip_prefix(b"branch ")
            && reference == expected
        {
            return Ok(path);
        }
    }
    Ok(None)
}

fn finish_journaled_worktree_move(
    repo_root: &Path,
    journal: &MetadataTransactionJournal,
) -> Result<(), MetadataTransactionError> {
    let parent = journal.target_path.parent().ok_or_else(|| {
        git_recovery_error(repo_root, journal, "journal target has no parent directory")
    })?;
    fs::create_dir_all(parent).map_err(|source| io_error(parent.to_path_buf(), source))?;
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("worktree")
        .arg("move")
        .arg(&journal.source_path)
        .arg(&journal.target_path)
        .output()
        .map_err(|source| io_error(repo_root.to_path_buf(), source))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(git_recovery_error(
            repo_root,
            journal,
            &format!(
                "failed to finish journaled worktree move: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ))
    }
}

fn same_location(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn same_location_or_equivalent_missing(
    safe_target: &Path,
    requested_target: &Path,
    managed_root: &Path,
    expected_relative: &Path,
) -> bool {
    same_location(safe_target, requested_target)
        || requested_target
            .strip_prefix(managed_root)
            .ok()
            .is_some_and(|relative| relative == expected_relative)
}

fn git_recovery_error(
    repo_root: &Path,
    journal: &MetadataTransactionJournal,
    reason: &str,
) -> MetadataTransactionError {
    MetadataTransactionError::InvalidJournal {
        path: transaction_directory(repo_root, &journal.transaction_id).join(JOURNAL_FILE),
        reason: reason.to_owned(),
    }
}

fn git_recovery_error_text(repo_root: &Path, reason: String) -> MetadataTransactionError {
    MetadataTransactionError::InvalidJournal {
        path: transaction_root(repo_root),
        reason,
    }
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;

    PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn local_branch_exists(repo_root: &Path, branch: &str) -> Option<bool> {
    let reference = format!("refs/heads/{branch}");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(reference)
        .output()
        .ok()?;
    match output.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

fn stage_plan(
    plan: &PreparedMetadataRename,
    paths: &TransactionPaths,
) -> Result<(), MetadataTransactionError> {
    if let Some(record) = &plan.target_lock {
        write_new_at(&paths.staged_lock, record)?;
    }
    write_new_at(&paths.staged_lifecycle, &plan.target_lifecycle)?;
    if let Some(record) = &plan.source_lock {
        write_new_at(&paths.backup_lock, record)?;
    }
    if let Some(record) = &plan.source_lifecycle {
        write_new_at(&paths.backup_lifecycle, record)?;
    }
    Ok(())
}

fn rollback_prepared(
    repo_root: &Path,
    journal: &MetadataTransactionJournal,
    paths: &TransactionPaths,
) -> Result<(), MetadataTransactionError> {
    if journal.source_lock_existed {
        let backup =
            required_staged_record::<WorktreeLockRecord>("backup lock", &paths.backup_lock)?;
        restore_record(
            "source lock",
            &worktree_lock_file_path(repo_root, &journal.from_branch),
            &backup,
        )?;
    }
    if journal.source_lifecycle_existed {
        let backup = required_staged_record::<WorktreeLifecycleRecord>(
            "backup lifecycle",
            &paths.backup_lifecycle,
        )?;
        restore_lifecycle_record(
            repo_root,
            &journal.from_branch,
            &backup,
            LifecycleRestoreMode::PreserveExisting,
        )?;
    }

    if paths.staged_lifecycle.exists() {
        let staged = required_staged_record::<WorktreeLifecycleRecord>(
            "staged lifecycle",
            &paths.staged_lifecycle,
        )?;
        remove_expected_record(
            "target lifecycle",
            &lifecycle_file_path(repo_root, &journal.to_branch),
            &staged,
        )?;
    }
    if journal.source_lock_existed {
        let staged =
            required_staged_record::<WorktreeLockRecord>("staged lock", &paths.staged_lock)?;
        remove_expected_record(
            "target lock",
            &worktree_lock_file_path(repo_root, &journal.to_branch),
            &staged,
        )?;
    }
    Ok(())
}

fn finish_forward(
    repo_root: &Path,
    journal: &MetadataTransactionJournal,
    paths: &TransactionPaths,
) -> Result<(), MetadataTransactionError> {
    let source_lock = if journal.source_lock_existed {
        Some(required_staged_record::<WorktreeLockRecord>(
            "backup lock",
            &paths.backup_lock,
        )?)
    } else {
        None
    };
    let source_lifecycle = if journal.source_lifecycle_existed {
        Some(required_staged_record::<WorktreeLifecycleRecord>(
            "backup lifecycle",
            &paths.backup_lifecycle,
        )?)
    } else {
        None
    };

    if journal.source_lock_existed {
        let staged =
            required_staged_record::<WorktreeLockRecord>("staged lock", &paths.staged_lock)?;
        restore_record(
            "target lock",
            &worktree_lock_file_path(repo_root, &journal.to_branch),
            &staged,
        )?;
    }
    let staged = required_staged_record::<WorktreeLifecycleRecord>(
        "staged lifecycle",
        &paths.staged_lifecycle,
    )?;
    restore_lifecycle_record(
        repo_root,
        &journal.to_branch,
        &staged,
        LifecycleRestoreMode::MergeExisting,
    )?;
    remove_source_record(
        "source lock",
        &worktree_lock_file_path(repo_root, &journal.from_branch),
        source_lock.as_ref(),
    )?;
    remove_source_record(
        "source lifecycle",
        &lifecycle_file_path(repo_root, &journal.from_branch),
        source_lifecycle.as_ref(),
    )
}

fn valid_or_missing<T>(
    kind: &'static str,
    path: PathBuf,
    state: JsonRecordState<T>,
) -> Result<Option<T>, MetadataTransactionError> {
    match state {
        JsonRecordState::Missing => Ok(None),
        JsonRecordState::Valid(record) => Ok(Some(record)),
        JsonRecordState::Invalid { reason } => {
            Err(MetadataTransactionError::InvalidMetadata { kind, path, reason })
        }
    }
}

fn reject_existing<T>(
    kind: &'static str,
    path: PathBuf,
    state: JsonRecordState<T>,
) -> Result<(), MetadataTransactionError> {
    match state {
        JsonRecordState::Missing => Ok(()),
        JsonRecordState::Valid(_) => Err(MetadataTransactionError::TargetExists { kind, path }),
        JsonRecordState::Invalid { reason } => {
            Err(MetadataTransactionError::InvalidMetadata { kind, path, reason })
        }
    }
}

fn install_new_record<T>(
    kind: &'static str,
    path: &Path,
    record: &T,
) -> Result<(), MetadataTransactionError>
where
    T: Serialize,
{
    write_json_atomically_new(path, record).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            MetadataTransactionError::TargetExists {
                kind,
                path: path.to_path_buf(),
            }
        } else {
            io_error(path.to_path_buf(), source)
        }
    })
}

fn restore_record<T>(
    kind: &'static str,
    path: &Path,
    expected: &T,
) -> Result<(), MetadataTransactionError>
where
    T: for<'de> Deserialize<'de> + Serialize + PartialEq,
{
    match read_json_record::<T>(path).state {
        JsonRecordState::Missing => write_json_atomically_new(path, expected)
            .map_err(|source| io_error(path.to_path_buf(), source)),
        JsonRecordState::Valid(actual) if actual == *expected => Ok(()),
        JsonRecordState::Valid(_) | JsonRecordState::Invalid { .. } => {
            Err(MetadataTransactionError::RecoveryConflict {
                kind,
                path: path.to_path_buf(),
            })
        }
    }
}

#[derive(Clone, Copy)]
enum LifecycleRestoreMode {
    PreserveExisting,
    MergeExisting,
}

fn restore_lifecycle_record(
    repo_root: &Path,
    branch: &str,
    expected: &WorktreeLifecycleRecord,
    mode: LifecycleRestoreMode,
) -> Result<(), MetadataTransactionError> {
    let read = read_worktree_lifecycle(repo_root, branch);
    match read.state {
        JsonRecordState::Missing => write_json_atomically_new(&read.path, expected)
            .map_err(|source| io_error(read.path, source)),
        JsonRecordState::Invalid { reason } => Err(MetadataTransactionError::InvalidMetadata {
            kind: "lifecycle",
            path: read.path,
            reason,
        }),
        JsonRecordState::Valid(actual) => match mode {
            LifecycleRestoreMode::PreserveExisting => Ok(()),
            LifecycleRestoreMode::MergeExisting => {
                let mut merged = expected.clone();
                merged.ever_diverged |= actual.ever_diverged;
                merged.last_diverged_head = actual.last_diverged_head.or(merged.last_diverged_head);
                merged.updated_at = actual.updated_at;
                write_json_atomically(&read.path, &merged)
                    .map_err(|source| io_error(read.path, source))
            }
        },
    }
}

fn remove_expected_record<T>(
    kind: &'static str,
    path: &Path,
    expected: &T,
) -> Result<(), MetadataTransactionError>
where
    T: for<'de> Deserialize<'de> + PartialEq,
{
    match read_json_record::<T>(path).state {
        JsonRecordState::Missing => Ok(()),
        JsonRecordState::Valid(actual) if actual == *expected => remove_if_exists(path),
        JsonRecordState::Valid(_) | JsonRecordState::Invalid { .. } => {
            Err(MetadataTransactionError::RecoveryConflict {
                kind,
                path: path.to_path_buf(),
            })
        }
    }
}

fn remove_source_record<T>(
    kind: &'static str,
    path: &Path,
    expected: Option<&T>,
) -> Result<(), MetadataTransactionError>
where
    T: for<'de> Deserialize<'de> + PartialEq,
{
    match (expected, read_json_record::<T>(path).state) {
        (_, JsonRecordState::Missing) => Ok(()),
        (Some(expected), JsonRecordState::Valid(actual)) if actual == *expected => {
            remove_if_exists(path)
        }
        (_, JsonRecordState::Valid(_) | JsonRecordState::Invalid { .. }) => {
            Err(MetadataTransactionError::RecoveryConflict {
                kind,
                path: path.to_path_buf(),
            })
        }
    }
}

fn required_staged_record<T>(kind: &'static str, path: &Path) -> Result<T, MetadataTransactionError>
where
    T: for<'de> Deserialize<'de>,
{
    match read_json_record::<T>(path).state {
        JsonRecordState::Valid(record) => Ok(record),
        JsonRecordState::Missing => Err(MetadataTransactionError::InvalidJournal {
            path: path.to_path_buf(),
            reason: format!("required {kind} is missing"),
        }),
        JsonRecordState::Invalid { reason } => Err(MetadataTransactionError::InvalidJournal {
            path: path.to_path_buf(),
            reason: format!("invalid {kind}: {reason}"),
        }),
    }
}

fn read_required_journal(
    paths: &TransactionPaths,
) -> Result<MetadataTransactionJournal, MetadataTransactionError> {
    match read_json_record::<MetadataTransactionJournal>(&paths.journal).state {
        JsonRecordState::Valid(journal) => {
            validate_journal(&journal, &paths.directory)?;
            Ok(journal)
        }
        JsonRecordState::Missing => Err(MetadataTransactionError::InvalidJournal {
            path: paths.journal.clone(),
            reason: "required journal is missing".to_owned(),
        }),
        JsonRecordState::Invalid { reason } => Err(MetadataTransactionError::InvalidJournal {
            path: paths.journal.clone(),
            reason,
        }),
    }
}

fn validate_journal(
    journal: &MetadataTransactionJournal,
    directory: &Path,
) -> Result<(), MetadataTransactionError> {
    let expected_id = transaction_id(&journal.from_branch, &journal.to_branch);
    let directory_id = directory.file_name().and_then(|value| value.to_str());
    if journal.schema_version != TRANSACTION_SCHEMA_VERSION
        || journal.transaction_id != expected_id
        || directory_id != Some(expected_id.as_str())
        || journal.from_branch.trim().is_empty()
        || journal.to_branch.trim().is_empty()
        || journal.from_branch == journal.to_branch
        || !journal.source_path.is_absolute()
        || !journal.target_path.is_absolute()
        || journal.source_path == journal.target_path
        || !journal.managed_root.is_absolute()
        || journal.target_relative_path != Path::new(&journal.to_branch)
    {
        return Err(MetadataTransactionError::InvalidJournal {
            path: directory.join(JOURNAL_FILE),
            reason: "journal identity or schema is invalid".to_owned(),
        });
    }
    Ok(())
}

fn revalidate_journal_target(
    repo_root: &Path,
    journal: &MetadataTransactionJournal,
) -> Result<(), MetadataTransactionError> {
    let validated =
        ValidatedManagedPath::validate(&journal.managed_root, &journal.target_relative_path)
            .map_err(|error| {
                git_recovery_error(
                    repo_root,
                    journal,
                    &format!("journal target containment validation failed: {error}"),
                )
            })?;
    let safe_target = validated
        .with_revalidated_path(|path| Ok::<PathBuf, io::Error>(path.to_path_buf()))
        .map_err(|error| {
            git_recovery_error(
                repo_root,
                journal,
                &format!("journal target revalidation failed: {error}"),
            )
        })?;
    if safe_target != journal.target_path {
        return Err(git_recovery_error(
            repo_root,
            journal,
            "journal target does not match its validated managed-root identity",
        ));
    }
    Ok(())
}

fn inject(
    injector: &mut dyn MetadataTransactionFaultInjector,
    step: MetadataTransactionStep,
) -> Result<(), MetadataTransactionError> {
    injector
        .after_step(step)
        .map_err(|message| MetadataTransactionError::InjectedCrash { step, message })
}

fn create_transaction_directory(path: &Path) -> Result<(), MetadataTransactionError> {
    fs::create_dir_all(path.parent().expect("transaction directory has a parent"))
        .map_err(|source| io_error(path.to_path_buf(), source))?;
    fs::create_dir(path).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            MetadataTransactionError::PendingTransaction(path.to_path_buf())
        } else {
            io_error(path.to_path_buf(), source)
        }
    })?;
    sync_parent(path)
}

fn cleanup_transaction_directory(path: &Path) -> Result<(), MetadataTransactionError> {
    match fs::remove_dir_all(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(path.to_path_buf(), source)),
    }
}

fn remove_if_exists(path: &Path) -> Result<(), MetadataTransactionError> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(path.to_path_buf(), source)),
    }
}

fn write_at<T>(path: &Path, value: &T) -> Result<(), MetadataTransactionError>
where
    T: Serialize,
{
    write_json_atomically(path, value).map_err(|source| io_error(path.to_path_buf(), source))
}

fn write_new_at<T>(path: &Path, value: &T) -> Result<(), MetadataTransactionError>
where
    T: Serialize,
{
    write_json_atomically_new(path, value).map_err(|source| io_error(path.to_path_buf(), source))
}

fn sync_parent(path: &Path) -> Result<(), MetadataTransactionError> {
    let parent = path.parent().expect("metadata path has a parent");
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error(parent.to_path_buf(), source))
}

fn io_error(path: PathBuf, source: io::Error) -> MetadataTransactionError {
    MetadataTransactionError::Io { path, source }
}

fn timestamp() -> Result<String, time::error::Format> {
    OffsetDateTime::now_utc().format(&Rfc3339)
}

fn transaction_id(from_branch: &str, to_branch: &str) -> String {
    format!(
        "{}--to--{}",
        branch_to_worktree_id(from_branch),
        branch_to_worktree_id(to_branch)
    )
}

fn transaction_root(repo_root: &Path) -> PathBuf {
    repo_root.join(TRANSACTION_ROOT)
}

fn transaction_directory(repo_root: &Path, id: &str) -> PathBuf {
    transaction_root(repo_root).join(id)
}

struct TransactionPaths {
    directory: PathBuf,
    journal: PathBuf,
    staged_lock: PathBuf,
    staged_lifecycle: PathBuf,
    backup_lock: PathBuf,
    backup_lifecycle: PathBuf,
}

impl TransactionPaths {
    fn new(repo_root: &Path, id: &str) -> Self {
        Self::from_directory(transaction_directory(repo_root, id))
    }

    fn from_directory(directory: PathBuf) -> Self {
        Self {
            journal: directory.join(JOURNAL_FILE),
            staged_lock: directory.join(STAGED_LOCK_FILE),
            staged_lifecycle: directory.join(STAGED_LIFECYCLE_FILE),
            backup_lock: directory.join(BACKUP_LOCK_FILE),
            backup_lifecycle: directory.join(BACKUP_LIFECYCLE_FILE),
            directory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::json_store::write_json_atomically;

    const OLD: &str = "feature/old";
    const NEW: &str = "feature/new";
    const BASE: &str = "main";

    struct CrashAfter(MetadataTransactionStep);

    impl MetadataTransactionFaultInjector for CrashAfter {
        fn after_step(&mut self, step: MetadataTransactionStep) -> Result<(), String> {
            if step == self.0 {
                Err("test crash".to_owned())
            } else {
                Ok(())
            }
        }
    }

    fn old_lock() -> WorktreeLockRecord {
        WorktreeLockRecord {
            schema_version: 1,
            branch: OLD.to_owned(),
            worktree_id: branch_to_worktree_id(OLD),
            reason: "do not delete".to_owned(),
            owner: "codex".to_owned(),
            host: "host".to_owned(),
            pid: 42,
            created_at: "2024-01-01T00:00:00Z".to_owned(),
            updated_at: "2024-01-01T00:00:00Z".to_owned(),
        }
    }

    fn old_lifecycle() -> WorktreeLifecycleRecord {
        WorktreeLifecycleRecord {
            schema_version: 2,
            branch: OLD.to_owned(),
            worktree_id: branch_to_worktree_id(OLD),
            base_branch: BASE.to_owned(),
            ever_diverged: true,
            last_diverged_head: Some("abc123".to_owned()),
            created_at: "2024-01-01T00:00:00Z".to_owned(),
            updated_at: "2024-01-01T00:00:00Z".to_owned(),
        }
    }

    fn initialized_repo() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        write_json_atomically(&worktree_lock_file_path(directory.path(), OLD), &old_lock())
            .unwrap();
        write_json_atomically(
            &lifecycle_file_path(directory.path(), OLD),
            &old_lifecycle(),
        )
        .unwrap();
        directory
    }

    fn prepare(
        repo: &Path,
        observed_diverged_head: Option<&str>,
    ) -> Result<PreparedMetadataRename, MetadataTransactionError> {
        prepare_metadata_rename(MetadataRenameRequest {
            repo_root: repo,
            from_branch: OLD,
            to_branch: NEW,
            source_path: &repo.join("source-worktree"),
            target_path: &repo.join(NEW),
            managed_root: repo,
            target_relative_path: Path::new(NEW),
            base_branch: BASE,
            observed_diverged_head,
        })
    }

    fn valid_lock(repo: &Path, branch: &str) -> Option<WorktreeLockRecord> {
        match read_worktree_lock(repo, branch).state {
            JsonRecordState::Valid(record) => Some(record),
            JsonRecordState::Missing => None,
            JsonRecordState::Invalid { reason } => panic!("invalid lock: {reason}"),
        }
    }

    fn valid_lifecycle(repo: &Path, branch: &str) -> Option<WorktreeLifecycleRecord> {
        match read_worktree_lifecycle(repo, branch).state {
            JsonRecordState::Valid(record) => Some(record),
            JsonRecordState::Missing => None,
            JsonRecordState::Invalid { reason } => panic!("invalid lifecycle: {reason}"),
        }
    }

    fn assert_old_state(repo: &Path) {
        assert_eq!(valid_lock(repo, OLD), Some(old_lock()));
        assert_eq!(valid_lifecycle(repo, OLD), Some(old_lifecycle()));
        assert_eq!(valid_lock(repo, NEW), None);
        assert_eq!(valid_lifecycle(repo, NEW), None);
    }

    fn assert_new_state(repo: &Path) {
        assert_eq!(valid_lock(repo, OLD), None);
        assert_eq!(valid_lifecycle(repo, OLD), None);
        let lock = valid_lock(repo, NEW).expect("new lock");
        assert_eq!(lock.owner, "codex");
        assert_eq!(lock.reason, "do not delete");
        assert_eq!(lock.created_at, "2024-01-01T00:00:00Z");
        assert_eq!(lock.branch, NEW);
        assert_eq!(lock.worktree_id, branch_to_worktree_id(NEW));
        assert_ne!(lock.updated_at, "2024-01-01T00:00:00Z");
        let lifecycle = valid_lifecycle(repo, NEW).expect("new lifecycle");
        assert!(lifecycle.ever_diverged);
        assert_eq!(lifecycle.last_diverged_head.as_deref(), Some("abc123"));
        assert_eq!(lifecycle.created_at, "2024-01-01T00:00:00Z");
        assert_eq!(lifecycle.branch, NEW);
        assert_eq!(lifecycle.worktree_id, branch_to_worktree_id(NEW));
        assert_ne!(lifecycle.updated_at, "2024-01-01T00:00:00Z");
    }

    #[test]
    fn staged_journal_precedes_git_and_commit_follows_durable_git_subphases() {
        let directory = initialized_repo();
        let repo = directory.path();
        let plan = prepare(repo, None).unwrap();

        stage_metadata_rename(&plan).unwrap();
        assert_old_state(repo);
        let paths = TransactionPaths::new(repo, plan.transaction_id());
        assert!(paths.journal.is_file());
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.journal).unwrap()).unwrap();
        assert_eq!(journal["schemaVersion"], TRANSACTION_SCHEMA_VERSION);
        assert_eq!(
            journal["sourcePath"],
            serde_json::json!(repo.join("source-worktree"))
        );
        assert_eq!(
            journal["targetPath"],
            serde_json::json!(repo.canonicalize().unwrap().join(NEW))
        );
        assert_eq!(journal["managedRoot"], serde_json::json!(repo));
        assert_eq!(journal["targetRelativePath"], NEW);
        assert_eq!(journal["phase"], "prepared");

        mark_metadata_rename_branch_renamed(&plan).unwrap();
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.journal).unwrap()).unwrap();
        assert_eq!(journal["phase"], "branchRenamed");
        mark_metadata_rename_worktree_moved(&plan).unwrap();
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.journal).unwrap()).unwrap();
        assert_eq!(journal["phase"], "worktreeMoved");
        commit_metadata_rename(plan).unwrap();
        assert_new_state(repo);
    }

    #[test]
    fn staged_journal_can_be_rolled_back_before_git_without_touching_sources() {
        let directory = initialized_repo();
        let repo = directory.path();
        let plan = prepare(repo, None).unwrap();

        stage_metadata_rename(&plan).unwrap();
        rollback_staged_metadata_rename(&plan).unwrap();

        assert_old_state(repo);
        assert!(
            !transaction_root(repo).exists()
                || fs::read_dir(transaction_root(repo))
                    .unwrap()
                    .next()
                    .is_none()
        );
    }

    #[test]
    fn recovery_detects_git_rename_completed_before_git_applied_marker() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.com"]);
        fs::write(repo.join("README.md"), "initial\n").unwrap();
        git(&["add", "README.md"]);
        git(&["commit", "-m", "initial"]);
        git(&["branch", OLD]);
        let source = repo.join("source-worktree");
        let target = repo.join(NEW);
        git(&["worktree", "add", source.to_str().unwrap(), OLD]);
        write_json_atomically(&worktree_lock_file_path(repo, OLD), &old_lock()).unwrap();
        write_json_atomically(&lifecycle_file_path(repo, OLD), &old_lifecycle()).unwrap();

        let plan = prepare_metadata_rename(MetadataRenameRequest {
            repo_root: repo,
            from_branch: OLD,
            to_branch: NEW,
            source_path: &source,
            target_path: &target,
            managed_root: repo,
            target_relative_path: Path::new(NEW),
            base_branch: BASE,
            observed_diverged_head: None,
        })
        .unwrap();
        stage_metadata_rename(&plan).unwrap();
        let rename = Command::new("git")
            .args(["branch", "-m", OLD, NEW])
            .current_dir(&source)
            .output()
            .unwrap();
        assert!(rename.status.success());

        let outcomes = recover_pending_metadata_transactions(repo).unwrap();
        assert_eq!(
            outcomes[0].resolution,
            MetadataRecoveryResolution::Committed
        );
        assert_new_state(repo);
        assert!(!source.exists());
        assert!(target.is_dir());
    }

    #[test]
    fn every_durable_fault_point_recovers_to_an_integral_old_or_new_pair() {
        let rollback_steps = [
            MetadataTransactionStep::ArtifactsStaged,
            MetadataTransactionStep::PreparedJournalWritten,
            MetadataTransactionStep::TargetLockInstalled,
            MetadataTransactionStep::TargetLifecycleInstalled,
        ];
        let forward_steps = [
            MetadataTransactionStep::CommitForwardRecorded,
            MetadataTransactionStep::SourceLockRemoved,
            MetadataTransactionStep::SourceLifecycleRemoved,
            MetadataTransactionStep::CommittedJournalWritten,
            MetadataTransactionStep::CleanupComplete,
        ];

        for step in rollback_steps.into_iter().chain(forward_steps) {
            let directory = initialized_repo();
            let plan = prepare(directory.path(), None).unwrap();
            let error = commit_metadata_rename_with_injector(plan, &mut CrashAfter(step))
                .expect_err("fault must stop commit");
            assert!(matches!(
                error,
                MetadataTransactionError::InjectedCrash { step: actual, .. } if actual == step
            ));

            // At no injected point may deletion protection disappear from both branch IDs.
            assert!(
                valid_lock(directory.path(), OLD).is_some()
                    || valid_lock(directory.path(), NEW).is_some()
            );
            recover_pending_metadata_transactions(directory.path()).unwrap();
            if rollback_steps.contains(&step) {
                assert_old_state(directory.path());
            } else {
                assert_new_state(directory.path());
            }
            assert!(
                recover_pending_metadata_transactions(directory.path())
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn missing_lock_is_noop_and_missing_lifecycle_creates_a_new_record() {
        let directory = tempfile::tempdir().unwrap();
        let plan = prepare(directory.path(), Some("diverged")).unwrap();
        let outcome = commit_metadata_rename(plan).unwrap();
        assert!(!outcome.lock_moved);
        assert!(outcome.lifecycle_created);
        assert_eq!(valid_lock(directory.path(), OLD), None);
        assert_eq!(valid_lock(directory.path(), NEW), None);
        let lifecycle = valid_lifecycle(directory.path(), NEW).unwrap();
        assert_eq!(lifecycle.base_branch, BASE);
        assert!(lifecycle.ever_diverged);
        assert_eq!(lifecycle.last_diverged_head.as_deref(), Some("diverged"));
    }

    #[test]
    fn observation_after_mv_staging_is_merged_into_the_renamed_lifecycle() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = prepare(directory.path(), None).unwrap();
        stage_metadata_rename(&plan).unwrap();

        super::super::lifecycle::merge_lifecycle_observation(
            directory.path(),
            OLD,
            BASE,
            Some("concurrent-head"),
        )
        .unwrap();
        let guard = LifecycleObservationGuard::acquire(directory.path()).unwrap();
        refresh_staged_lifecycle(&mut plan, &guard).unwrap();
        commit_metadata_rename_locked(plan, &guard).unwrap();

        assert!(valid_lifecycle(directory.path(), OLD).is_none());
        let renamed = valid_lifecycle(directory.path(), NEW).unwrap();
        assert!(renamed.ever_diverged);
        assert_eq!(
            renamed.last_diverged_head.as_deref(),
            Some("concurrent-head")
        );
    }

    #[test]
    fn forward_recovery_merges_a_concurrent_target_observation() {
        let directory = initialized_repo();
        let plan = prepare(directory.path(), None).unwrap();
        let error = commit_metadata_rename_with_injector(
            plan,
            &mut CrashAfter(MetadataTransactionStep::CommitForwardRecorded),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            MetadataTransactionError::InjectedCrash { .. }
        ));

        super::super::lifecycle::merge_lifecycle_observation(
            directory.path(),
            NEW,
            BASE,
            Some("recovery-observation"),
        )
        .unwrap();
        recover_pending_metadata_transactions(directory.path()).unwrap();

        let recovered = valid_lifecycle(directory.path(), NEW).unwrap();
        assert!(recovered.ever_diverged);
        assert_eq!(
            recovered.last_diverged_head.as_deref(),
            Some("recovery-observation")
        );
    }

    #[test]
    fn invalid_sources_and_any_existing_target_are_rejected_during_preflight() {
        for source_path in [
            worktree_lock_file_path(Path::new("REPO"), OLD),
            lifecycle_file_path(Path::new("REPO"), OLD),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let actual = directory
                .path()
                .join(source_path.strip_prefix("REPO").unwrap());
            fs::create_dir_all(actual.parent().unwrap()).unwrap();
            fs::write(&actual, b"{invalid").unwrap();
            assert!(matches!(
                prepare(directory.path(), None),
                Err(MetadataTransactionError::InvalidMetadata { .. })
            ));
            assert!(!transaction_root(directory.path()).exists());
        }

        for target_path in [
            worktree_lock_file_path(Path::new("REPO"), NEW),
            lifecycle_file_path(Path::new("REPO"), NEW),
        ] {
            let directory = initialized_repo();
            let actual = directory
                .path()
                .join(target_path.strip_prefix("REPO").unwrap());
            fs::create_dir_all(actual.parent().unwrap()).unwrap();
            fs::write(&actual, b"{}").unwrap();
            assert!(matches!(
                prepare(directory.path(), None),
                Err(MetadataTransactionError::InvalidMetadata { .. }
                    | MetadataTransactionError::TargetExists { .. })
            ));
            assert!(!transaction_root(directory.path()).exists());
        }
    }

    #[test]
    fn valid_existing_target_is_rejected_without_replacement() {
        let directory = initialized_repo();
        let target = WorktreeLifecycleRecord {
            branch: NEW.to_owned(),
            worktree_id: branch_to_worktree_id(NEW),
            ..old_lifecycle()
        };
        write_json_atomically(&lifecycle_file_path(directory.path(), NEW), &target).unwrap();
        assert!(matches!(
            prepare(directory.path(), None),
            Err(MetadataTransactionError::TargetExists {
                kind: "target lifecycle",
                ..
            })
        ));
        assert_eq!(valid_lifecycle(directory.path(), NEW), Some(target));
    }

    #[test]
    fn forward_recovery_preserves_a_source_record_changed_after_preflight() {
        let directory = initialized_repo();
        let plan = prepare(directory.path(), None).unwrap();
        let error = commit_metadata_rename_with_injector(
            plan,
            &mut CrashAfter(MetadataTransactionStep::CommitForwardRecorded),
        )
        .expect_err("fault must stop commit");
        assert!(matches!(
            error,
            MetadataTransactionError::InjectedCrash {
                step: MetadataTransactionStep::CommitForwardRecorded,
                ..
            }
        ));

        let mut changed_lock = old_lock();
        changed_lock.reason = "changed by another owner".to_owned();
        write_json_atomically(
            &worktree_lock_file_path(directory.path(), OLD),
            &changed_lock,
        )
        .unwrap();

        assert!(matches!(
            recover_pending_metadata_transactions(directory.path()),
            Err(MetadataTransactionError::RecoveryConflict {
                kind: "source lock",
                ..
            })
        ));
        assert_eq!(valid_lock(directory.path(), OLD), Some(changed_lock));
        assert!(valid_lock(directory.path(), NEW).is_some());
        assert!(valid_lifecycle(directory.path(), NEW).is_some());
    }

    #[test]
    fn orphaned_staging_directory_is_removed_without_touching_sources() {
        let directory = initialized_repo();
        let orphan = transaction_root(directory.path()).join("orphan");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join(STAGED_LIFECYCLE_FILE), b"{}").unwrap();
        let outcomes = recover_pending_metadata_transactions(directory.path()).unwrap();
        assert_eq!(
            outcomes,
            [MetadataRecoveryOutcome {
                transaction_id: "orphan".to_owned(),
                resolution: MetadataRecoveryResolution::OrphanRemoved,
            }]
        );
        assert_old_state(directory.path());
    }
}
