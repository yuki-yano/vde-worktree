use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::adapters::git_cli::GitCli;
use crate::app::dispatch::CommandOutput;
use crate::app::error_mapper::MapToCliError;
use crate::app::error_mapper::map_metadata_transaction_error;
use crate::app::snapshot::resolve_base_branch;
use crate::cli::{CommonOptions, ParsedRequest};
use crate::domain::error::{CliError, ErrorCode, ExecutionPhase, ExecutionState};
use crate::domain::repo::RepoContext;
use crate::ports::process::{ProcessCommand, ProcessRunner};
use crate::presentation::json::ErrorPayload;
use crate::state::config::{ConfigSource, LoadedConfig, ResolvedConfig, load_resolved_config};
use crate::state::metadata_transaction::inspect_pending_metadata_transactions;

pub fn initialization_status(repo_root: &Path) -> Result<bool, CliError> {
    for leaf in ["hooks", "logs", "locks", "state"] {
        let path = repo_root.join(".vde/worktree").join(leaf);
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(CliError::new(
                    ErrorCode::InternalError,
                    format!(
                        "cannot inspect initialization at {}: {error}",
                        path.display()
                    ),
                ));
            }
        }
    }
    Ok(true)
}

pub fn apply_cli_values(config: &mut ResolvedConfig, common: &CommonOptions) {
    config.hooks.enabled = common.hooks_enabled(config.hooks.enabled);
    config.github.enabled = common.gh_enabled(config.github.enabled);
    if let Some(value) = common.hook_timeout_ms {
        config.hooks.timeout_ms = value;
    }
    if let Some(value) = common.lock_timeout_ms {
        config.locks.timeout_ms = value;
    }
    if let Some(value) = &common.prompt {
        config.selector.cd.prompt.clone_from(value);
    }
    config
        .selector
        .cd
        .fzf
        .extra_args
        .extend(common.fzf_args.clone());
}

pub fn effective_configuration(mut loaded: LoadedConfig, common: &CommonOptions) -> LoadedConfig {
    apply_cli_values(&mut loaded.config, common);
    for (key, argument) in [
        (
            "hooks.enabled",
            if common.hooks {
                Some("--hooks")
            } else if common.no_hooks {
                Some("--no-hooks")
            } else {
                None
            },
        ),
        (
            "github.enabled",
            if common.gh {
                Some("--gh")
            } else if common.no_gh {
                Some("--no-gh")
            } else {
                None
            },
        ),
        (
            "hooks.timeoutMs",
            common.hook_timeout_ms.map(|_| "--hook-timeout-ms"),
        ),
        (
            "locks.timeoutMs",
            common.lock_timeout_ms.map(|_| "--lock-timeout-ms"),
        ),
        (
            "selector.cd.prompt",
            common.prompt.as_ref().map(|_| "--prompt"),
        ),
    ] {
        if let Some(argument) = argument {
            loaded.sources.insert(
                key.to_owned(),
                vec![ConfigSource::CommandLine {
                    argument: argument.to_owned(),
                }],
            );
        }
    }
    if !common.fzf_args.is_empty() {
        loaded
            .sources
            .entry("selector.cd.fzf.extraArgs".to_owned())
            .or_default()
            .push(ConfigSource::CommandLine {
                argument: "--fzf-arg".to_owned(),
            });
    }
    loaded
}

pub fn configuration_value(loaded: &LoadedConfig) -> Result<Value, CliError> {
    for path in &loaded.loaded_files {
        crate::app::target::ensure_path(path)?;
    }
    Ok(
        json!({ "loadedFiles": loaded.loaded_files, "sources": loaded.sources, "effective": loaded.config }),
    )
}

fn repository_value(context: &RepoContext) -> Value {
    json!({ "repoRoot": context.repo_root.to_string_lossy(), "currentWorktreeRoot": context.current_worktree_root.to_string_lossy(), "gitCommonDir": context.git_common_dir.to_string_lossy() })
}

pub fn context_output<R: ProcessRunner + Sync>(
    request: &ParsedRequest,
    cwd: &Path,
    context: &RepoContext,
    git: &GitCli<R>,
) -> Result<CommandOutput, CliError> {
    for path in [
        cwd,
        context.repo_root.as_path(),
        context.current_worktree_root.as_path(),
        context.git_common_dir.as_path(),
    ] {
        crate::app::target::ensure_path(path)?;
    }
    let loaded = effective_configuration(
        load_resolved_config(cwd, &context.repo_root).map_err(MapToCliError::map_to_cli_error)?,
        &request.common,
    );
    let base = resolve_base_branch(
        git,
        &context.repo_root,
        loaded.config.git.base_branch.as_deref(),
        &loaded.config.git.base_remote,
    )
    .map_err(MapToCliError::map_to_cli_error);
    let pending = inspect_pending_metadata_transactions(&context.repo_root)
        .map_err(|error| map_metadata_transaction_error(&error))?;
    let managed = context.repo_root.join(&loaded.config.paths.worktree_root);
    let initialized = initialization_status(&context.repo_root)?;
    let mut output = CommandOutput::new(json!({
        "executionDirectory": cwd,
        "repository": repository_value(context),
        "initialized": initialized,
        "managedWorktreeRoot": managed,
        "baseBranch": base.as_ref().ok(),
        "baseBranchError": base.as_ref().err().map(ErrorPayload::from),
        "config": configuration_value(&loaded)?,
        "pendingRecoveries": serde_json::to_value(&pending).map_err(|error| CliError::new(ErrorCode::UnsupportedRepositoryLayout, error.to_string()))?,
    }));
    output.human_stdout = format!(
        "directory: {}\nrepository: {}\nworktree: {}\ninitialized: {initialized}\nmanaged root: {}\nbase branch: {}\nconfig files: {}\npending recoveries: {}\n",
        cwd.display(),
        context.repo_root.display(),
        context.current_worktree_root.display(),
        managed.display(),
        base.as_deref().unwrap_or("(unavailable)"),
        loaded
            .loaded_files
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        pending.len()
    );
    if let Err(error) = base {
        let _ = writeln!(output.human_stderr, "Warning: {}", error.message);
        output.warnings.push(error);
    }
    Ok(output)
}

fn check(name: &str, status: &str, message: impl Into<String>, details: Value) -> Value {
    let mut value = json!({ "name": name, "status": status, "message": message.into() });
    value["details"] = details;
    value
}

fn error_check(name: &str, error: &CliError) -> Value {
    check(
        name,
        "error",
        &error.message,
        json!(ErrorPayload::from(error)),
    )
}

fn dependency_probe(
    runner: &impl ProcessRunner,
    cwd: &Path,
    program: &str,
    arguments: &[&str],
    required: bool,
    expose_output: bool,
) -> Value {
    let mut command = ProcessCommand::new(program);
    command.args = arguments.iter().map(std::ffi::OsString::from).collect();
    command.cwd = Some(cwd.to_path_buf());
    command.timeout = Some(Duration::from_secs(5));
    match runner.run(&command) {
        Ok(output) => {
            let success =
                output.exit_code == Some(0) && !output.timed_out && !output.is_truncated();
            check(
                program,
                if success {
                    "ok"
                } else if required {
                    "error"
                } else {
                    "warning"
                },
                if success { "available" } else { "probe failed" },
                json!({
                    "exitCode": output.exit_code, "timedOut": output.timed_out,
                    "signal": output.signal, "stdoutTruncated": output.stdout_truncated, "stderrTruncated": output.stderr_truncated,
                    "version": expose_output.then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()),
                }),
            )
        }
        Err(error) => check(
            program,
            if required { "error" } else { "warning" },
            error.to_string(),
            json!({}),
        ),
    }
}

fn repository_checks<R: ProcessRunner + Sync>(
    request: &ParsedRequest,
    cwd: &Path,
    context: &Result<RepoContext, CliError>,
    git: &GitCli<R>,
    checks: &mut Vec<Value>,
) -> (Value, Value, Option<bool>) {
    let mut config_value = Value::Null;
    let mut pending_value = json!([]);
    let mut effective_gh = None;
    match &context {
        Err(error) => checks.push(error_check("repository", error)),
        Ok(context) => {
            checks.push(check(
                "repository",
                "ok",
                "Git repository resolved",
                repository_value(context),
            ));
            for path in [
                cwd,
                context.repo_root.as_path(),
                context.current_worktree_root.as_path(),
                context.git_common_dir.as_path(),
            ] {
                if let Err(error) = crate::app::target::ensure_path(path) {
                    checks.push(error_check("repositoryPaths", &error));
                    break;
                }
            }
            match initialization_status(&context.repo_root) {
                Ok(initialized) => checks.push(check(
                    "initialization",
                    if initialized { "ok" } else { "error" },
                    if initialized {
                        "repository state directories exist"
                    } else {
                        "run vw init to initialize repository state"
                    },
                    json!({ "initialized": initialized }),
                )),
                Err(error) => checks.push(error_check("initialization", &error)),
            }
            match load_resolved_config(cwd, &context.repo_root) {
                Ok(loaded) => {
                    let loaded = effective_configuration(loaded, &request.common);
                    effective_gh = Some(loaded.config.github.enabled);
                    match configuration_value(&loaded) {
                        Ok(value) => config_value = value,
                        Err(error) => checks.push(error_check("configurationPaths", &error)),
                    }
                    checks.push(check(
                        "configuration",
                        "ok",
                        "configuration is valid",
                        json!({ "loadedFiles": loaded.loaded_files.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>() }),
                    ));
                    match resolve_base_branch(
                        git,
                        &context.repo_root,
                        loaded.config.git.base_branch.as_deref(),
                        &loaded.config.git.base_remote,
                    ) {
                        Ok(base) => checks.push(check(
                            "baseBranch",
                            "ok",
                            "base branch resolved",
                            json!({ "branch": base }),
                        )),
                        Err(error) => {
                            checks.push(error_check("baseBranch", &error.map_to_cli_error()));
                        }
                    }
                }
                Err(error) => checks.push(error_check("configuration", &error.map_to_cli_error())),
            }
            match inspect_pending_metadata_transactions(&context.repo_root) {
                Ok(pending) => {
                    checks.push(check(
                        "pendingRecoveries",
                        if pending.is_empty() { "ok" } else { "error" },
                        if pending.is_empty() {
                            "no pending metadata transactions"
                        } else {
                            "pending metadata transactions require recovery before mutation"
                        },
                        json!({ "count": pending.len() }),
                    ));
                    match serde_json::to_value(pending) {
                        Ok(value) => pending_value = value,
                        Err(error) => checks.push(check(
                            "pendingRecoveryPaths",
                            "error",
                            error.to_string(),
                            json!({}),
                        )),
                    }
                }
                Err(error) => checks.push(error_check(
                    "pendingRecoveries",
                    &map_metadata_transaction_error(&error),
                )),
            }
        }
    }
    (config_value, pending_value, effective_gh)
}

/// Reports independent failures even when repository resolution or configuration is invalid.
/// No repository locks, lifecycle observations, hooks or recovery are run by this command.
pub fn doctor_output<R: ProcessRunner + Sync>(
    request: &ParsedRequest,
    cwd: &Path,
    context: &Result<RepoContext, CliError>,
    git: &GitCli<R>,
    runner: &R,
) -> CommandOutput {
    let mut checks = vec![dependency_probe(
        runner,
        cwd,
        "git",
        &["--version"],
        true,
        true,
    )];
    let (config_value, pending_value, effective_gh) =
        repository_checks(request, cwd, context, git, &mut checks);
    if effective_gh == Some(true) {
        let probe = dependency_probe(runner, cwd, "gh", &["--version"], false, true);
        let available = probe["status"] == "ok";
        checks.push(probe);
        if available {
            let mut authentication =
                dependency_probe(runner, cwd, "gh", &["auth", "status"], false, false);
            authentication["name"] = json!("githubAuthentication");
            checks.push(authentication);
        }
    } else {
        checks.push(check(
            "gh",
            "skipped",
            if effective_gh == Some(false) {
                "GitHub integration is disabled"
            } else {
                "effective configuration is unavailable"
            },
            json!({}),
        ));
    }
    checks.push(dependency_probe(
        runner,
        cwd,
        "fzf",
        &["--version"],
        false,
        true,
    ));
    checks.push(dependency_probe(runner, cwd, "tmux", &["-V"], false, true));
    let failed = checks
        .iter()
        .filter(|check| check["status"] == "error")
        .map(|check| check["name"].clone())
        .collect::<Vec<_>>();
    let healthy = failed.is_empty();
    let mut output = CommandOutput::new(json!({
        "executionDirectory": cwd.to_string_lossy(), "repository": context.as_ref().ok().map(repository_value),
        "healthy": healthy, "config": config_value, "checks": checks, "pendingRecoveries": pending_value,
    }));
    for check in &checks {
        let _ = writeln!(
            output.human_stdout,
            "{}: {} — {}",
            check["name"].as_str().unwrap_or("check"),
            check["status"].as_str().unwrap_or("unknown"),
            check["message"].as_str().unwrap_or("")
        );
    }
    if !healthy {
        output.partial_error = Some(
            CliError::new(
                ErrorCode::SafetyRejected,
                "doctor found repository setup issues",
            )
            .with_details(BTreeMap::from([("failedChecks".to_owned(), json!(failed))]))
            .at_phase(ExecutionPhase::Preflight, ExecutionState::NotStarted, &[]),
        );
    }
    output
}
