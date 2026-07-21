use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::json_store::{
    JsonRecordRead, JsonRecordState, read_json_record, write_json_atomically,
    write_json_atomically_new,
};

const WORKTREE_ID_SLUG_MAX_LENGTH: usize = 48;
const WORKTREE_ID_HASH_LENGTH: usize = 12;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorktreeLockRecord {
    pub schema_version: u8,
    pub branch: String,
    pub worktree_id: String,
    pub reason: String,
    pub owner: String,
    pub host: String,
    pub pid: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug)]
pub struct WorktreeLockUpdate<'a> {
    pub reason: &'a str,
    pub owner: &'a str,
    pub host: &'a str,
    pub pid: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeUnlockOutcome {
    AlreadyUnlocked,
    RemovedValid,
    RemovedInvalid,
}

#[derive(Debug, Error)]
pub enum WorktreeLockError {
    #[error("worktree lock record is invalid at {path}: {reason}")]
    InvalidRecord { path: PathBuf, reason: String },
    #[error("worktree lock does not exist at {0}")]
    Missing(PathBuf),
    #[error("worktree lock target already exists at {0}")]
    TargetExists(PathBuf),
    #[error("worktree lock I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to format timestamp: {0}")]
    Timestamp(#[from] time::error::Format),
}

pub fn branch_to_worktree_id(branch: &str) -> String {
    let lowercase: String = branch.chars().flat_map(char::to_lowercase).collect();
    let mut slug = String::with_capacity(lowercase.len().min(WORKTREE_ID_SLUG_MAX_LENGTH));
    let mut pending_separator = false;
    for character in lowercase.chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            pending_separator = false;
            slug.push(character);
        } else if !slug.is_empty() {
            pending_separator = true;
        }
    }
    if slug.is_empty() {
        slug.push_str("branch");
    }
    slug.truncate(WORKTREE_ID_SLUG_MAX_LENGTH);

    let digest = Sha256::digest(branch.as_bytes());
    let mut hash = String::with_capacity(WORKTREE_ID_HASH_LENGTH);
    for byte in digest.iter().take(WORKTREE_ID_HASH_LENGTH / 2) {
        use std::fmt::Write as _;
        write!(hash, "{byte:02x}").expect("writing to String cannot fail");
    }
    format!("{slug}--{hash}")
}

pub fn worktree_lock_file_path(repo_root: &Path, branch: &str) -> PathBuf {
    repo_root
        .join(".vde/worktree/locks")
        .join(format!("{}.json", branch_to_worktree_id(branch)))
}

pub fn read_worktree_lock(repo_root: &Path, branch: &str) -> JsonRecordRead<WorktreeLockRecord> {
    let path = worktree_lock_file_path(repo_root, branch);
    let mut read = read_json_record::<WorktreeLockRecord>(&path);
    if let JsonRecordState::Valid(record) = &read.state
        && let Err(reason) = validate_record(record, branch, &path)
    {
        read.state = JsonRecordState::Invalid { reason };
    }
    read
}

pub fn upsert_worktree_lock(
    repo_root: &Path,
    branch: &str,
    update: WorktreeLockUpdate<'_>,
) -> Result<WorktreeLockRecord, WorktreeLockError> {
    validate_update(branch, update)?;
    let current = read_worktree_lock(repo_root, branch);
    let now = timestamp()?;
    let created_at = match current.state {
        JsonRecordState::Missing => now.clone(),
        JsonRecordState::Valid(record) => record.created_at,
        JsonRecordState::Invalid { reason } => {
            return Err(WorktreeLockError::InvalidRecord {
                path: current.path,
                reason,
            });
        }
    };
    let next = WorktreeLockRecord {
        schema_version: 1,
        branch: branch.to_owned(),
        worktree_id: branch_to_worktree_id(branch),
        reason: update.reason.to_owned(),
        owner: update.owner.to_owned(),
        host: update.host.to_owned(),
        pid: update.pid,
        created_at,
        updated_at: now,
    };
    write_json_atomically(&current.path, &next).map_err(|source| WorktreeLockError::Io {
        path: current.path,
        source,
    })?;
    Ok(next)
}

pub fn move_worktree_lock(
    repo_root: &Path,
    from_branch: &str,
    to_branch: &str,
) -> Result<WorktreeLockRecord, WorktreeLockError> {
    let source = read_worktree_lock(repo_root, from_branch);
    let source_record = require_valid(source)?;
    if from_branch == to_branch {
        return Ok(source_record);
    }
    let target_path = worktree_lock_file_path(repo_root, to_branch);
    match read_worktree_lock(repo_root, to_branch).state {
        JsonRecordState::Missing => {}
        JsonRecordState::Valid(_) | JsonRecordState::Invalid { .. } => {
            return Err(WorktreeLockError::TargetExists(target_path));
        }
    }
    let next = WorktreeLockRecord {
        schema_version: 1,
        branch: to_branch.to_owned(),
        worktree_id: branch_to_worktree_id(to_branch),
        reason: source_record.reason,
        owner: source_record.owner,
        host: source_record.host,
        pid: source_record.pid,
        created_at: source_record.created_at,
        updated_at: timestamp()?,
    };
    write_json_atomically_new(&target_path, &next).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            WorktreeLockError::TargetExists(target_path.clone())
        } else {
            WorktreeLockError::Io {
                path: target_path.clone(),
                source,
            }
        }
    })?;
    let source_path = worktree_lock_file_path(repo_root, from_branch);
    fs::remove_file(&source_path).map_err(|source| WorktreeLockError::Io {
        path: source_path,
        source,
    })?;
    Ok(next)
}

pub fn delete_worktree_lock(repo_root: &Path, branch: &str) -> Result<(), WorktreeLockError> {
    unlock_worktree_lock(repo_root, branch, false).map(|_| ())
}

/// Removes worktree lock metadata according to the public `unlock --force` contract.
///
/// Missing metadata is idempotent. Invalid metadata is preserved and reported unless
/// `force_invalid` is true; force is the only path that may explicitly remove an invalid record.
/// The caller is responsible for owner checks on valid records and must hold the repository
/// mutation lock.
pub fn unlock_worktree_lock(
    repo_root: &Path,
    branch: &str,
    force_invalid: bool,
) -> Result<WorktreeUnlockOutcome, WorktreeLockError> {
    let read = read_worktree_lock(repo_root, branch);
    match read.state {
        JsonRecordState::Missing => Ok(WorktreeUnlockOutcome::AlreadyUnlocked),
        JsonRecordState::Invalid { reason } if !force_invalid => {
            Err(WorktreeLockError::InvalidRecord {
                path: read.path,
                reason,
            })
        }
        JsonRecordState::Invalid { .. } => {
            remove_lock_file(read.path, WorktreeUnlockOutcome::RemovedInvalid)
        }
        JsonRecordState::Valid(_) => {
            remove_lock_file(read.path, WorktreeUnlockOutcome::RemovedValid)
        }
    }
}

fn remove_lock_file(
    path: PathBuf,
    outcome: WorktreeUnlockOutcome,
) -> Result<WorktreeUnlockOutcome, WorktreeLockError> {
    fs::remove_file(&path)
        .map(|()| outcome)
        .map_err(|source| WorktreeLockError::Io { path, source })
}

fn require_valid(
    read: JsonRecordRead<WorktreeLockRecord>,
) -> Result<WorktreeLockRecord, WorktreeLockError> {
    match read.state {
        JsonRecordState::Valid(record) => Ok(record),
        JsonRecordState::Missing => Err(WorktreeLockError::Missing(read.path)),
        JsonRecordState::Invalid { reason } => Err(WorktreeLockError::InvalidRecord {
            path: read.path,
            reason,
        }),
    }
}

fn validate_record(
    record: &WorktreeLockRecord,
    lookup_branch: &str,
    path: &Path,
) -> Result<(), String> {
    let expected_id = branch_to_worktree_id(lookup_branch);
    let expected_file_name = format!("{expected_id}.json");
    if record.schema_version != 1 {
        return Err("schemaVersion must be 1".to_owned());
    }
    if record.branch != lookup_branch {
        return Err("record.branch does not match lookup branch".to_owned());
    }
    if record.worktree_id != expected_id {
        return Err("record.worktreeId does not match lookup branch".to_owned());
    }
    if path.file_name().and_then(|value| value.to_str()) != Some(expected_file_name.as_str()) {
        return Err("record filename does not match worktreeId".to_owned());
    }
    for (name, value) in [
        ("branch", record.branch.as_str()),
        ("worktreeId", record.worktree_id.as_str()),
        ("reason", record.reason.as_str()),
        ("owner", record.owner.as_str()),
        ("host", record.host.as_str()),
        ("createdAt", record.created_at.as_str()),
        ("updatedAt", record.updated_at.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{name} must be non-empty"));
        }
    }
    Ok(())
}

fn validate_update(branch: &str, update: WorktreeLockUpdate<'_>) -> Result<(), WorktreeLockError> {
    for (name, value) in [
        ("branch", branch),
        ("reason", update.reason),
        ("owner", update.owner),
        ("host", update.host),
    ] {
        if value.trim().is_empty() {
            return Err(WorktreeLockError::InvalidRecord {
                path: PathBuf::from("<input>"),
                reason: format!("{name} must be non-empty"),
            });
        }
    }
    Ok(())
}

fn timestamp() -> Result<String, time::error::Format> {
    OffsetDateTime::now_utc().format(&Rfc3339)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        schema_version: u8,
        vectors: Vec<Vector>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Vector {
        branch: String,
        worktree_id: String,
    }

    #[test]
    fn worktree_id_matches_all_migration_vectors() {
        let source = include_str!("../../fixtures/rust-migration/worktree-id-vectors.json");
        let fixture: Fixture = serde_json::from_str(source).unwrap();
        assert_eq!(fixture.schema_version, 1);
        assert_eq!(fixture.vectors.len(), 19);
        for vector in fixture.vectors {
            assert_eq!(branch_to_worktree_id(&vector.branch), vector.worktree_id);
        }
    }

    #[test]
    fn invalid_record_is_not_overwritten_and_move_preserves_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let update = WorktreeLockUpdate {
            reason: "in use",
            owner: "agent",
            host: "host",
            pid: 42,
        };
        let first = upsert_worktree_lock(directory.path(), "feature/a", update).unwrap();
        let moved = move_worktree_lock(directory.path(), "feature/a", "feature/b").unwrap();
        assert_eq!(moved.reason, first.reason);
        assert_eq!(moved.owner, first.owner);
        assert_eq!(moved.created_at, first.created_at);
        assert!(matches!(
            read_worktree_lock(directory.path(), "feature/a").state,
            JsonRecordState::Missing
        ));

        let path = worktree_lock_file_path(directory.path(), "broken");
        fs::write(&path, "{}\n").unwrap();
        assert!(matches!(
            upsert_worktree_lock(directory.path(), "broken", update),
            Err(WorktreeLockError::InvalidRecord { .. })
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), "{}\n");
    }

    #[test]
    fn only_forced_unlock_removes_invalid_metadata_and_missing_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let branch = "feature/broken-lock";
        let path = worktree_lock_file_path(directory.path(), branch);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{invalid").unwrap();

        assert!(matches!(
            unlock_worktree_lock(directory.path(), branch, false),
            Err(WorktreeLockError::InvalidRecord { .. })
        ));
        assert_eq!(fs::read(&path).unwrap(), b"{invalid");
        assert_eq!(
            unlock_worktree_lock(directory.path(), branch, true).unwrap(),
            WorktreeUnlockOutcome::RemovedInvalid
        );
        assert!(!path.exists());
        assert_eq!(
            unlock_worktree_lock(directory.path(), branch, true).unwrap(),
            WorktreeUnlockOutcome::AlreadyUnlocked
        );
    }

    #[test]
    fn rejects_unknown_fields_and_lookup_identity_mismatches() {
        let directory = tempfile::tempdir().unwrap();
        let path = worktree_lock_file_path(directory.path(), "feature/a");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let record = WorktreeLockRecord {
            schema_version: 1,
            branch: "feature/b".to_owned(),
            worktree_id: branch_to_worktree_id("feature/b"),
            reason: "in use".to_owned(),
            owner: "agent".to_owned(),
            host: "host".to_owned(),
            pid: 42,
            created_at: "now".to_owned(),
            updated_at: "now".to_owned(),
        };
        write_json_atomically(&path, &record).unwrap();
        assert!(matches!(
            read_worktree_lock(directory.path(), "feature/a").state,
            JsonRecordState::Invalid { .. }
        ));

        fs::write(
            &path,
            r#"{"schemaVersion":1,"branch":"feature/a","worktreeId":"ignored","reason":"r","owner":"o","host":"h","pid":1,"createdAt":"c","updatedAt":"u","unknown":true}"#,
        )
        .unwrap();
        assert!(matches!(
            read_worktree_lock(directory.path(), "feature/a").state,
            JsonRecordState::Invalid { .. }
        ));
    }

    #[test]
    fn reads_a_typescript_v0_0_22_lock_fixture() {
        let directory = tempfile::tempdir().unwrap();
        let branch = "feature/compat";
        let path = worktree_lock_file_path(directory.path(), branch);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            include_bytes!("../../fixtures/rust-migration/typescript-lock-v1.json"),
        )
        .unwrap();

        let read = read_worktree_lock(directory.path(), branch);
        let JsonRecordState::Valid(record) = read.state else {
            panic!("TypeScript lock fixture was not accepted: {:?}", read.state);
        };
        assert_eq!(record.schema_version, 1);
        assert_eq!(record.branch, branch);
        assert_eq!(record.reason, "TypeScript v0.0.22 compatibility fixture");
        assert_eq!(record.owner, "fixture-owner");
    }
}
