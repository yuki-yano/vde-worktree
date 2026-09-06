use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::domain::error::{CliError, ErrorCode, ExecutionPhase, ExecutionState};
use crate::domain::worktree::{GitWorktree, WorktreeStatus};

pub trait WorktreeIdentity {
    fn path(&self) -> &Path;
    fn branch(&self) -> Option<&str>;
}

macro_rules! identity {
    ($type:ty) => {
        impl WorktreeIdentity for $type {
            fn path(&self) -> &Path {
                &self.path
            }
            fn branch(&self) -> Option<&str> {
                self.branch.as_deref()
            }
        }
    };
}
identity!(GitWorktree);
identity!(WorktreeStatus);

pub fn optional_branch<'a, T: WorktreeIdentity>(
    worktrees: &'a [T],
    branch: &str,
) -> Result<Option<&'a T>, CliError> {
    let candidates = worktrees
        .iter()
        .filter(|item| item.branch() == Some(branch))
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [] => Ok(None),
        [item] => Ok(Some(*item)),
        _ => Err(CliError::new(
            ErrorCode::InvalidArgument,
            format!("multiple worktrees found for branch: {branch}; select a worktree path"),
        )
        .with_details(BTreeMap::from([
            ("branch".to_owned(), json!(branch)),
            (
                "candidates".to_owned(),
                json!(
                    candidates
                        .iter()
                        .map(|item| item.path())
                        .collect::<Vec<_>>()
                ),
            ),
        ]))
        .at_phase(ExecutionPhase::Resolve, ExecutionState::NotStarted, &[])),
    }
}

pub fn resolve<'a, T: WorktreeIdentity>(
    worktrees: &'a [T],
    branch: Option<&str>,
    path: Option<&Path>,
    current: &Path,
) -> Result<&'a T, CliError> {
    let selected = if let Some(branch) = branch {
        optional_branch(worktrees, branch)?
    } else if path.is_none() {
        worktrees.iter().find(|item| item.path() == current)
    } else {
        let requested = path.unwrap_or(current);
        let canonical = requested.canonicalize().map_err(|error| {
            CliError::new(
                ErrorCode::WorktreeNotFound,
                format!("cannot resolve worktree path: {error}"),
            )
            .with_details(BTreeMap::from([(
                "path".to_owned(),
                json!(requested.to_string_lossy()),
            )]))
        })?;
        worktrees
            .iter()
            .filter_map(|item| {
                let root = item.path().canonicalize().ok()?;
                canonical
                    .starts_with(&root)
                    .then_some((root.components().count(), item))
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, item)| item)
    };
    selected.ok_or_else(|| {
        CliError::new(ErrorCode::WorktreeNotFound, "worktree was not found")
            .with_details(BTreeMap::from([
                ("branch".to_owned(), json!(branch)),
                (
                    "path".to_owned(),
                    json!(path.unwrap_or(current).to_string_lossy()),
                ),
            ]))
            .at_phase(ExecutionPhase::Resolve, ExecutionState::NotStarted, &[])
    })
}

pub fn ensure_path(path: &Path) -> Result<&str, CliError> {
    path.to_str().filter(|value| !value.chars().any(char::is_control)).ok_or_else(|| {
        CliError::new(ErrorCode::UnsupportedRepositoryLayout,
            "repository paths containing non-UTF-8 or control characters are unsupported by the one-line path contract")
            .with_details(BTreeMap::from([("path".to_owned(), json!(path.to_string_lossy()))]))
    })
}
