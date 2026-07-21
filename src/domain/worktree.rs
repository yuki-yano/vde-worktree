use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitWorktree {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrStatus {
    None,
    Open,
    Merged,
    ClosedUnmerged,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrState {
    pub status: Option<PrStatus>,
    pub url: Option<String>,
}

impl PrState {
    pub const fn unknown() -> Self {
        Self {
            status: Some(PrStatus::Unknown),
            url: None,
        }
    }

    pub const fn none() -> Self {
        Self {
            status: Some(PrStatus::None),
            url: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeLockState {
    pub value: bool,
    pub reason: Option<String>,
    pub owner: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeMergedState {
    pub by_ancestry: Option<bool>,
    #[serde(rename = "byPR")]
    pub by_pr: Option<bool>,
    pub overall: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeUpstreamState {
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub remote: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus {
    pub branch: Option<String>,
    pub path: PathBuf,
    pub head: String,
    pub dirty: bool,
    pub locked: WorktreeLockState,
    pub merged: WorktreeMergedState,
    pub pr: PrState,
    pub upstream: WorktreeUpstreamState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSnapshot {
    pub repo_root: PathBuf,
    pub base_branch: Option<String>,
    pub worktrees: Vec<WorktreeStatus>,
    #[serde(skip)]
    pub warnings: Vec<SnapshotWarning>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotWarning {
    pub code: SnapshotWarningCode,
    pub message: String,
    pub branch: Option<String>,
    pub path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SnapshotWarningCode {
    InvalidLifecycle,
    LifecycleObservationFailed,
    InvalidLock,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_json_uses_null_pr_status_and_keeps_diagnostics_off_stdout_contract() {
        let snapshot = WorktreeSnapshot {
            repo_root: PathBuf::from("/repo"),
            base_branch: Some("main".to_owned()),
            worktrees: vec![WorktreeStatus {
                branch: Some("main".to_owned()),
                path: PathBuf::from("/repo"),
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
                pr: PrState {
                    status: None,
                    url: None,
                },
                upstream: WorktreeUpstreamState {
                    ahead: None,
                    behind: None,
                    remote: None,
                },
            }],
            warnings: vec![SnapshotWarning {
                code: SnapshotWarningCode::InvalidLifecycle,
                message: "diagnostic".to_owned(),
                branch: Some("main".to_owned()),
                path: None,
            }],
        };
        let value = serde_json::to_value(snapshot).unwrap();
        assert_eq!(
            value["worktrees"][0]["pr"]["status"],
            serde_json::Value::Null
        );
        assert_eq!(
            value["worktrees"][0]["merged"]["byPR"],
            serde_json::Value::Null
        );
        assert!(value.get("warnings").is_none());
    }
}
