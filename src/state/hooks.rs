use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::adapters::process::StdProcessRunner;
use crate::domain::hook::{HookName, InvalidHookName};
use crate::ports::process::{
    EnvironmentVariable, OutputPolicy, ProcessCommand, ProcessRunner, StdinPolicy,
};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookPhase {
    Pre,
    Post,
}

impl HookPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pre => "pre",
            Self::Post => "post",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookContext {
    pub repo_root: PathBuf,
    pub action: String,
    pub branch: Option<String>,
    pub worktree_path: Option<PathBuf>,
    pub execution_cwd: Option<PathBuf>,
    pub is_tty: bool,
    pub tool: String,
    pub extra_env: BTreeMap<String, String>,
    pub timeout: Duration,
}

impl HookContext {
    pub fn new(repo_root: PathBuf, action: impl Into<String>) -> Self {
        Self {
            repo_root,
            action: action.into(),
            branch: None,
            worktree_path: None,
            execution_cwd: None,
            is_tty: false,
            tool: "vde-worktree".to_owned(),
            extra_env: BTreeMap::new(),
            timeout: DEFAULT_HOOK_TIMEOUT,
        }
    }

    pub fn environment(&self) -> BTreeMap<String, String> {
        let mut environment = self.extra_env.clone();
        environment.extend([
            (
                "WT_REPO_ROOT".to_owned(),
                self.repo_root.to_string_lossy().into_owned(),
            ),
            ("WT_ACTION".to_owned(), self.action.clone()),
            (
                "WT_BRANCH".to_owned(),
                self.branch.clone().unwrap_or_default(),
            ),
            (
                "WT_WORKTREE_PATH".to_owned(),
                self.worktree_path
                    .as_deref()
                    .map_or_else(String::new, |path| path.to_string_lossy().into_owned()),
            ),
            (
                "WT_IS_TTY".to_owned(),
                if self.is_tty { "1" } else { "0" }.to_owned(),
            ),
            ("WT_TOOL".to_owned(), self.tool.clone()),
        ]);
        environment
    }
}

/// Stable hook contexts fixed by mutation preflight before any hook or mutation runs.
///
/// Both phases expose the future target through `WT_BRANCH` and `WT_WORKTREE_PATH`. The pre-hook
/// executes from an existing source worktree (or repository root), while the post-hook executes
/// from the applied target worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationHookContexts {
    pre: HookContext,
    post: HookContext,
}

impl MutationHookContexts {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repo_root: PathBuf,
        action: impl Into<String>,
        target_branch: Option<String>,
        target_worktree_path: Option<PathBuf>,
        pre_execution_cwd: PathBuf,
        post_execution_cwd: PathBuf,
        is_tty: bool,
        extra_env: BTreeMap<String, String>,
    ) -> Self {
        let mut pre = HookContext::new(repo_root, action);
        pre.branch = target_branch;
        pre.worktree_path = target_worktree_path;
        pre.execution_cwd = Some(pre_execution_cwd);
        pre.is_tty = is_tty;
        pre.extra_env = extra_env;
        let mut post = pre.clone();
        post.execution_cwd = Some(post_execution_cwd);
        Self { pre, post }
    }

    pub const fn for_phase(&self, phase: HookPhase) -> &HookContext {
        match phase {
            HookPhase::Pre => &self.pre,
            HookPhase::Post => &self.post,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookProcessRequest {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub environment: BTreeMap<String, String>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookProcessOutput {
    pub signal: Option<i32>,
    pub stderr_truncated: bool,
    pub exit_code: Option<i32>,
    pub stderr: String,
    pub timed_out: bool,
}

pub trait HookProcessRunner {
    fn run(&self, request: &HookProcessRequest) -> Result<HookProcessOutput, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemHookProcessRunner;

impl HookProcessRunner for SystemHookProcessRunner {
    fn run(&self, request: &HookProcessRequest) -> Result<HookProcessOutput, String> {
        let mut command = ProcessCommand::new(request.executable.as_os_str());
        command.args = request.args.iter().map(Into::into).collect();
        command.cwd = Some(request.cwd.clone());
        command.env = request
            .environment
            .iter()
            .map(|(name, value)| EnvironmentVariable::set(name, value))
            .collect();
        command.stdin = StdinPolicy::Null;
        command.stdout = OutputPolicy::Null;
        command.stderr = OutputPolicy::Capture;
        command.timeout = Some(request.timeout);

        let output = StdProcessRunner
            .run(&command)
            .map_err(|error| error.to_string())?;
        Ok(HookProcessOutput {
            exit_code: output.exit_code,
            signal: output.signal,
            stderr_truncated: output.stderr_truncated,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            timed_out: output.timed_out,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookExecution {
    pub signal: Option<i32>,
    pub stderr_truncated: bool,
    pub started_at: String,
    pub ended_at: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stderr: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookOutcome {
    Missing {
        path: PathBuf,
    },
    NonExecutable {
        path: PathBuf,
    },
    TimedOut(HookExecution),
    NonZero(HookExecution),
    SpawnFailure {
        execution: HookExecution,
        message: String,
    },
    Success(HookExecution),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookDisposition {
    Continue,
    Warning,
    Fatal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookRunReport {
    pub hook: String,
    pub phase: HookPhase,
    pub outcome: HookOutcome,
    pub disposition: HookDisposition,
    pub log_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookLogFields {
    pub signal: Option<i32>,
    pub stderr_truncated: bool,
    pub hook: String,
    pub phase: HookPhase,
    pub start: String,
    pub end: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stderr: String,
}

impl HookLogFields {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.hook.trim().is_empty() {
            return Err("hook must be non-empty");
        }
        if self.start.trim().is_empty() {
            return Err("start must be non-empty");
        }
        if self.end.trim().is_empty() {
            return Err("end must be non-empty");
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, &'static str> {
        self.validate()?;
        Ok([
            format!("hook={}", self.hook),
            format!("phase={}", self.phase.as_str()),
            format!("start={}", self.start),
            format!("end={}", self.end),
            format!(
                "exitCode={}",
                self.exit_code
                    .map_or_else(|| "null".to_owned(), |code| code.to_string())
            ),
            format!("timedOut={}", if self.timed_out { "1" } else { "0" }),
            format!(
                "signal={}",
                self.signal
                    .map_or_else(|| "null".to_owned(), |signal| signal.to_string())
            ),
            format!(
                "stderrTruncated={}",
                if self.stderr_truncated { "1" } else { "0" }
            ),
            format!("stderr={}", self.stderr),
            String::new(),
        ]
        .join("\n"))
    }
}

#[derive(Debug, Error)]
pub enum HookError {
    #[error(transparent)]
    InvalidName(#[from] InvalidHookName),
    #[error("hook I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to format hook timestamp: {0}")]
    Timestamp(#[from] time::error::Format),
    #[error("invalid hook log: {0}")]
    InvalidLog(&'static str),
}

pub fn classify_hook_outcome(
    phase: HookPhase,
    strict_post_hooks: bool,
    missing_is_failure: bool,
    outcome: &HookOutcome,
) -> HookDisposition {
    let succeeded = matches!(outcome, HookOutcome::Success(_))
        || (matches!(outcome, HookOutcome::Missing { .. }) && !missing_is_failure);
    if succeeded {
        return HookDisposition::Continue;
    }
    match phase {
        HookPhase::Pre => HookDisposition::Fatal,
        HookPhase::Post if strict_post_hooks => HookDisposition::Fatal,
        HookPhase::Post => HookDisposition::Warning,
    }
}

pub fn run_pre_hook(
    name: &str,
    context: &HookContext,
    runner: &dyn HookProcessRunner,
) -> Result<HookRunReport, HookError> {
    let hook_name = HookName::pre(name)?;
    run_hook(
        HookPhase::Pre,
        &hook_name,
        &[],
        context,
        runner,
        false,
        false,
    )
}

pub fn run_post_hook(
    name: &str,
    context: &HookContext,
    runner: &dyn HookProcessRunner,
    strict_post_hooks: bool,
) -> Result<HookRunReport, HookError> {
    let hook_name = HookName::post(name)?;
    run_hook(
        HookPhase::Post,
        &hook_name,
        &[],
        context,
        runner,
        strict_post_hooks,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_hook(
    phase: HookPhase,
    hook_name: &HookName,
    args: &[String],
    context: &HookContext,
    runner: &dyn HookProcessRunner,
    strict_post_hooks: bool,
    missing_is_failure: bool,
) -> Result<HookRunReport, HookError> {
    let path = hook_path(&context.repo_root, hook_name);
    let start = timestamp()?;
    let outcome = match fs::metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => HookOutcome::Missing { path },
        Err(error) => {
            return Err(HookError::Io {
                path,
                source: error,
            });
        }
        Ok(metadata) if !is_executable(&metadata) => HookOutcome::NonExecutable { path },
        Ok(_) => {
            let request = HookProcessRequest {
                executable: path,
                args: args.to_vec(),
                cwd: context
                    .execution_cwd
                    .clone()
                    .or_else(|| context.worktree_path.clone())
                    .unwrap_or_else(|| context.repo_root.clone()),
                environment: context.environment(),
                timeout: context.timeout,
            };
            match runner.run(&request) {
                Err(message) => HookOutcome::SpawnFailure {
                    execution: HookExecution {
                        started_at: start.clone(),
                        ended_at: timestamp()?,
                        exit_code: None,
                        signal: None,
                        stderr_truncated: false,
                        timed_out: false,
                        stderr: message.clone(),
                    },
                    message,
                },
                Ok(output) => {
                    let execution = HookExecution {
                        started_at: start.clone(),
                        ended_at: timestamp()?,
                        exit_code: output.exit_code,
                        signal: output.signal,
                        stderr_truncated: output.stderr_truncated,
                        timed_out: output.timed_out,
                        stderr: output.stderr,
                    };
                    if execution.timed_out {
                        HookOutcome::TimedOut(execution)
                    } else if execution.exit_code == Some(0) {
                        HookOutcome::Success(execution)
                    } else {
                        HookOutcome::NonZero(execution)
                    }
                }
            }
        }
    };
    let disposition = classify_hook_outcome(phase, strict_post_hooks, missing_is_failure, &outcome);
    let fields = log_fields(hook_name.as_str(), phase, &start, &outcome)?;
    let log_path = append_hook_log(context, &fields)?;
    Ok(HookRunReport {
        hook: hook_name.to_string(),
        phase,
        outcome,
        disposition,
        log_path,
    })
}

fn log_fields(
    hook: &str,
    phase: HookPhase,
    fallback_start: &str,
    outcome: &HookOutcome,
) -> Result<HookLogFields, HookError> {
    let (start, end, exit_code, signal, timed_out, stderr_truncated, stderr) = match outcome {
        HookOutcome::Missing { path } => (
            fallback_start.to_owned(),
            timestamp()?,
            None,
            None,
            false,
            false,
            format!("hook not found: {}", path.display()),
        ),
        HookOutcome::NonExecutable { path } => (
            fallback_start.to_owned(),
            timestamp()?,
            None,
            None,
            false,
            false,
            format!("hook is not executable: {}", path.display()),
        ),
        HookOutcome::TimedOut(execution)
        | HookOutcome::NonZero(execution)
        | HookOutcome::Success(execution)
        | HookOutcome::SpawnFailure { execution, .. } => (
            execution.started_at.clone(),
            execution.ended_at.clone(),
            execution.exit_code,
            execution.signal,
            execution.timed_out,
            execution.stderr_truncated,
            execution.stderr.clone(),
        ),
    };
    Ok(HookLogFields {
        hook: hook.to_owned(),
        phase,
        start,
        end,
        exit_code,
        signal,
        timed_out,
        stderr_truncated,
        stderr,
    })
}

fn append_hook_log(context: &HookContext, fields: &HookLogFields) -> Result<PathBuf, HookError> {
    let directory = context.repo_root.join(".vde/worktree/logs");
    fs::create_dir_all(&directory).map_err(|source| HookError::Io {
        path: directory.clone(),
        source,
    })?;
    let branch = context.branch.as_deref().unwrap_or("none");
    let file_name = format!(
        "{}_{}_{}.log",
        file_timestamp(),
        safe_file_component(&context.action),
        safe_file_component(branch)
    );
    let path = directory.join(file_name);
    let log_text = fields.render().map_err(HookError::InvalidLog)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| HookError::Io {
            path: path.clone(),
            source,
        })?;
    file.write_all(log_text.as_bytes())
        .and_then(|()| file.flush())
        .map_err(|source| HookError::Io {
            path: path.clone(),
            source,
        })?;
    Ok(path)
}

fn hook_path(repo_root: &Path, hook_name: &HookName) -> PathBuf {
    repo_root
        .join(".vde/worktree/hooks")
        .join(hook_name.as_str())
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(metadata: &fs::Metadata) -> bool {
    metadata.is_file()
}

fn safe_file_component(value: &str) -> String {
    let safe: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        "none".to_owned()
    } else {
        safe
    }
}

fn timestamp() -> Result<String, time::error::Format> {
    OffsetDateTime::now_utc().format(&Rfc3339)
}

fn file_timestamp() -> i128 {
    OffsetDateTime::now_utc().unix_timestamp_nanos()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Clone)]
    struct FakeRunner {
        output: Result<HookProcessOutput, String>,
        requests: std::sync::Arc<Mutex<Vec<HookProcessRequest>>>,
    }

    impl HookProcessRunner for FakeRunner {
        fn run(&self, request: &HookProcessRequest) -> Result<HookProcessOutput, String> {
            self.requests.lock().unwrap().push(request.clone());
            self.output.clone()
        }
    }

    fn executable_hook(repo_root: &Path, name: &str) {
        let hook_name = name.parse::<HookName>().unwrap();
        let path = hook_path(repo_root, &hook_name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn maps_timeout_and_applies_phase_policy_separately() {
        let directory = tempfile::tempdir().unwrap();
        executable_hook(directory.path(), "post-switch");
        let requests = std::sync::Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner {
            output: Ok(HookProcessOutput {
                exit_code: None,
                signal: Some(9),
                stderr_truncated: true,
                stderr: "slow".to_owned(),
                timed_out: true,
            }),
            requests: requests.clone(),
        };
        let mut context = HookContext::new(directory.path().to_path_buf(), "switch");
        context.branch = Some("feature/a".to_owned());
        context.is_tty = true;
        context
            .extra_env
            .insert("EXTRA".to_owned(), "ok".to_owned());

        let warning = run_post_hook("switch", &context, &runner, false).unwrap();
        assert!(matches!(warning.outcome, HookOutcome::TimedOut(_)));
        assert_eq!(warning.disposition, HookDisposition::Warning);
        let fatal = run_post_hook("switch", &context, &runner, true).unwrap();
        assert_eq!(fatal.disposition, HookDisposition::Fatal);

        let request = &requests.lock().unwrap()[0];
        assert_eq!(request.environment["WT_BRANCH"], "feature/a");
        assert_eq!(request.environment["WT_IS_TTY"], "1");
        assert_eq!(request.environment["WT_TOOL"], "vde-worktree");
        assert_eq!(request.environment["EXTRA"], "ok");
        let log = fs::read_to_string(warning.log_path).unwrap();
        for field in [
            "hook=post-switch",
            "phase=post",
            "start=",
            "end=",
            "exitCode=null",
            "signal=9",
            "stderrTruncated=1",
            "timedOut=1",
            "stderr=slow",
        ] {
            assert!(log.contains(field), "missing log field: {field}");
        }
    }

    #[test]
    fn mutation_context_exposes_the_future_target_but_uses_phase_specific_cwds() {
        let directory = tempfile::tempdir().unwrap();
        executable_hook(directory.path(), "pre-new");
        executable_hook(directory.path(), "post-new");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        let contexts = MutationHookContexts::new(
            directory.path().to_path_buf(),
            "new",
            Some("feature/future".to_owned()),
            Some(target.clone()),
            source.clone(),
            target.clone(),
            true,
            BTreeMap::from([
                ("WT_REMOTE".to_owned(), "origin".to_owned()),
                ("WT_BRANCH".to_owned(), "must-not-override".to_owned()),
            ]),
        );
        let requests = std::sync::Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner {
            output: Ok(HookProcessOutput {
                exit_code: Some(0),
                signal: None,
                stderr_truncated: false,
                stderr: String::new(),
                timed_out: false,
            }),
            requests: requests.clone(),
        };

        run_pre_hook("new", contexts.for_phase(HookPhase::Pre), &runner).unwrap();
        run_post_hook("new", contexts.for_phase(HookPhase::Post), &runner, false).unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests[0].cwd, source);
        assert_eq!(requests[1].cwd, target);
        for request in requests.iter() {
            assert_eq!(request.environment["WT_BRANCH"], "feature/future");
            assert_eq!(
                request.environment["WT_WORKTREE_PATH"],
                directory.path().join("target").to_string_lossy()
            );
            assert_eq!(request.environment["WT_IS_TTY"], "1");
            assert_eq!(request.environment["WT_REMOTE"], "origin");
        }
    }

    #[test]
    fn every_pre_failure_is_fatal_and_optional_missing_is_successful() {
        let directory = tempfile::tempdir().unwrap();
        let runner = FakeRunner {
            output: Err("spawn failed".to_owned()),
            requests: std::sync::Arc::new(Mutex::new(Vec::new())),
        };
        let context = HookContext::new(directory.path().to_path_buf(), "switch");
        let missing = run_pre_hook("switch", &context, &runner).unwrap();
        assert_eq!(missing.disposition, HookDisposition::Continue);

        executable_hook(directory.path(), "pre-switch");
        let failed = run_pre_hook("switch", &context, &runner).unwrap();
        assert!(matches!(failed.outcome, HookOutcome::SpawnFailure { .. }));
        assert_eq!(failed.disposition, HookDisposition::Fatal);
    }

    #[test]
    fn hook_engine_rejects_invalid_action_names_before_path_resolution() {
        let directory = tempfile::tempdir().unwrap();
        let runner = FakeRunner {
            output: Err("must not run".to_owned()),
            requests: std::sync::Arc::new(Mutex::new(Vec::new())),
        };
        let context = HookContext::new(directory.path().to_path_buf(), "invoke");

        let error = run_pre_hook("../escape", &context, &runner)
            .expect_err("directory escape must be rejected");
        assert!(matches!(error, HookError::InvalidName(_)));
    }
}
