use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::domain::error::{CliError, ErrorCode};

const SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorPayload {
    code: ErrorCode,
    message: String,
    details: BTreeMap<String, Value>,
}

impl ErrorPayload {
    pub fn new(
        code: ErrorCode,
        message: impl Into<String>,
        details: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details,
        }
    }
}

impl From<&CliError> for ErrorPayload {
    fn from(error: &CliError) -> Self {
        Self::new(error.code, error.message.clone(), error.details.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessEnvelope<T> {
    schema_version: u8,
    command: String,
    status: SuccessStatus,
    repo_root: Option<String>,
    data: T,
    error: (),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEnvelope {
    schema_version: u8,
    command: String,
    status: ErrorStatus,
    repo_root: Option<String>,
    data: (),
    error: ErrorPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialErrorEnvelope<T> {
    schema_version: u8,
    command: String,
    status: ErrorStatus,
    repo_root: Option<String>,
    data: T,
    error: ErrorPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum SuccessStatus {
    Ok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ErrorStatus {
    Error,
}

impl<T> SuccessEnvelope<T> {
    pub fn new(command: impl Into<String>, repo_root: Option<String>, data: T) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            command: command.into(),
            status: SuccessStatus::Ok,
            repo_root,
            data,
            error: (),
        }
    }
}

impl ErrorEnvelope {
    pub fn new(command: impl Into<String>, repo_root: Option<String>, error: ErrorPayload) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            command: command.into(),
            status: ErrorStatus::Error,
            repo_root,
            data: (),
            error,
        }
    }
}

impl<T> PartialErrorEnvelope<T> {
    pub fn new(
        command: impl Into<String>,
        repo_root: Option<String>,
        data: T,
        error: ErrorPayload,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            command: command.into(),
            status: ErrorStatus::Error,
            repo_root,
            data,
            error,
        }
    }
}

mod private {
    pub trait Sealed {}
}

pub trait JsonEnvelope: private::Sealed + Serialize {}

impl<T: Serialize> private::Sealed for SuccessEnvelope<T> {}
impl private::Sealed for ErrorEnvelope {}
impl<T: Serialize> private::Sealed for PartialErrorEnvelope<T> {}

impl<T: Serialize> JsonEnvelope for SuccessEnvelope<T> {}
impl JsonEnvelope for ErrorEnvelope {}
impl<T: Serialize> JsonEnvelope for PartialErrorEnvelope<T> {}

pub fn to_stdout_json(envelope: &impl JsonEnvelope) -> Result<String, serde_json::Error> {
    let mut serialized = serde_json::to_string(envelope)?;
    serialized.push('\n');
    Ok(serialized)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{
        ErrorEnvelope, ErrorPayload, PartialErrorEnvelope, SuccessEnvelope, to_stdout_json,
    };
    use crate::cli::COMMAND_NAMES;
    use crate::domain::error::ErrorCode;

    fn parse_stdout(value: &str) -> Value {
        assert!(value.ends_with('\n'));
        assert!(!value.ends_with("\n\n"));
        serde_json::from_str(value).expect("valid envelope")
    }

    #[test]
    fn serializes_success_error_partial_and_cancel_envelopes() {
        let success =
            SuccessEnvelope::new("path", Some("/repo".into()), json!({"path": "/repo/wt"}));
        let success_value = parse_stdout(&to_stdout_json(&success).expect("serialize success"));
        assert_eq!(success_value["status"], "ok");
        assert!(success_value["error"].is_null());
        assert!(success_value["data"].is_object());

        let error = ErrorEnvelope::new(
            "path",
            Some("/repo".into()),
            ErrorPayload::new(ErrorCode::WorktreeNotFound, "missing", BTreeMap::new()),
        );
        let error_value = parse_stdout(&to_stdout_json(&error).expect("serialize error"));
        assert_eq!(error_value["status"], "error");
        assert!(error_value["data"].is_null());
        assert_eq!(error_value["error"]["code"], "WORKTREE_NOT_FOUND");

        let partial = PartialErrorEnvelope::new(
            "gone",
            Some("/repo".into()),
            json!({"deleted": ["a"], "failed": ["b"]}),
            ErrorPayload::new(
                ErrorCode::GitCommandFailed,
                "partial failure",
                BTreeMap::new(),
            ),
        );
        let partial_value = parse_stdout(&to_stdout_json(&partial).expect("serialize partial"));
        assert_eq!(partial_value["status"], "error");
        assert!(partial_value["data"].is_object());
        assert!(partial_value["error"].is_object());

        let cancelled = ErrorEnvelope::new(
            "cd",
            Some("/repo".into()),
            ErrorPayload::new(ErrorCode::Cancelled, "cancelled", BTreeMap::new()),
        );
        let cancelled_value = parse_stdout(&to_stdout_json(&cancelled).expect("serialize cancel"));
        assert_eq!(cancelled_value["error"]["code"], "CANCELLED");
        assert_eq!(ErrorCode::Cancelled.exit_code(), 130);
    }

    #[test]
    fn every_public_command_uses_the_complete_version_two_success_envelope() {
        for command in COMMAND_NAMES {
            let envelope = SuccessEnvelope::new(
                command,
                Some("/repo".into()),
                json!({"contractProbe": command}),
            );
            let value = parse_stdout(&to_stdout_json(&envelope).expect("serialize success"));
            let fields = value
                .as_object()
                .expect("success envelope object")
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();

            assert_eq!(value["schemaVersion"], 2, "{command}");
            assert_eq!(value["command"], command, "{command}");
            assert_eq!(value["status"], "ok", "{command}");
            assert_eq!(value["repoRoot"], "/repo", "{command}");
            assert_eq!(value["data"]["contractProbe"], command, "{command}");
            assert!(value["error"].is_null(), "{command}");
            assert_eq!(
                fields,
                [
                    "command",
                    "data",
                    "error",
                    "repoRoot",
                    "schemaVersion",
                    "status",
                ]
                .into_iter()
                .collect(),
                "{command}"
            );
        }
    }
}
