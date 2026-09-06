use crate::state::metadata_transaction::MetadataTransactionError;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::adapters::fzf::{FzfCommandFailure, FzfError};
use crate::adapters::git_cli::GitCliError;
use crate::app::snapshot::SnapshotError;
use crate::domain::error::{CliError, ErrorCode, ExecutionPhase, ExecutionState};
use crate::domain::path::PathContainmentError;
use crate::ports::process::ProcessError;
use crate::state::config::ConfigError;
use crate::state::hooks::{HookError, HookOutcome, HookPhase, HookRunReport};
use crate::state::lifecycle::LifecycleError;
use crate::state::repo_lock::RepoLockError;
use crate::state::worktree_lock::WorktreeLockError;

pub trait MapToCliError {
    fn map_to_cli_error(self) -> CliError;
}

pub fn map_transaction_error(mut error: CliError, phase: ExecutionPhase) -> CliError {
    for key in [
        "recoveryPath",
        "backupPath",
        "stagedPath",
        "transactionPath",
        "recoveryPathUnavailable",
        "recoveryPathError",
        "rollbackFailures",
        "cleanupError",
        "transactionCleanupError",
    ] {
        if let Some(value) = error.details.get(key) {
            error
                .execution
                .recovery
                .insert(key.to_owned(), value.clone());
        }
    }
    let state = if error.details.get("recoveryRequired") == Some(&json!(true))
        || error.details.get("rollbackFailed") == Some(&json!(true))
    {
        ExecutionState::RecoveryRequired
    } else if error.details.get("committed") == Some(&json!(true)) {
        ExecutionState::Applied
    } else if error.details.get("committed") == Some(&json!(false)) {
        ExecutionState::RolledBack
    } else {
        ExecutionState::Unknown
    };
    let completed = if error.details.get("committed") == Some(&json!(true))
        && error.details.get("rollbackFailed") != Some(&json!(true))
    {
        &["apply"][..]
    } else {
        &[]
    };
    error.at_phase(phase, state, completed)
}

pub fn map_hook_report(report: &HookRunReport) -> CliError {
    let mut error = map_hook_outcome(&report.outcome);
    error.details.insert("hook".to_owned(), json!(report.hook));
    error
        .details
        .insert("phase".to_owned(), json!(report.phase.as_str()));
    error
        .details
        .insert("logPath".to_owned(), json!(report.log_path));
    error.at_phase(
        match report.phase {
            HookPhase::Pre => ExecutionPhase::PreHook,
            HookPhase::Post => ExecutionPhase::PostHook,
        },
        ExecutionState::Unknown,
        &[],
    )
}

pub fn map_hook_outcome(outcome: &HookOutcome) -> CliError {
    match outcome {
        HookOutcome::Missing { path } => CliError::new(
            ErrorCode::HookNotFound,
            format!("hook was not found: {}", path.display()),
        )
        .with_details(details([("path", json!(path.to_string_lossy()))])),
        HookOutcome::NonExecutable { path } => CliError::new(
            ErrorCode::HookNotExecutable,
            format!("hook is not executable: {}", path.display()),
        )
        .with_details(details([("path", json!(path.to_string_lossy()))])),
        HookOutcome::TimedOut(execution) => {
            CliError::new(ErrorCode::HookTimeout, "hook execution timed out")
                .with_details(hook_execution_details(execution))
        }
        HookOutcome::NonZero(execution) => {
            CliError::new(ErrorCode::HookFailed, "hook execution failed")
                .with_details(hook_execution_details(execution))
        }
        HookOutcome::SpawnFailure { execution, message } => {
            let mut mapped = CliError::new(ErrorCode::HookFailed, "hook execution failed")
                .with_details(hook_execution_details(execution));
            mapped.details.insert("cause".to_owned(), json!(message));
            mapped
        }
        HookOutcome::Success(_) => CliError::new(
            ErrorCode::InternalError,
            "successful hook was classified as fatal",
        ),
    }
}

fn hook_execution_details(
    execution: &crate::state::hooks::HookExecution,
) -> BTreeMap<String, Value> {
    details([
        ("exitCode", json!(execution.exit_code)),
        ("timedOut", json!(execution.timed_out)),
        ("stderr", json!(execution.stderr)),
        ("startedAt", json!(execution.started_at)),
        ("endedAt", json!(execution.ended_at)),
    ])
}

impl MapToCliError for GitCliError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        match self {
            GitCliError::NotGitRepository { cwd, stderr } => {
                CliError::new(ErrorCode::NotGitRepository, message).with_details(details([
                    ("cwd", json!(cwd.to_string_lossy())),
                    ("stderr", json!(String::from_utf8_lossy(&stderr))),
                ]))
            }
            GitCliError::UnsupportedRepositoryLayout { cwd, reason } => {
                CliError::new(ErrorCode::UnsupportedRepositoryLayout, message).with_details(
                    details([
                        ("cwd", json!(cwd.to_string_lossy())),
                        ("reason", json!(reason)),
                    ]),
                )
            }
            GitCliError::GitCommandFailed(failure) => {
                let args = failure
                    .args
                    .iter()
                    .map(|argument| argument.to_string_lossy())
                    .collect::<Vec<_>>();
                let source = failure.source.as_ref().map(ToString::to_string);
                CliError::new(ErrorCode::GitCommandFailed, message).with_details(details([
                    ("cwd", json!(failure.cwd.to_string_lossy())),
                    ("argv", json!(args)),
                    ("exitCode", json!(failure.exit_code)),
                    ("timedOut", json!(failure.timed_out)),
                    ("stdout", json!(String::from_utf8_lossy(&failure.stdout))),
                    ("stderr", json!(String::from_utf8_lossy(&failure.stderr))),
                    ("cause", json!(source)),
                ]))
            }
        }
    }
}

impl MapToCliError for PathContainmentError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        match self {
            PathContainmentError::AbsolutePathNotAllowed { path } => {
                CliError::new(ErrorCode::AbsolutePathNotAllowed, message)
                    .with_details(details([("path", json!(path.to_string_lossy()))]))
            }
            PathContainmentError::LexicalTraversal { path } => {
                CliError::new(ErrorCode::PathOutsideRepo, message)
                    .with_details(details([("path", json!(path.to_string_lossy()))]))
            }
            PathContainmentError::ManagedRootNotAllowed { root } => {
                CliError::new(ErrorCode::PathOutsideRepo, message)
                    .with_details(details([("root", json!(root.to_string_lossy()))]))
            }
            PathContainmentError::OutsideManagedRoot { root, path } => {
                CliError::new(ErrorCode::PathOutsideRepo, message).with_details(details([
                    ("root", json!(root.to_string_lossy())),
                    ("path", json!(path.to_string_lossy())),
                ]))
            }
            PathContainmentError::FileSystem { path, source } => {
                CliError::new(ErrorCode::PathOutsideRepo, message).with_details(details([
                    ("path", json!(path.to_string_lossy())),
                    ("cause", json!(source.to_string())),
                ]))
            }
        }
    }
}

impl MapToCliError for RepoLockError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        match self {
            RepoLockError::Timeout { path, timeout } => {
                CliError::new(ErrorCode::RepoLockTimeout, message).with_details(details([
                    ("path", json!(path.to_string_lossy())),
                    ("timeoutMs", json!(timeout.as_millis())),
                ]))
            }
            RepoLockError::Io { path, source } => CliError::new(ErrorCode::InternalError, message)
                .with_details(details([
                    ("path", json!(path.to_string_lossy())),
                    ("cause", json!(source.to_string())),
                ])),
        }
    }
}

impl MapToCliError for ConfigError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        let mut mapped = CliError::new(ErrorCode::InvalidConfig, message);
        mapped.details = match self {
            ConfigError::Io { path, source } => details([
                ("path", json!(path.to_string_lossy())),
                ("cause", json!(source.to_string())),
            ]),
            ConfigError::Invalid { path, reason } => details([
                ("path", json!(path.to_string_lossy())),
                ("reason", json!(reason)),
            ]),
            ConfigError::HomeUnavailable => BTreeMap::new(),
        };
        mapped
    }
}

impl MapToCliError for HookError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        match self {
            HookError::InvalidName(_) => CliError::new(ErrorCode::InvalidArgument, message),
            HookError::Io { path, source } => CliError::new(ErrorCode::InternalError, message)
                .with_details(details([
                    ("path", json!(path.to_string_lossy())),
                    ("cause", json!(source.to_string())),
                ])),
            HookError::Timestamp(source) => CliError::new(ErrorCode::InternalError, message)
                .with_details(details([("cause", json!(source.to_string()))])),
            HookError::InvalidLog(reason) => CliError::new(ErrorCode::InternalError, message)
                .with_details(details([("reason", json!(reason))])),
        }
    }
}

impl MapToCliError for WorktreeLockError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        match self {
            WorktreeLockError::InvalidRecord { path, reason } => {
                CliError::new(ErrorCode::LockConflict, message).with_details(details([
                    ("path", json!(path.to_string_lossy())),
                    ("reason", json!(reason)),
                ]))
            }
            WorktreeLockError::Missing(path) | WorktreeLockError::TargetExists(path) => {
                CliError::new(ErrorCode::LockConflict, message)
                    .with_details(details([("path", json!(path.to_string_lossy()))]))
            }
            WorktreeLockError::Io { path, source } => {
                CliError::new(ErrorCode::InternalError, message).with_details(details([
                    ("path", json!(path.to_string_lossy())),
                    ("cause", json!(source.to_string())),
                ]))
            }
            WorktreeLockError::Timestamp(source) => {
                CliError::new(ErrorCode::InternalError, message)
                    .with_details(details([("cause", json!(source.to_string()))]))
            }
        }
    }
}

impl MapToCliError for LifecycleError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        match self {
            LifecycleError::LockTimeout(path) => CliError::new(ErrorCode::RepoLockTimeout, message)
                .with_details(details([("path", json!(path.to_string_lossy()))])),
            LifecycleError::TargetExists(path) => CliError::new(ErrorCode::LockConflict, message)
                .with_details(details([("path", json!(path.to_string_lossy()))])),
            LifecycleError::InvalidRecord { path, reason } => {
                CliError::new(ErrorCode::InternalError, message).with_details(details([
                    ("path", json!(path.to_string_lossy())),
                    ("reason", json!(reason)),
                ]))
            }
            LifecycleError::Missing(path) => CliError::new(ErrorCode::InternalError, message)
                .with_details(details([("path", json!(path.to_string_lossy()))])),
            LifecycleError::Io { path, source } => CliError::new(ErrorCode::InternalError, message)
                .with_details(details([
                    ("path", json!(path.to_string_lossy())),
                    ("cause", json!(source.to_string())),
                ])),
            LifecycleError::Timestamp(source) => CliError::new(ErrorCode::InternalError, message)
                .with_details(details([("cause", json!(source.to_string()))])),
        }
    }
}

impl MapToCliError for ProcessError {
    fn map_to_cli_error(self) -> CliError {
        CliError::new(ErrorCode::ChildProcessFailed, self.to_string())
    }
}

impl MapToCliError for SnapshotError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        match self {
            SnapshotError::BaseBranchUnavailable { remote } => {
                CliError::new(ErrorCode::InvalidArgument, message)
                    .with_details(details([("remote", json!(remote))]))
            }
            SnapshotError::InvalidPorcelain(error) => {
                CliError::new(ErrorCode::GitCommandFailed, message).with_details(details([
                    ("fieldIndex", json!(error.field_index)),
                    ("reason", json!(error.reason)),
                ]))
            }
            SnapshotError::Git(failure) => {
                let argv = failure
                    .argv
                    .iter()
                    .map(|argument| argument.to_string_lossy())
                    .collect::<Vec<_>>();
                CliError::new(ErrorCode::GitCommandFailed, message).with_details(details([
                    ("cwd", json!(failure.cwd.to_string_lossy())),
                    ("argv", json!(argv)),
                    ("exitCode", json!(failure.exit_code)),
                    ("timedOut", json!(failure.timed_out)),
                    ("stdout", json!(String::from_utf8_lossy(&failure.stdout))),
                    ("stderr", json!(String::from_utf8_lossy(&failure.stderr))),
                    ("cause", json!(failure.cause)),
                ]))
            }
        }
    }
}

impl MapToCliError for FzfError {
    fn map_to_cli_error(self) -> CliError {
        let message = self.to_string();
        match self {
            FzfError::NoCandidates => CliError::new(ErrorCode::WorktreeNotFound, message),
            FzfError::InteractiveRequired
            | FzfError::DependencyMissing
            | FzfError::TmuxPopupUnsupported => {
                CliError::new(ErrorCode::DependencyMissing, message)
            }
            FzfError::InvalidArgument(reason) => CliError::new(ErrorCode::InvalidArgument, message)
                .with_details(details([("reason", json!(reason))])),
            FzfError::CapabilityCheckFailed(failure) => {
                fzf_failure(ErrorCode::DependencyMissing, message, &failure)
            }
            FzfError::CommandFailed(failure) => {
                fzf_failure(ErrorCode::ChildProcessFailed, message, &failure)
            }
            FzfError::InvalidOutput(reason) | FzfError::AmbiguousCandidate(reason) => {
                CliError::new(ErrorCode::ChildProcessFailed, message)
                    .with_details(details([("reason", json!(reason))]))
            }
            FzfError::Process(source) => CliError::new(ErrorCode::ChildProcessFailed, message)
                .with_details(details([("cause", json!(source.to_string()))])),
        }
    }
}

fn fzf_failure(code: ErrorCode, message: String, failure: &FzfCommandFailure) -> CliError {
    CliError::new(code, message).with_details(details([
        ("exitCode", json!(failure.exit_code)),
        ("timedOut", json!(failure.timed_out)),
        ("stdout", json!(String::from_utf8_lossy(&failure.stdout))),
        ("stderr", json!(String::from_utf8_lossy(&failure.stderr))),
    ]))
}

fn details<const N: usize>(entries: [(&str, Value); N]) -> BTreeMap<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

pub fn map_metadata_transaction_error(error: &MetadataTransactionError) -> CliError {
    match error {
        MetadataTransactionError::InvalidMetadata { .. }
        | MetadataTransactionError::TargetExists { .. }
        | MetadataTransactionError::PendingTransaction(_)
        | MetadataTransactionError::RecoveryConflict { .. } => {
            CliError::new(ErrorCode::LockConflict, error.to_string())
        }
        _ => CliError::new(ErrorCode::InternalError, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use serde_json::Value;

    use super::*;
    use crate::adapters::git_cli::GitCli;
    use crate::ports::process::{ProcessCommand, ProcessOutput, ProcessRunner};
    use crate::presentation::json::{ErrorEnvelope, ErrorPayload, to_stdout_json};

    struct FailingGitRunner;

    impl ProcessRunner for FailingGitRunner {
        fn run(&self, _command: &ProcessCommand) -> Result<ProcessOutput, ProcessError> {
            Ok(ProcessOutput {
                stdout: b"partial".to_vec(),
                stderr: b"fatal: broken".to_vec(),
                exit_code: Some(128),
                timed_out: false,
            })
        }
    }

    #[test]
    fn maps_a_fake_adapter_failure_through_cli_error_to_json() {
        let adapter = GitCli::new(FailingGitRunner);
        let source = adapter
            .execute_checked(Path::new("/repo"), ["status", "--porcelain"])
            .expect_err("fake git must fail");
        let error = source.map_to_cli_error();
        assert_eq!(error.code, ErrorCode::GitCommandFailed);
        assert_eq!(error.exit_code(), 20);
        assert_eq!(error.details["exitCode"], 128);

        let envelope = ErrorEnvelope::new(
            "status",
            Some("/repo".to_owned()),
            ErrorPayload::from(&error),
        );
        let stdout = to_stdout_json(&envelope).expect("serialize mapped error");
        let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
        assert_eq!(value["error"]["code"], "GIT_COMMAND_FAILED");
        assert_eq!(value["error"]["details"]["stderr"], "fatal: broken");
        assert_eq!(value["error"]["details"]["exitCode"], 128);
    }

    #[test]
    fn snapshot_git_failure_preserves_argv_and_stderr() {
        let error = SnapshotError::Git(Box::new(crate::app::snapshot::SnapshotGitFailure {
            cwd: Path::new("/repo").to_path_buf(),
            argv: vec![OsString::from("status"), OsString::from("--porcelain")],
            exit_code: Some(128),
            timed_out: false,
            stdout: Vec::new(),
            stderr: b"fatal: missing worktree".to_vec(),
            cause: None,
        }))
        .map_to_cli_error();

        assert_eq!(error.code, ErrorCode::GitCommandFailed);
        assert_eq!(error.details["argv"], json!(["status", "--porcelain"]));
        assert_eq!(error.details["stderr"], "fatal: missing worktree");
    }

    #[test]
    fn maps_unsupported_repository_layout_to_safety_exit() {
        let error = GitCliError::UnsupportedRepositoryLayout {
            cwd: "/linked/repository".into(),
            reason: "core.worktree is missing".to_owned(),
        }
        .map_to_cli_error();

        assert_eq!(error.code, ErrorCode::UnsupportedRepositoryLayout);
        assert_eq!(error.exit_code(), 4);
        assert_eq!(error.details["cwd"], "/linked/repository");
        assert_eq!(error.details["reason"], "core.worktree is missing");
    }

    #[test]
    fn preserves_public_exit_contract_for_representative_state_errors() {
        let timeout = RepoLockError::Timeout {
            path: "/repo/.git/vde-worktree.lock".into(),
            timeout: std::time::Duration::from_millis(12),
        }
        .map_to_cli_error();
        assert_eq!(timeout.code, ErrorCode::RepoLockTimeout);
        assert_eq!(timeout.exit_code(), 6);

        let conflict = WorktreeLockError::TargetExists("/repo/lock.json".into()).map_to_cli_error();
        assert_eq!(conflict.code, ErrorCode::LockConflict);
        assert_eq!(conflict.exit_code(), 4);

        let hook_timeout =
            map_hook_outcome(&HookOutcome::TimedOut(crate::state::hooks::HookExecution {
                started_at: "2026-01-01T00:00:00Z".to_owned(),
                ended_at: "2026-01-01T00:00:01Z".to_owned(),
                exit_code: None,
                timed_out: true,
                stderr: "slow".to_owned(),
            }));
        assert_eq!(hook_timeout.code, ErrorCode::HookTimeout);
        assert_eq!(hook_timeout.exit_code(), 10);
        assert_eq!(hook_timeout.details["timedOut"], true);
    }
}
