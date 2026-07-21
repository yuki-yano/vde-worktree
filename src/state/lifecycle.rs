use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::json_store::{
    JsonRecordRead, JsonRecordState, read_json_record, write_json_atomically,
    write_json_atomically_new,
};
use super::worktree_lock::branch_to_worktree_id;

const OBSERVATION_LOCK_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorktreeLifecycleRecord {
    pub schema_version: u8,
    pub branch: String,
    pub worktree_id: String,
    pub base_branch: String,
    pub ever_diverged: bool,
    pub last_diverged_head: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("lifecycle record is invalid at {path}: {reason}")]
    InvalidRecord { path: PathBuf, reason: String },
    #[error("lifecycle record does not exist at {0}")]
    Missing(PathBuf),
    #[error("lifecycle target already exists at {0}")]
    TargetExists(PathBuf),
    #[error("timed out acquiring lifecycle observation lock {0}")]
    LockTimeout(PathBuf),
    #[error("lifecycle I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to format timestamp: {0}")]
    Timestamp(#[from] time::error::Format),
}

pub fn lifecycle_file_path(repo_root: &Path, branch: &str) -> PathBuf {
    repo_root
        .join(".vde/worktree/state/branches")
        .join(format!("{}.json", branch_to_worktree_id(branch)))
}

pub fn lifecycle_observation_lock_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".vde/worktree/state/lifecycle-observation.lock")
}

pub fn read_worktree_lifecycle(
    repo_root: &Path,
    branch: &str,
) -> JsonRecordRead<WorktreeLifecycleRecord> {
    let path = lifecycle_file_path(repo_root, branch);
    let mut read = read_json_record::<WorktreeLifecycleRecord>(&path);
    if let JsonRecordState::Valid(record) = &read.state
        && let Err(reason) = validate_record(record, branch, &path)
    {
        read.state = JsonRecordState::Invalid { reason };
    }
    read
}

pub fn merge_lifecycle_observation(
    repo_root: &Path,
    branch: &str,
    base_branch: &str,
    observed_diverged_head: Option<&str>,
) -> Result<WorktreeLifecycleRecord, LifecycleError> {
    let guard = LifecycleObservationGuard::acquire(repo_root)?;
    merge_lifecycle_observation_locked(
        &guard,
        repo_root,
        branch,
        base_branch,
        observed_diverged_head,
    )
}

pub(crate) fn merge_lifecycle_observation_locked(
    _guard: &LifecycleObservationGuard,
    repo_root: &Path,
    branch: &str,
    base_branch: &str,
    observed_diverged_head: Option<&str>,
) -> Result<WorktreeLifecycleRecord, LifecycleError> {
    validate_input(branch, base_branch, observed_diverged_head)?;
    let current = read_worktree_lifecycle(repo_root, branch);
    let observed = observed_diverged_head.filter(|value| !value.is_empty());
    let now = timestamp()?;
    let (created_at, ever_diverged, last_diverged_head) = match current.state {
        JsonRecordState::Missing => (now.clone(), observed.is_some(), observed.map(str::to_owned)),
        JsonRecordState::Invalid { reason } => {
            return Err(LifecycleError::InvalidRecord {
                path: current.path,
                reason,
            });
        }
        JsonRecordState::Valid(record) => {
            if record.base_branch == base_branch && observed.is_none() {
                return Ok(record);
            }
            (
                record.created_at,
                record.ever_diverged || observed.is_some(),
                observed.map(str::to_owned).or(record.last_diverged_head),
            )
        }
    };
    let next = WorktreeLifecycleRecord {
        schema_version: 2,
        branch: branch.to_owned(),
        worktree_id: branch_to_worktree_id(branch),
        base_branch: base_branch.to_owned(),
        ever_diverged,
        last_diverged_head,
        created_at,
        updated_at: now,
    };
    write_json_atomically(&current.path, &next).map_err(|source| LifecycleError::Io {
        path: current.path,
        source,
    })?;
    Ok(next)
}

#[derive(Debug)]
pub struct LifecycleObservationGuard {
    _lock: ObservationLock,
}

impl LifecycleObservationGuard {
    pub(crate) fn acquire(repo_root: &Path) -> Result<Self, LifecycleError> {
        ObservationLock::acquire(repo_root).map(|lock| Self { _lock: lock })
    }
}

pub fn move_worktree_lifecycle(
    repo_root: &Path,
    from_branch: &str,
    to_branch: &str,
    base_branch: &str,
    observed_diverged_head: Option<&str>,
) -> Result<WorktreeLifecycleRecord, LifecycleError> {
    validate_input(to_branch, base_branch, observed_diverged_head)?;
    let _guard = ObservationLock::acquire(repo_root)?;
    let source = read_worktree_lifecycle(repo_root, from_branch);
    let source_record = require_valid(source)?;
    if from_branch == to_branch {
        return Ok(source_record);
    }
    let target_path = lifecycle_file_path(repo_root, to_branch);
    match read_worktree_lifecycle(repo_root, to_branch).state {
        JsonRecordState::Missing => {}
        JsonRecordState::Valid(_) | JsonRecordState::Invalid { .. } => {
            return Err(LifecycleError::TargetExists(target_path));
        }
    }
    let observed = observed_diverged_head.filter(|value| !value.is_empty());
    let next = WorktreeLifecycleRecord {
        schema_version: 2,
        branch: to_branch.to_owned(),
        worktree_id: branch_to_worktree_id(to_branch),
        base_branch: base_branch.to_owned(),
        ever_diverged: source_record.ever_diverged || observed.is_some(),
        last_diverged_head: observed
            .map(str::to_owned)
            .or(source_record.last_diverged_head),
        created_at: source_record.created_at,
        updated_at: timestamp()?,
    };
    write_json_atomically_new(&target_path, &next).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            LifecycleError::TargetExists(target_path.clone())
        } else {
            LifecycleError::Io {
                path: target_path.clone(),
                source,
            }
        }
    })?;
    let source_path = lifecycle_file_path(repo_root, from_branch);
    fs::remove_file(&source_path).map_err(|source| LifecycleError::Io {
        path: source_path,
        source,
    })?;
    Ok(next)
}

pub fn delete_worktree_lifecycle(repo_root: &Path, branch: &str) -> Result<(), LifecycleError> {
    let _guard = ObservationLock::acquire(repo_root)?;
    let read = read_worktree_lifecycle(repo_root, branch);
    match read.state {
        JsonRecordState::Missing => Ok(()),
        JsonRecordState::Invalid { reason } => Err(LifecycleError::InvalidRecord {
            path: read.path,
            reason,
        }),
        JsonRecordState::Valid(_) => {
            fs::remove_file(&read.path).map_err(|source| LifecycleError::Io {
                path: read.path,
                source,
            })
        }
    }
}

#[derive(Debug)]
struct ObservationLock {
    file: Option<File>,
    path: PathBuf,
}

impl ObservationLock {
    fn acquire(repo_root: &Path) -> Result<Self, LifecycleError> {
        let path = lifecycle_observation_lock_path(repo_root);
        let parent = path.parent().expect("observation lock path has a parent");
        fs::create_dir_all(parent).map_err(|source| LifecycleError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| LifecycleError::Io {
                path: path.clone(),
                source,
            })?;
        let started = Instant::now();
        loop {
            match File::try_lock(&file) {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    if started.elapsed() >= OBSERVATION_LOCK_TIMEOUT {
                        return Err(LifecycleError::LockTimeout(path));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(std::fs::TryLockError::Error(source)) => {
                    return Err(LifecycleError::Io { path, source });
                }
            }
        }
        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
            .and_then(|()| writeln!(file, "pid={}", std::process::id()))
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_all())
            .map_err(|source| LifecycleError::Io {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            file: Some(file),
            path,
        })
    }

    fn unlock(&mut self) -> Result<(), LifecycleError> {
        let Some(file) = self.file.take() else {
            return Ok(());
        };
        File::unlock(&file).map_err(|source| LifecycleError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

impl Drop for ObservationLock {
    fn drop(&mut self) {
        let _ = self.unlock();
    }
}

fn require_valid(
    read: JsonRecordRead<WorktreeLifecycleRecord>,
) -> Result<WorktreeLifecycleRecord, LifecycleError> {
    match read.state {
        JsonRecordState::Valid(record) => Ok(record),
        JsonRecordState::Missing => Err(LifecycleError::Missing(read.path)),
        JsonRecordState::Invalid { reason } => Err(LifecycleError::InvalidRecord {
            path: read.path,
            reason,
        }),
    }
}

fn validate_record(
    record: &WorktreeLifecycleRecord,
    lookup_branch: &str,
    path: &Path,
) -> Result<(), String> {
    let expected_id = branch_to_worktree_id(lookup_branch);
    if record.schema_version != 2 {
        return Err("schemaVersion must be 2".to_owned());
    }
    if record.branch != lookup_branch {
        return Err("record.branch does not match lookup branch".to_owned());
    }
    if record.worktree_id != expected_id {
        return Err("record.worktreeId does not match lookup branch".to_owned());
    }
    if path.file_name().and_then(|value| value.to_str())
        != Some(format!("{expected_id}.json").as_str())
    {
        return Err("record filename does not match worktreeId".to_owned());
    }
    for (name, value) in [
        ("branch", record.branch.as_str()),
        ("worktreeId", record.worktree_id.as_str()),
        ("baseBranch", record.base_branch.as_str()),
        ("createdAt", record.created_at.as_str()),
        ("updatedAt", record.updated_at.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{name} must be non-empty"));
        }
    }
    if record
        .last_diverged_head
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err("lastDivergedHead must be null or non-empty".to_owned());
    }
    if !record.ever_diverged && record.last_diverged_head.is_some() {
        return Err("lastDivergedHead requires everDiverged=true".to_owned());
    }
    Ok(())
}

fn validate_input(
    branch: &str,
    base_branch: &str,
    observed_diverged_head: Option<&str>,
) -> Result<(), LifecycleError> {
    for (name, value) in [("branch", branch), ("baseBranch", base_branch)] {
        if value.trim().is_empty() {
            return Err(LifecycleError::InvalidRecord {
                path: PathBuf::from("<input>"),
                reason: format!("{name} must be non-empty"),
            });
        }
    }
    if observed_diverged_head.is_some_and(str::is_empty) {
        return Err(LifecycleError::InvalidRecord {
            path: PathBuf::from("<input>"),
            reason: "observedDivergedHead must be null or non-empty".to_owned(),
        });
    }
    Ok(())
}

fn timestamp() -> Result<String, time::error::Format> {
    OffsetDateTime::now_utc().format(&Rfc3339)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_merge_preserves_divergence_and_created_at() {
        let directory = tempfile::tempdir().unwrap();
        let first =
            merge_lifecycle_observation(directory.path(), "feature/a", "main", Some("abc123"))
                .unwrap();
        let second =
            merge_lifecycle_observation(directory.path(), "feature/a", "main", None).unwrap();
        assert!(second.ever_diverged);
        assert_eq!(second.last_diverged_head.as_deref(), Some("abc123"));
        assert_eq!(second.created_at, first.created_at);
    }

    #[test]
    fn invalid_lifecycle_is_never_automatically_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = lifecycle_file_path(directory.path(), "feature/a");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{}\n").unwrap();
        assert!(matches!(
            merge_lifecycle_observation(directory.path(), "feature/a", "main", None),
            Err(LifecycleError::InvalidRecord { .. })
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), "{}\n");
    }

    #[test]
    fn rejects_lifecycle_lookup_identity_mismatches() {
        let directory = tempfile::tempdir().unwrap();
        let path = lifecycle_file_path(directory.path(), "feature/a");
        let record = WorktreeLifecycleRecord {
            schema_version: 2,
            branch: "feature/b".to_owned(),
            worktree_id: branch_to_worktree_id("feature/b"),
            base_branch: "main".to_owned(),
            ever_diverged: false,
            last_diverged_head: None,
            created_at: "now".to_owned(),
            updated_at: "now".to_owned(),
        };
        write_json_atomically(&path, &record).unwrap();
        assert!(matches!(
            read_worktree_lifecycle(directory.path(), "feature/a").state,
            JsonRecordState::Invalid { .. }
        ));
    }

    #[test]
    fn stale_observation_lock_file_does_not_block_a_new_process() {
        let directory = tempfile::tempdir().unwrap();
        let path = lifecycle_observation_lock_path(directory.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "pid=999999\n").unwrap();

        merge_lifecycle_observation(directory.path(), "feature/a", "main", None).unwrap();

        assert!(matches!(
            read_worktree_lifecycle(directory.path(), "feature/a").state,
            JsonRecordState::Valid(_)
        ));
        assert!(path.exists());
    }

    #[test]
    fn reads_a_typescript_v0_0_22_lifecycle_fixture() {
        let directory = tempfile::tempdir().unwrap();
        let branch = "feature/compat";
        let path = lifecycle_file_path(directory.path(), branch);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            include_bytes!("../../fixtures/rust-migration/typescript-lifecycle-v2.json"),
        )
        .unwrap();

        let read = read_worktree_lifecycle(directory.path(), branch);
        let JsonRecordState::Valid(record) = read.state else {
            panic!(
                "TypeScript lifecycle fixture was not accepted: {:?}",
                read.state
            );
        };
        assert_eq!(record.schema_version, 2);
        assert_eq!(record.branch, branch);
        assert!(record.ever_diverged);
        assert_eq!(
            record.last_diverged_head.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
    }
}
