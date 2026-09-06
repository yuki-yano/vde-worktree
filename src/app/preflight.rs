//! Read-only inspection results. Inspection does not reserve a target or authorize a later apply.
use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::app::dispatch::CommandOutput;
use crate::cli::ParsedRequest;
use crate::domain::error::{CliError, ExecutionPhase, ExecutionState};
use crate::presentation::json::ErrorPayload;
use crate::state::metadata_transaction::PendingMetadataTransaction;

pub fn output(
    request: &ParsedRequest,
    target: &Value,
    planned_result: &Value,
    evidence: &Value,
    pending: Vec<PendingMetadataTransaction>,
    errors: Vec<CliError>,
) -> Result<CommandOutput, CliError> {
    let errors = errors
        .into_iter()
        .map(|error| error.at_phase(ExecutionPhase::Preflight, ExecutionState::NotStarted, &[]))
        .collect::<Vec<_>>();
    let pending = serde_json::to_value(pending).map_err(|error| {
        CliError::new(
            crate::domain::error::ErrorCode::InvalidMetadata,
            format!("cannot represent pending recovery paths: {error}"),
        )
    })?;
    let data = json!({
        "dryRun": true,
        "command": request.command.name(),
        "allowed": errors.is_empty(),
        "target": target,
        "plannedResult": planned_result,
        "effects": crate::cli::contract::command_effects(request.command.name()),
        "evidence": evidence,
        "pendingRecoveries": pending,
        "rejections": errors.iter().map(ErrorPayload::from).collect::<Vec<_>>(),
        "requiresRevalidation": true,
    });
    let mut output = CommandOutput::new(data);
    output.partial_error = errors.first().cloned();
    let _ = writeln!(
        output.human_stdout,
        "{}: {} (inspection only)",
        request.command.name(),
        if errors.is_empty() {
            "allowed"
        } else {
            "blocked"
        }
    );
    if !target.is_null() {
        let _ = writeln!(output.human_stdout, "Target: {target}");
    }
    for error in &errors {
        let _ = writeln!(output.human_stdout, "[{}] {}", error.code, error.message);
    }
    if !planned_result.is_null() {
        let _ = writeln!(output.human_stdout, "Plan: {planned_result}");
    }
    if !evidence.is_null() {
        let _ = writeln!(output.human_stdout, "Evidence: {evidence}");
    }
    Ok(output)
}

/// Use the exact snapshot checked by preparation so the decision and evidence cannot diverge.
pub fn deletion_evidence(
    repo_root: &std::path::Path,
    managed_root: &std::path::Path,
    snapshot: &crate::domain::worktree::WorktreeSnapshot,
    force: crate::app::mutations_delete::DeleteForceOptions,
    gone: bool,
) -> Result<Value, CliError> {
    crate::app::target::ensure_path(repo_root)?;
    let targets = snapshot.worktrees.iter().map(|target| {
        crate::app::target::ensure_path(&target.path)?;
        let errors = crate::app::mutations_delete::inspect_delete_guards(repo_root, managed_root, snapshot, target, force, gone);
        Ok(json!({"worktree": target, "eligible": errors.is_empty(), "rejections": errors.iter().map(ErrorPayload::from).collect::<Vec<_>>()}))
    }).collect::<Result<Vec<_>, CliError>>()?;
    Ok(json!({"baseBranch": snapshot.base_branch, "checksUpstream": !gone, "targets": targets}))
}
