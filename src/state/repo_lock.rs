use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOCK_FILE_NAME: &str = "vde-worktree.lock";
const RETRY_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
pub enum RepoLockError {
    Timeout {
        path: PathBuf,
        timeout: Duration,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl RepoLockError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Timeout { .. } => "REPO_LOCK_TIMEOUT",
            Self::Io { .. } => "REPO_LOCK_FAILED",
        }
    }

    pub const fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Timeout { .. } => Some(6),
            Self::Io { .. } => None,
        }
    }
}

impl fmt::Display for RepoLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout { path, timeout } => write!(
                formatter,
                "timed out after {} ms while acquiring repository lock {}",
                timeout.as_millis(),
                path.display()
            ),
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "repository lock operation failed for {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for RepoLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Timeout { .. } => None,
        }
    }
}

#[derive(Debug)]
pub struct RepoLock {
    file: Option<File>,
    path: PathBuf,
}

impl RepoLock {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn release(mut self) -> Result<(), RepoLockError> {
        self.unlock()
    }

    fn unlock(&mut self) -> Result<(), RepoLockError> {
        let Some(file) = self.file.take() else {
            return Ok(());
        };
        File::unlock(&file).map_err(|source| RepoLockError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        let _ = self.unlock();
    }
}

pub fn acquire_repo_lock(
    git_common_dir: &Path,
    timeout: Duration,
    command: &str,
) -> Result<RepoLock, RepoLockError> {
    let path = git_common_dir.join(LOCK_FILE_NAME);
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| RepoLockError::Io {
            path: path.clone(),
            source,
        })?;
    let started_at = Instant::now();

    loop {
        match File::try_lock(&file) {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) => {
                let elapsed = started_at.elapsed();
                if elapsed >= timeout {
                    return Err(RepoLockError::Timeout { path, timeout });
                }
                thread::sleep(RETRY_INTERVAL.min(timeout.saturating_sub(elapsed)));
            }
            Err(std::fs::TryLockError::Error(source)) => {
                return Err(RepoLockError::Io { path, source });
            }
        }
    }

    write_diagnostic_metadata(&mut file, &path, command)?;
    Ok(RepoLock {
        file: Some(file),
        path,
    })
}

fn write_diagnostic_metadata(
    file: &mut File,
    path: &Path,
    command: &str,
) -> Result<(), RepoLockError> {
    let started_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let metadata = format!(
        "owner=vde-worktree\npid={}\nstarted_at_unix={started_at}\ncommand={command}\n",
        std::process::id()
    );
    file.set_len(0).map_err(|source| RepoLockError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.seek(SeekFrom::Start(0))
        .map_err(|source| RepoLockError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(metadata.as_bytes())
        .and_then(|()| file.flush())
        .map_err(|source| RepoLockError::Io {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use super::{RepoLockError, acquire_repo_lock};

    #[test]
    fn contention_times_out_without_truncating_owner_metadata() {
        let common_dir = tempdir().expect("create temporary common directory");
        let owner = acquire_repo_lock(common_dir.path(), Duration::from_secs(1), "owner-command")
            .expect("acquire owner lock");
        let metadata_before = fs::read_to_string(owner.path()).expect("read lock metadata");
        let started_at = Instant::now();

        let error = acquire_repo_lock(
            common_dir.path(),
            Duration::from_millis(80),
            "contending-command",
        )
        .expect_err("contending lock must time out");

        assert!(matches!(error, RepoLockError::Timeout { .. }));
        assert_eq!(error.exit_code(), Some(6));
        assert!(started_at.elapsed() >= Duration::from_millis(80));
        assert_eq!(
            fs::read_to_string(owner.path()).expect("read lock metadata after contention"),
            metadata_before
        );
        assert!(metadata_before.contains("command=owner-command"));
    }

    #[test]
    fn explicit_release_and_drop_unlock_the_file() {
        let common_dir = tempdir().expect("create temporary common directory");
        let first = acquire_repo_lock(common_dir.path(), Duration::ZERO, "first")
            .expect("acquire first lock");
        first.release().expect("release first lock");

        let second = acquire_repo_lock(common_dir.path(), Duration::ZERO, "second")
            .expect("acquire after explicit release");
        drop(second);

        acquire_repo_lock(common_dir.path(), Duration::ZERO, "third").expect("acquire after drop");
    }
}
