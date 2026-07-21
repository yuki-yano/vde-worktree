use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonRecordState<T> {
    Missing,
    Valid(T),
    Invalid { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonRecordRead<T> {
    pub path: PathBuf,
    pub state: JsonRecordState<T>,
}

pub fn read_json_record<T>(path: &Path) -> JsonRecordRead<T>
where
    T: DeserializeOwned,
{
    let state = match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(record) => JsonRecordState::Valid(record),
            Err(error) => JsonRecordState::Invalid {
                reason: error.to_string(),
            },
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => JsonRecordState::Missing,
        Err(error) => JsonRecordState::Invalid {
            reason: error.to_string(),
        },
    };
    JsonRecordRead {
        path: path.to_path_buf(),
        state,
    }
}

pub fn write_json_atomically<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize,
{
    let parent = parent_directory(path)?;
    fs::create_dir_all(parent)?;
    let (temporary_path, mut temporary) = create_temporary_file(path)?;
    let result = (|| {
        write_json(&mut temporary, value)?;
        drop(temporary);
        fs::rename(&temporary_path, path)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

/// Writes a complete JSON file without replacing an existing target.
///
/// A same-directory temporary file is flushed and synced first. Linking it to
/// the destination supplies the no-clobber guarantee needed by record moves.
pub fn write_json_atomically_new<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize,
{
    let parent = parent_directory(path)?;
    fs::create_dir_all(parent)?;
    let (temporary_path, mut temporary) = create_temporary_file(path)?;
    let result = (|| {
        write_json(&mut temporary, value)?;
        drop(temporary);
        fs::hard_link(&temporary_path, path)?;
        fs::remove_file(&temporary_path)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn write_json<T>(file: &mut File, value: &T) -> io::Result<()>
where
    T: Serialize,
{
    serde_json::to_writer(&mut *file, value).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_all()
}

fn create_temporary_file(target: &Path) -> io::Result<(PathBuf, File)> {
    let parent = parent_directory(target)?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "target has no UTF-8 file name")
        })?;
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = parent.join(format!(
            ".{file_name}.tmp-{}-{nanos:x}-{sequence:x}",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique atomic-write temporary file",
    ))
}

fn parent_directory(path: &Path) -> io::Result<&Path> {
    path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic JSON target must have a parent directory",
        )
    })
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Record {
        value: u8,
    }

    #[test]
    fn distinguishes_missing_valid_and_invalid_records() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("record.json");
        assert_eq!(
            read_json_record::<Record>(&path).state,
            JsonRecordState::Missing
        );

        write_json_atomically(&path, &Record { value: 3 }).unwrap();
        assert_eq!(
            read_json_record::<Record>(&path).state,
            JsonRecordState::Valid(Record { value: 3 })
        );
        assert!(fs::read(&path).unwrap().ends_with(b"\n"));

        fs::write(&path, br#"{"value":3,"unknown":true}"#).unwrap();
        assert!(matches!(
            read_json_record::<Record>(&path).state,
            JsonRecordState::Invalid { .. }
        ));
    }

    #[test]
    fn exclusive_atomic_write_never_replaces_a_target() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("record.json");
        write_json_atomically_new(&path, &Record { value: 1 }).unwrap();
        let error = write_json_atomically_new(&path, &Record { value: 2 }).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            read_json_record::<Record>(&path).state,
            JsonRecordState::Valid(Record { value: 1 })
        );
    }
}
