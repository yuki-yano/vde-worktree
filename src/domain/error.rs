use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Ok = 0,
    NotGitRepository = 2,
    InvalidArgument = 3,
    SafetyRejected = 4,
    DependencyMissing = 5,
    LockFailed = 6,
    HookFailed = 10,
    GitCommandFailed = 20,
    ChildProcessFailed = 21,
    InternalError = 30,
    Cancelled = 130,
}

impl ExitCode {
    pub const fn value(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    NotGitRepository,
    UnsupportedRepositoryLayout,
    InvalidArgument,
    InvalidConfig,
    UnknownCommand,
    SafetyRejected,
    UnsafeFlagRequired,
    NotInitialized,
    WorktreeNotFound,
    BranchAlreadyAttached,
    BranchAlreadyExists,
    BranchInUse,
    TargetPathNotEmpty,
    PathOutsideRepo,
    AbsolutePathNotAllowed,
    LockConflict,
    DetachedHead,
    DirtyWorktree,
    UnmergedWorktree,
    UnpushedWorktree,
    LockedWorktree,
    StashApplyFailed,
    RemoteNotFound,
    RemoteBranchNotFound,
    InvalidRemoteBranchFormat,
    HookNotFound,
    DependencyMissing,
    RepoLockTimeout,
    RepoLockStaleRecoveryFailed,
    HookNotExecutable,
    HookTimeout,
    HookFailed,
    GitCommandFailed,
    ChildProcessFailed,
    InternalError,
    Cancelled,
}

impl ErrorCode {
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::NotGitRepository => ExitCode::NotGitRepository.value(),
            Self::InvalidArgument
            | Self::InvalidConfig
            | Self::UnknownCommand
            | Self::InvalidRemoteBranchFormat => ExitCode::InvalidArgument.value(),
            Self::SafetyRejected
            | Self::UnsupportedRepositoryLayout
            | Self::UnsafeFlagRequired
            | Self::NotInitialized
            | Self::WorktreeNotFound
            | Self::BranchAlreadyAttached
            | Self::BranchAlreadyExists
            | Self::BranchInUse
            | Self::TargetPathNotEmpty
            | Self::PathOutsideRepo
            | Self::AbsolutePathNotAllowed
            | Self::LockConflict
            | Self::DetachedHead
            | Self::DirtyWorktree
            | Self::UnmergedWorktree
            | Self::UnpushedWorktree
            | Self::LockedWorktree
            | Self::StashApplyFailed
            | Self::RemoteNotFound
            | Self::RemoteBranchNotFound
            | Self::HookNotFound => ExitCode::SafetyRejected.value(),
            Self::DependencyMissing => ExitCode::DependencyMissing.value(),
            Self::RepoLockTimeout | Self::RepoLockStaleRecoveryFailed => {
                ExitCode::LockFailed.value()
            }
            Self::HookNotExecutable | Self::HookTimeout | Self::HookFailed => {
                ExitCode::HookFailed.value()
            }
            Self::GitCommandFailed => ExitCode::GitCommandFailed.value(),
            Self::ChildProcessFailed => ExitCode::ChildProcessFailed.value(),
            Self::InternalError => ExitCode::InternalError.value(),
            Self::Cancelled => ExitCode::Cancelled.value(),
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        formatter.write_str(value.as_str().ok_or(fmt::Error)?)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CliError {
    pub code: ErrorCode,
    pub message: String,
    pub details: BTreeMap<String, Value>,
}

impl CliError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: BTreeMap<String, Value>) -> Self {
        self.details = details;
        self
    }

    pub const fn exit_code(&self) -> i32 {
        self.code.exit_code()
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorCode, ExitCode};

    #[test]
    fn error_codes_map_to_the_public_exit_contract() {
        assert_eq!(ExitCode::Ok.value(), 0);
        let cases = [
            (ErrorCode::NotGitRepository, 2),
            (ErrorCode::InvalidArgument, 3),
            (ErrorCode::SafetyRejected, 4),
            (ErrorCode::UnsupportedRepositoryLayout, 4),
            (ErrorCode::UnsafeFlagRequired, 4),
            (ErrorCode::DependencyMissing, 5),
            (ErrorCode::RepoLockTimeout, 6),
            (ErrorCode::HookFailed, 10),
            (ErrorCode::GitCommandFailed, 20),
            (ErrorCode::ChildProcessFailed, 21),
            (ErrorCode::InternalError, 30),
            (ErrorCode::Cancelled, 130),
        ];

        for (code, expected) in cases {
            assert_eq!(code.exit_code(), expected);
        }
        assert_eq!(
            ErrorCode::NotGitRepository.to_string(),
            "NOT_GIT_REPOSITORY"
        );
        assert_eq!(
            ErrorCode::UnsafeFlagRequired.to_string(),
            "UNSAFE_FLAG_REQUIRED"
        );
        assert_eq!(
            ErrorCode::UnsupportedRepositoryLayout.to_string(),
            "UNSUPPORTED_REPOSITORY_LAYOUT"
        );
    }
}
