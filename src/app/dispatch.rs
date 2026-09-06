use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::adapters::fzf::FzfAdapter;
use crate::adapters::gh_cli::GhCli;
use crate::adapters::git_cli::GitCli;
use crate::adapters::process::StdProcessRunner;
use crate::app::completion::execute_completion;
use crate::app::error_mapper::map_metadata_transaction_error;
use crate::app::error_mapper::{MapToCliError, map_hook_report};
use crate::app::misc_commands::{MiscCommandOutput, execute_misc_command};
use crate::app::mutations_change::{
    ExtractGitApplied, ExtractPlan, ExtractResult, LockPlan, MvGitApplied, MvPlan, MvResult,
    StagedExtractPlan, StagedMvPlan, UnlockPlan, UseInvocation, UseOptions, UsePlan, UseResult,
    UseSharing, apply_extract_git, apply_lock, apply_mv_git, apply_unlock, apply_use,
    finalize_extract_state, finalize_mv_state, prepare_extract, prepare_lock, prepare_mv,
    prepare_unlock, prepare_use, restore_extract_after_pre_hook_failure,
    rollback_mv_after_pre_hook_failure, stage_extract_for_hook, stage_mv_for_hook,
};
use crate::app::mutations_create::{
    AdoptPlan, AdoptResult, GetPlan, InitPlan, InitResult, NewPlan, SwitchPlan, WorktreeGitApplied,
    WorktreeMutationResult, apply_adopt, apply_get_git, apply_init, apply_new_git,
    apply_switch_git, finalize_worktree_state, prepare_adopt, prepare_get, prepare_init,
    prepare_new, prepare_switch, resolve_managed_worktree_root,
};
use crate::app::mutations_delete::{
    DelGitApplied, DelPlan, DelResult, DeleteForceOptions, DeleteInvocation,
    FilesystemDeleteMutationState, GoneGitApplied, GonePlan, GoneResult, apply_del_git,
    apply_gone_git, finalize_del_state, finalize_gone_state, prepare_del, prepare_gone,
    revalidate_del,
};
use crate::app::read_commands::{ReadCommandRuntime, execute_read_command};
use crate::app::result::{ProcessOutput, TerminalCapabilities};
use crate::app::snapshot::{SnapshotCollector, resolve_base_branch};
use crate::app::transfer::{
    StagedTransfer, StashRetention, TransferDirection, TransferInvocation, TransferOptions,
    TransferPlan, TransferResult, apply_transfer, prepare_absorb, prepare_unabsorb,
    rollback_transfer_after_pre_hook_failure, stage_transfer,
};
use crate::cli::{Command, ParsedRequest};
use crate::domain::error::{CliError, ErrorCode, ExecutionPhase, ExecutionState};
use crate::domain::repo::RepoContext;
use crate::domain::worktree::WorktreeSnapshot;
use crate::presentation::json::{
    ErrorEnvelope, ErrorPayload, PartialErrorEnvelope, SuccessEnvelope, to_stdout_json,
};
use crate::state::config::{ResolvedConfig, load_resolved_config};
use crate::state::hooks::{
    HookContext, HookDisposition, HookPhase, MutationHookContexts, SystemHookProcessRunner,
    run_post_hook, run_pre_hook,
};
use crate::state::metadata_transaction::recover_pending_metadata_transactions;
use crate::state::repo_lock::{RepoLock, acquire_repo_lock};

#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    pub data: Value,
    pub human_stdout: String,
    pub human_stderr: String,
    pub partial_error: Option<CliError>,
    pub warnings: Vec<CliError>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplicationHookResult {
    Continue,
    Warning(CliError),
}

/// A command-specific mutation plan whose hook target and phase-specific cwd were fixed during
/// preflight.
pub trait PlannedMutation {
    fn hook_context(&self, phase: HookPhase) -> &HookContext;

    fn requires_hooks(&self) -> bool {
        true
    }
}

impl PlannedMutation for MutationHookContexts {
    fn hook_context(&self, phase: HookPhase) -> &HookContext {
        self.for_phase(phase)
    }
}

impl CommandOutput {
    pub fn new(data: Value) -> Self {
        Self {
            data,
            human_stdout: String::new(),
            human_stderr: String::new(),
            partial_error: None,
            warnings: Vec::new(),
        }
    }

    pub fn partial(data: Value, error: CliError) -> Self {
        Self {
            data,
            human_stdout: String::new(),
            human_stderr: String::new(),
            partial_error: Some(error.at_phase(
                ExecutionPhase::Finalize,
                ExecutionState::Partial,
                &[],
            )),
            warnings: Vec::new(),
        }
    }
}

impl From<MiscCommandOutput> for CommandOutput {
    fn from(output: MiscCommandOutput) -> Self {
        Self {
            data: output.data,
            human_stdout: output.human_stdout,
            human_stderr: output.human_stderr,
            partial_error: output.partial_error,
            warnings: Vec::new(),
        }
    }
}

pub trait ApplicationBackend {
    type LockGuard;
    type MutationPlan: PlannedMutation;
    type MutationStage;
    type MutationResult;

    fn resolve_repo_context(&self) -> Result<RepoContext, CliError>;

    fn resolve_config(&self, context: &RepoContext) -> Result<ResolvedConfig, CliError>;

    fn is_initialized(&self, repo_root: &Path) -> Result<bool, CliError>;

    fn acquire_repo_lock(
        &self,
        context: &RepoContext,
        timeout: Duration,
        command: &str,
    ) -> Result<Self::LockGuard, CliError>;

    fn run_hook(
        &self,
        phase: HookPhase,
        request: &ParsedRequest,
        context: &HookContext,
        timeout: Duration,
    ) -> Result<ApplicationHookResult, CliError>;

    /// Performs every command-specific safety check and fixes the complete mutation plan.
    ///
    /// Implementations must not mutate Git, persistent state, or the worktree filesystem here.
    fn prepare_mutation(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
    ) -> Result<Self::MutationPlan, CliError>;

    /// Inspect only; never lock, recover, stage, invoke hooks or finalize state.
    fn inspect_mutation(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
    ) -> Result<CommandOutput, CliError> {
        if !matches!(request.command, Command::Init) && !self.is_initialized(&context.repo_root)? {
            return Err(CliError::new(
                ErrorCode::NotInitialized,
                "Repository is not initialized. Run `vw init` first.",
            ));
        }
        let plan = self.prepare_mutation(request, context)?;
        let target = plan.hook_context(HookPhase::Post);
        crate::app::preflight::output(
            request,
            &json!({"branch": target.branch, "path": target.worktree_path}),
            &Value::Null,
            &Value::Null,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Called only under the mutation lock, before planning an actual operation.
    fn recover_mutations(&self, _context: &RepoContext) -> Result<Vec<CliError>, CliError> {
        Ok(Vec::new())
    }

    /// Performs reversible staging required before a pre-hook, such as stashing dirty state.
    fn stage_mutation(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
        plan: &Self::MutationPlan,
    ) -> Result<Self::MutationStage, CliError>;

    /// Restores reversible staging when a pre-hook fails.
    fn rollback_mutation_stage(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
        plan: &Self::MutationPlan,
        stage: &Self::MutationStage,
    ) -> Result<(), CliError>;

    /// Applies the command's Git/filesystem mutation without updating persistent application state.
    fn apply_mutation(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
        plan: &Self::MutationPlan,
        stage: Self::MutationStage,
    ) -> Result<Self::MutationResult, CliError>;

    /// Persists state after a successful apply and builds the public command result.
    fn update_mutation_state(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
        plan: &Self::MutationPlan,
        result: Self::MutationResult,
    ) -> Result<CommandOutput, CliError>;

    fn execute(
        &self,
        request: &ParsedRequest,
        context: Option<&RepoContext>,
    ) -> Result<CommandOutput, CliError>;
}

pub fn dispatch<B>(request: &ParsedRequest, backend: &B) -> ProcessOutput
where
    B: ApplicationBackend,
{
    let context = if matches!(
        request.command,
        Command::Completion { .. } | Command::Describe { .. } | Command::Doctor
    ) {
        None
    } else {
        match backend.resolve_repo_context() {
            Ok(context) => Some(context),
            Err(error) => {
                return render_error(
                    request,
                    None,
                    &error.at_phase(ExecutionPhase::Resolve, ExecutionState::NotStarted, &[]),
                );
            }
        }
    };
    let config = match context.as_ref() {
        Some(context) => match backend.resolve_config(context) {
            Ok(config) => Some(config),
            Err(error) => {
                return render_error(
                    request,
                    Some(context),
                    &error.at_phase(ExecutionPhase::Configure, ExecutionState::NotStarted, &[]),
                );
            }
        },
        None => None,
    };

    let write_command = is_write_command(&request.command);
    let lock_guard = match mutation_lock(request, backend, context.as_ref(), config.as_ref()) {
        Ok(guard) => guard,
        Err(error) => return render_error(request, context.as_ref(), &error),
    };

    let mut hook_warnings = Vec::new();
    let result = if write_command && request.is_preview() {
        backend.inspect_mutation(request, context.as_ref().expect("mutation context"))
    } else if write_command {
        let context = context
            .as_ref()
            .expect("write commands always resolve a context");
        let config = config
            .as_ref()
            .expect("write commands always resolve config");
        let hook_timeout = Duration::from_millis(
            request
                .common
                .hook_timeout_ms
                .unwrap_or(config.hooks.timeout_ms),
        );
        let hooks_enabled = request.common.hooks_enabled(config.hooks.enabled);
        execute_mutation_pipeline(
            request,
            backend,
            context,
            hooks_enabled,
            hook_timeout,
            &mut hook_warnings,
        )
    } else {
        backend.execute(request, context.as_ref())
    };
    drop(lock_guard);

    let mut rendered = match result {
        Ok(mut output) => {
            for warning in &hook_warnings {
                let _ = writeln!(output.human_stderr, "Warning: {}", warning.message);
            }
            output.warnings.extend(hook_warnings);
            render_output(request, context.as_ref(), output)
        }
        Err(error) => render_error_with_warnings(request, context.as_ref(), &error, &hook_warnings),
    };
    append_verbose(request, context.as_ref(), config.as_ref(), &mut rendered);
    rendered
}

fn append_verbose(
    request: &ParsedRequest,
    context: Option<&RepoContext>,
    config: Option<&ResolvedConfig>,
    rendered: &mut ProcessOutput,
) {
    if request.common.verbose > 0 {
        let _ = writeln!(
            rendered.stderr,
            "[verbose] command={} repo={} worktree={} exit={}",
            request.command.name(),
            context.map_or_else(
                || "(none)".to_owned(),
                |context| context.repo_root.display().to_string()
            ),
            context.map_or_else(
                || "(none)".to_owned(),
                |context| context.current_worktree_root.display().to_string()
            ),
            rendered.exit_code
        );
        if let Some(config) = config {
            let mut config = config.clone();
            crate::app::diagnostics::apply_cli_values(&mut config, &request.common);
            let _ = writeln!(
                rendered.stderr,
                "[verbose] hooks={} github={} hookTimeoutMs={} lockTimeoutMs={}",
                config.hooks.enabled,
                config.github.enabled,
                config.hooks.timeout_ms,
                config.locks.timeout_ms
            );
            if request.common.verbose > 1 {
                let _ = writeln!(rendered.stderr, "[verbose] config={}", json!(config));
            }
        }
    }
}

fn mutation_lock<B: ApplicationBackend>(
    request: &ParsedRequest,
    backend: &B,
    context: Option<&RepoContext>,
    config: Option<&ResolvedConfig>,
) -> Result<Option<B::LockGuard>, CliError> {
    if !is_write_command(&request.command) || request.is_preview() {
        return Ok(None);
    }
    let context = context.expect("write commands resolve a context");
    let config = config.expect("write commands resolve config");
    if !matches!(request.command, Command::Init) {
        let initialized = backend
            .is_initialized(&context.repo_root)
            .map_err(|error| {
                error.at_phase(ExecutionPhase::Preflight, ExecutionState::NotStarted, &[])
            })?;
        if !initialized {
            return Err(CliError::new(
                ErrorCode::NotInitialized,
                "Repository is not initialized. Run `vde-worktree init` first.",
            )
            .with_details(BTreeMap::from([(
                "repoRoot".to_owned(),
                json!(context.repo_root),
            )]))
            .at_phase(ExecutionPhase::Preflight, ExecutionState::NotStarted, &[]));
        }
    }
    let timeout = Duration::from_millis(
        request
            .common
            .lock_timeout_ms
            .unwrap_or(config.locks.timeout_ms),
    );
    backend
        .acquire_repo_lock(context, timeout, request.command.name())
        .map(Some)
        .map_err(|error| error.at_phase(ExecutionPhase::Lock, ExecutionState::NotStarted, &[]))
}

fn execute_mutation_pipeline<B>(
    request: &ParsedRequest,
    backend: &B,
    context: &RepoContext,
    hooks_enabled: bool,
    hook_timeout: Duration,
    hook_warnings: &mut Vec<CliError>,
) -> Result<CommandOutput, CliError>
where
    B: ApplicationBackend,
{
    hook_warnings.extend(backend.recover_mutations(context)?);
    let plan = backend
        .prepare_mutation(request, context)
        .map_err(|error| {
            error.at_phase(ExecutionPhase::Preflight, ExecutionState::NotStarted, &[])
        })?;
    let stage = backend
        .stage_mutation(request, context, &plan)
        .map_err(|error| {
            error.at_phase(
                ExecutionPhase::Stage,
                ExecutionState::Unknown,
                &["preflight"],
            )
        })?;
    if hooks_enabled && uses_command_hooks(&request.command) && plan.requires_hooks() {
        match backend.run_hook(
            HookPhase::Pre,
            request,
            plan.hook_context(HookPhase::Pre),
            hook_timeout,
        ) {
            Ok(result) => collect_hook_result(result, hook_warnings),
            Err(hook_error) => {
                return match backend.rollback_mutation_stage(request, context, &plan, &stage) {
                    Ok(()) => Err(hook_error.at_phase(
                        ExecutionPhase::PreHook,
                        ExecutionState::RolledBack,
                        &["preflight", "stage", "rollbackStage"],
                    )),
                    Err(restore_error) => {
                        Err(hook_error_with_restore_failure(hook_error, &restore_error))
                    }
                };
            }
        }
    }
    let applied = backend
        .apply_mutation(request, context, &plan, stage)
        .map_err(|error| {
            error.at_phase(
                ExecutionPhase::Apply,
                ExecutionState::Unknown,
                &["preflight", "stage"],
            )
        })?;
    let mut output = backend
        .update_mutation_state(request, context, &plan, applied)
        .map_err(|error| {
            error.at_phase(
                ExecutionPhase::Finalize,
                ExecutionState::Partial,
                &["preflight", "stage", "apply"],
            )
        })?;
    if hooks_enabled && uses_command_hooks(&request.command) && plan.requires_hooks() {
        match backend.run_hook(
            HookPhase::Post,
            request,
            plan.hook_context(HookPhase::Post),
            hook_timeout,
        ) {
            Ok(result) => collect_hook_result(result, hook_warnings),
            Err(post_hook_error) if output.partial_error.is_some() => {
                let post_hook_error = post_hook_error.at_phase(
                    ExecutionPhase::PostHook,
                    ExecutionState::Partial,
                    &[],
                );
                let partial_error = output
                    .partial_error
                    .as_mut()
                    .expect("partial output was checked");
                partial_error.details.insert(
                    "postHookError".to_owned(),
                    json!({
                        "code": post_hook_error.code,
                        "message": post_hook_error.message,
                        "details": post_hook_error.details,
                        "execution": post_hook_error.execution,
                    }),
                );
            }
            Err(post_hook_error) => {
                output.partial_error = Some(post_hook_error.at_phase(
                    ExecutionPhase::PostHook,
                    ExecutionState::Applied,
                    &["preflight", "stage", "apply", "finalize"],
                ));
            }
        }
    }
    Ok(output)
}

fn hook_error_with_restore_failure(mut hook_error: CliError, restore_error: &CliError) -> CliError {
    hook_error
        .details
        .insert("autoRestoreFailed".to_owned(), json!(true));
    hook_error.details.insert(
        "autoRestoreError".to_owned(),
        json!({
            "code": restore_error.code,
            "message": restore_error.message,
            "details": restore_error.details,
        }),
    );
    hook_error.at_phase(ExecutionPhase::PreHook, ExecutionState::RecoveryRequired, &["preflight", "stage"])
        .with_recovery("rollbackError", json!({"code": restore_error.code, "details": restore_error.details, "execution": restore_error.execution}))
}

fn collect_hook_result(result: ApplicationHookResult, warnings: &mut Vec<CliError>) {
    if let ApplicationHookResult::Warning(error) = result {
        warnings.push(error);
    }
}

pub const fn is_write_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Init
            | Command::New { .. }
            | Command::Switch { .. }
            | Command::Mv { .. }
            | Command::Del { .. }
            | Command::Gone { .. }
            | Command::Adopt { .. }
            | Command::Get { .. }
            | Command::Extract { .. }
            | Command::Absorb { .. }
            | Command::Unabsorb { .. }
            | Command::Use { .. }
            | Command::Lock { .. }
            | Command::Unlock { .. }
    )
}

const fn uses_command_hooks(command: &Command) -> bool {
    is_write_command(command) && !matches!(command, Command::Lock { .. } | Command::Unlock { .. })
}

fn render_output(
    request: &ParsedRequest,
    context: Option<&RepoContext>,
    output: CommandOutput,
) -> ProcessOutput {
    if let Some(error) = &output.partial_error {
        if request.common.json {
            let mut envelope = PartialErrorEnvelope::new(
                request.output_command(),
                context.map(|value| value.repo_root.to_string_lossy().into_owned()),
                output.data,
                ErrorPayload::from(error),
            );
            envelope.warnings = output.warnings.iter().map(ErrorPayload::from).collect();
            return match to_stdout_json(&envelope) {
                Ok(stdout) => ProcessOutput {
                    exit_code: error.exit_code(),
                    stdout,
                    stderr: output.human_stderr,
                },
                Err(serialization_error) => {
                    ProcessOutput::stderr(30, format!("{serialization_error}\n"))
                }
            };
        }
        return ProcessOutput {
            exit_code: error.exit_code(),
            stdout: output.human_stdout,
            stderr: format!(
                "{}[{}] {}\n",
                output.human_stderr, error.code, error.message
            ),
        };
    }
    if request.common.json {
        let mut envelope = SuccessEnvelope::new(
            request.output_command(),
            context.map(|value| value.repo_root.to_string_lossy().into_owned()),
            output.data,
        );
        envelope.warnings = output.warnings.iter().map(ErrorPayload::from).collect();
        return match to_stdout_json(&envelope) {
            Ok(stdout) => ProcessOutput {
                exit_code: 0,
                stdout,
                stderr: output.human_stderr,
            },
            Err(error) => ProcessOutput::stderr(30, format!("{error}\n")),
        };
    }
    ProcessOutput {
        exit_code: 0,
        stdout: output.human_stdout,
        stderr: output.human_stderr,
    }
}

fn render_error(
    request: &ParsedRequest,
    context: Option<&RepoContext>,
    error: &CliError,
) -> ProcessOutput {
    render_error_with_warnings(request, context, error, &[])
}

fn render_error_with_warnings(
    request: &ParsedRequest,
    context: Option<&RepoContext>,
    error: &CliError,
    warnings: &[CliError],
) -> ProcessOutput {
    if request.common.json {
        let mut envelope = ErrorEnvelope::new(
            request.output_command(),
            context.map(|value| value.repo_root.to_string_lossy().into_owned()),
            ErrorPayload::from(error),
        );
        envelope.warnings = warnings.iter().map(ErrorPayload::from).collect();
        return match to_stdout_json(&envelope) {
            Ok(stdout) => ProcessOutput::stdout(error.exit_code(), stdout),
            Err(serialization_error) => {
                ProcessOutput::stderr(30, format!("{serialization_error}\n"))
            }
        };
    }
    if error.code == ErrorCode::Cancelled {
        return ProcessOutput::stdout(error.exit_code(), "");
    }
    let mut stderr = String::new();
    for warning in warnings {
        let _ = writeln!(stderr, "Warning: {}", warning.message);
    }
    let _ = writeln!(stderr, "[{}] {}", error.code, error.message);
    ProcessOutput::stderr(error.exit_code(), stderr)
}

#[derive(Debug)]
enum SystemMutationCommandPlan {
    Init(InitPlan),
    New(NewPlan),
    Switch(SwitchPlan),
    Get(GetPlan),
    Adopt(AdoptPlan),
    Mv(MvPlan),
    Extract(ExtractPlan),
    Use(UsePlan),
    Lock(LockPlan),
    Unlock(UnlockPlan),
    Del(Box<DelPlan>),
    Gone(GonePlan),
    Transfer(TransferPlan),
}

#[derive(Debug)]
pub struct SystemMutationPlan {
    command: SystemMutationCommandPlan,
    inspection_evidence: Value,
    hooks: MutationHookContexts,
    requires_hooks: bool,
}

impl PlannedMutation for SystemMutationPlan {
    fn hook_context(&self, phase: HookPhase) -> &HookContext {
        self.hooks.for_phase(phase)
    }

    fn requires_hooks(&self) -> bool {
        self.requires_hooks
    }
}

#[derive(Debug)]
pub enum SystemMutationStage {
    None,
    Mv(StagedMvPlan),
    Extract(StagedExtractPlan),
    Transfer(StagedTransfer),
}

#[derive(Debug)]
pub enum SystemMutationResult {
    Init(InitResult),
    WorktreeGitApplied(Box<WorktreeGitApplied>),
    Worktree(WorktreeMutationResult),
    Adopt(AdoptResult),
    MvGitApplied(Box<MvGitApplied>),
    Mv(MvResult),
    ExtractGitApplied(Box<ExtractGitApplied>),
    Extract(ExtractResult),
    Use(UseResult),
    Lock(crate::state::worktree_lock::WorktreeLockRecord),
    Unlock { branch: String },
    DelGitApplied(Box<DelGitApplied>),
    Del(DelResult),
    GoneGitApplied(Box<GoneGitApplied>),
    Gone(GoneResult),
    Transfer(TransferResult),
}

#[derive(Debug)]
pub struct SystemBackend {
    cwd: PathBuf,
    git: GitCli<StdProcessRunner>,
    gh: GhCli<StdProcessRunner>,
    fzf: FzfAdapter<StdProcessRunner>,
    terminal: TerminalCapabilities,
    home: Option<PathBuf>,
    in_tmux: bool,
}

impl SystemBackend {
    pub fn from_environment(directory: Option<&Path>) -> Result<Self, CliError> {
        let cwd = env::current_dir().map_err(|error| {
            CliError::new(
                ErrorCode::InternalError,
                format!("failed to resolve current directory: {error}"),
            )
        })?;
        let cwd = directory.map_or_else(|| cwd.clone(), |path| cwd.join(path));
        let cwd = cwd.canonicalize().map_err(|error| {
            CliError::new(
                ErrorCode::InvalidArgument,
                format!(
                    "cannot resolve execution directory {}: {error}",
                    cwd.display()
                ),
            )
        })?;
        if !cwd.is_dir() {
            return Err(CliError::new(
                ErrorCode::InvalidArgument,
                "execution directory must be a directory",
            ));
        }
        Ok(Self {
            cwd,
            git: GitCli::new(StdProcessRunner),
            gh: GhCli::new(StdProcessRunner),
            fzf: FzfAdapter::new(StdProcessRunner),
            terminal: TerminalCapabilities::from_environment(),
            home: env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            in_tmux: env::var_os("TMUX").is_some_and(|value| !value.is_empty()),
        })
    }

    pub fn resolve_request_paths(&self, request: &mut ParsedRequest) {
        request.common.directory = Some(self.cwd.clone());
        if let Some(path) = &mut request.common.worktree {
            *path = self.cwd.join(&*path);
        }
        if let Command::Completion {
            path: Some(path), ..
        } = &mut request.command
        {
            *path = self.cwd.join(&*path);
        }
    }

    fn mutation_config(&self, context: &RepoContext) -> Result<ResolvedConfig, CliError> {
        load_resolved_config(&self.cwd, &context.repo_root)
            .map(|loaded| loaded.config)
            .map_err(MapToCliError::map_to_cli_error)
    }

    fn mutation_snapshot(
        &self,
        context: &RepoContext,
        config: &ResolvedConfig,
        gh_enabled: bool,
    ) -> Result<WorktreeSnapshot, CliError> {
        let base_branch = resolve_base_branch(
            &self.git,
            &context.repo_root,
            config.git.base_branch.as_deref(),
            &config.git.base_remote,
        )
        .map_err(MapToCliError::map_to_cli_error)?;
        SnapshotCollector::new(&self.git, &self.gh)
            .without_lifecycle_observations()
            .collect(&context.repo_root, &base_branch, gh_enabled)
            .map_err(MapToCliError::map_to_cli_error)
    }
    fn selected_mutation_snapshot(
        &self,
        context: &RepoContext,
        config: &ResolvedConfig,
        branch: Option<&str>,
        gh_enabled: bool,
    ) -> Result<WorktreeSnapshot, CliError> {
        let registry = crate::app::snapshot::read_registry(&self.git, &context.repo_root)
            .map_err(MapToCliError::map_to_cli_error)?;
        let target =
            crate::app::target::resolve(&registry, branch, None, &context.current_worktree_root)?;
        if let Some(branch) = &target.branch {
            crate::app::target::optional_branch(&registry, branch)?;
        }
        let base = resolve_base_branch(
            &self.git,
            &context.repo_root,
            config.git.base_branch.as_deref(),
            &config.git.base_remote,
        )
        .map_err(MapToCliError::map_to_cli_error)?;
        SnapshotCollector::new(&self.git, &self.gh)
            .without_lifecycle_observations()
            .collect_registry(
                &context.repo_root,
                &base,
                gh_enabled,
                std::slice::from_ref(target),
            )
            .map_err(MapToCliError::map_to_cli_error)
    }
}

#[allow(clippy::too_many_arguments)]
fn mutation_hooks(
    terminal: TerminalCapabilities,
    repo_root: &Path,
    action: &str,
    branch: Option<String>,
    target_path: PathBuf,
    pre_cwd: PathBuf,
    post_cwd: PathBuf,
    extra_env: BTreeMap<String, String>,
) -> MutationHookContexts {
    MutationHookContexts::new(
        repo_root.to_path_buf(),
        action,
        branch,
        Some(target_path),
        pre_cwd,
        post_cwd,
        terminal.stdout_tty && terminal.stderr_tty,
        extra_env,
    )
}

fn transfer_invocation(
    terminal: TerminalCapabilities,
    allow_agent: bool,
    allow_unsafe: bool,
) -> TransferInvocation {
    if terminal.stdout_tty && terminal.stderr_tty {
        TransferInvocation::Interactive
    } else {
        TransferInvocation::NonInteractive {
            allow_agent,
            allow_unsafe,
        }
    }
}

fn transfer_hooks(
    terminal: TerminalCapabilities,
    repo_root: &Path,
    action: &str,
    plan: &TransferPlan,
) -> MutationHookContexts {
    mutation_hooks(
        terminal,
        repo_root,
        action,
        Some(plan.branch.clone()),
        plan.target_path.clone(),
        plan.source_path.clone(),
        plan.target_path.clone(),
        BTreeMap::from([
            (
                "WT_SOURCE".to_owned(),
                plan.source_path.to_string_lossy().into_owned(),
            ),
            (
                "WT_TARGET".to_owned(),
                plan.target_path.to_string_lossy().into_owned(),
            ),
        ]),
    )
}

fn generated_wip_branch() -> String {
    let sequence = time::OffsetDateTime::now_utc()
        .unix_timestamp_nanos()
        .unsigned_abs()
        % 1_000_000;
    format!("wip-{sequence:06}")
}

fn default_lock_owner() -> Result<String, CliError> {
    env::var("USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("USERNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .ok_or_else(|| {
            CliError::new(
                ErrorCode::InvalidArgument,
                "lock owner is unavailable; pass --owner explicitly",
            )
        })
}

fn local_host_name() -> Result<String, CliError> {
    #[cfg(unix)]
    {
        let value = nix::unistd::gethostname()
            .map_err(|error| {
                CliError::new(
                    ErrorCode::InternalError,
                    format!("failed to resolve host name: {error}"),
                )
            })?
            .to_string_lossy()
            .into_owned();
        if value.trim().is_empty() {
            return Err(CliError::new(
                ErrorCode::InternalError,
                "resolved host name is empty",
            ));
        }
        Ok(value)
    }
    #[cfg(not(unix))]
    {
        env::var("COMPUTERNAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                CliError::new(
                    ErrorCode::InternalError,
                    "host name is unavailable in COMPUTERNAME",
                )
            })
    }
}

#[allow(clippy::too_many_lines)]
fn mutation_command_output(result: SystemMutationResult) -> Result<CommandOutput, CliError> {
    match result {
        SystemMutationResult::Init(result) => Ok(CommandOutput::new(json!({
            "alreadyInitialized": result.already_initialized,
        }))),
        SystemMutationResult::WorktreeGitApplied(_)
        | SystemMutationResult::MvGitApplied(_)
        | SystemMutationResult::ExtractGitApplied(_)
        | SystemMutationResult::GoneGitApplied(_) => Err(CliError::new(
            ErrorCode::InternalError,
            "mutation metadata finalization was skipped",
        )),
        SystemMutationResult::Worktree(result) => {
            let human_stdout = format!("{}\n", result.path.display());
            let data = serde_json::to_value(result).map_err(|error| serialization_error(&error))?;
            let mut output = CommandOutput::new(data);
            output.human_stdout = human_stdout;
            Ok(output)
        }
        SystemMutationResult::Adopt(result) => {
            let mut human_stdout = String::new();
            if result.dry_run {
                for candidate in &result.candidates {
                    let _ = writeln!(
                        human_stdout,
                        "candidate: {} -> {}",
                        candidate.from_path.display(),
                        candidate.to_path.display()
                    );
                }
            } else {
                for moved in &result.moved {
                    let _ = writeln!(
                        human_stdout,
                        "moved: {} -> {}",
                        moved.from_path.display(),
                        moved.to_path.display()
                    );
                }
                for failed in &result.failed {
                    let _ = writeln!(
                        human_stdout,
                        "failed: {} -> {} [{}] {}",
                        failed.from_path.display(),
                        failed.to_path.display(),
                        failed.code,
                        failed.message
                    );
                }
            }
            let has_failures = result.has_failures();
            let data = serde_json::to_value(result).map_err(|error| serialization_error(&error))?;
            let mut output = if has_failures {
                CommandOutput::partial(
                    data,
                    CliError::new(
                        ErrorCode::SafetyRejected,
                        "one or more worktrees could not be adopted",
                    ),
                )
            } else {
                CommandOutput::new(data)
            };
            output.human_stdout = human_stdout;
            Ok(output)
        }
        SystemMutationResult::Mv(result) => Ok(path_result(&result.branch, &result.path)),
        SystemMutationResult::Extract(result) => Ok(path_result(&result.branch, &result.path)),
        SystemMutationResult::Use(result) => Ok(path_result(&result.branch, &result.path)),
        SystemMutationResult::Lock(result) => Ok(CommandOutput::new(json!({
            "branch": result.branch,
            "reason": result.reason,
            "owner": result.owner,
        }))),
        SystemMutationResult::Unlock { branch } => {
            Ok(CommandOutput::new(json!({ "branch": branch })))
        }
        SystemMutationResult::DelGitApplied(_) => Err(CliError::new(
            ErrorCode::InternalError,
            "del metadata finalization was skipped",
        )),
        SystemMutationResult::Del(result) => {
            let human_stdout = format!("{}\n", result.path.display());
            let mut output = CommandOutput::new(json!({
                "branch": result.branch,
                "path": result.path,
            }));
            output.human_stdout = human_stdout;
            Ok(output)
        }
        SystemMutationResult::Gone(result) => {
            let mut human_stdout = String::new();
            let label = if result.dry_run {
                "candidate"
            } else {
                "deleted"
            };
            for branch in if result.dry_run {
                &result.candidates
            } else {
                &result.deleted
            } {
                let _ = writeln!(human_stdout, "{label}: {branch}");
            }
            for failed in &result.failed {
                let _ = writeln!(
                    human_stdout,
                    "failed: {} [{}] {}",
                    failed.branch, failed.code, failed.message
                );
            }
            let has_failures = !result.failed.is_empty();
            let data = serde_json::to_value(result).map_err(|error| serialization_error(&error))?;
            let mut output = if has_failures {
                CommandOutput::partial(
                    data,
                    CliError::new(
                        ErrorCode::GitCommandFailed,
                        "one or more stale worktrees could not be deleted",
                    ),
                )
            } else {
                CommandOutput::new(data)
            };
            output.human_stdout = human_stdout;
            Ok(output)
        }
        SystemMutationResult::Transfer(result) => {
            let direction = match result.direction {
                TransferDirection::Absorb => "absorb",
                TransferDirection::Unabsorb => "unabsorb",
            };
            Ok(path_result_with_extra(
                &result.branch,
                &result.path,
                &json!({
                    "direction": direction,
                    "sourcePath": result.source_path,
                    "stashed": result.stashed,
                    "stashRef": result.stash_ref,
                }),
            ))
        }
    }
}

fn path_result(branch: &str, path: &Path) -> CommandOutput {
    let human_stdout = format!("{}\n", path.display());
    let mut output = CommandOutput::new(json!({ "branch": branch, "path": path }));
    output.human_stdout = human_stdout;
    output
}

fn path_result_with_extra(branch: &str, path: &Path, extra: &Value) -> CommandOutput {
    let mut data = json!({ "branch": branch, "path": path });
    if let (Some(target), Some(fields)) = (data.as_object_mut(), extra.as_object()) {
        target.extend(fields.clone());
    }
    let mut output = CommandOutput::new(data);
    output.human_stdout = format!("{}\n", path.display());
    output
}

fn serialization_error(error: &serde_json::Error) -> CliError {
    CliError::new(
        ErrorCode::InternalError,
        format!("failed to serialize mutation result: {error}"),
    )
}

impl ApplicationBackend for SystemBackend {
    type LockGuard = RepoLock;
    type MutationPlan = SystemMutationPlan;
    type MutationStage = SystemMutationStage;
    type MutationResult = SystemMutationResult;

    fn resolve_repo_context(&self) -> Result<RepoContext, CliError> {
        self.git
            .resolve_repo_context(&self.cwd)
            .map_err(MapToCliError::map_to_cli_error)
    }

    fn resolve_config(&self, context: &RepoContext) -> Result<ResolvedConfig, CliError> {
        self.mutation_config(context)
    }

    fn is_initialized(&self, repo_root: &Path) -> Result<bool, CliError> {
        crate::app::diagnostics::initialization_status(repo_root)
    }

    fn acquire_repo_lock(
        &self,
        context: &RepoContext,
        timeout: Duration,
        command: &str,
    ) -> Result<Self::LockGuard, CliError> {
        acquire_repo_lock(&context.git_common_dir, timeout, command)
            .map_err(MapToCliError::map_to_cli_error)
    }

    fn run_hook(
        &self,
        phase: HookPhase,
        request: &ParsedRequest,
        context: &HookContext,
        timeout: Duration,
    ) -> Result<ApplicationHookResult, CliError> {
        let mut hook_context = context.clone();
        hook_context.timeout = timeout;
        let report = match phase {
            HookPhase::Pre => run_pre_hook(
                &hook_context.action,
                &hook_context,
                &SystemHookProcessRunner,
            ),
            HookPhase::Post => run_post_hook(
                &hook_context.action,
                &hook_context,
                &SystemHookProcessRunner,
                request.common.strict_post_hooks,
            ),
        }
        .map_err(MapToCliError::map_to_cli_error)?;
        if report.disposition == HookDisposition::Fatal {
            return Err(map_hook_report(&report));
        }
        if report.disposition == HookDisposition::Warning {
            let warning = map_hook_report(&report);
            return Ok(ApplicationHookResult::Warning(warning));
        }
        Ok(ApplicationHookResult::Continue)
    }

    fn recover_mutations(&self, context: &RepoContext) -> Result<Vec<CliError>, CliError> {
        let recovered =
            recover_pending_metadata_transactions(&context.repo_root).map_err(|error| {
                map_metadata_transaction_error(&error).at_phase(
                    ExecutionPhase::Recover,
                    ExecutionState::RecoveryRequired,
                    &[],
                )
            })?;
        Ok(recovered
            .into_iter()
            .map(|outcome| {
                CliError::new(
                    ErrorCode::MetadataRecoveryCompleted,
                    "completed pending metadata recovery before planning the requested operation",
                )
                .with_details(BTreeMap::from([(
                    "recoveryOutcome".to_owned(),
                    json!(outcome),
                )]))
                .at_phase(
                    ExecutionPhase::Recover,
                    ExecutionState::Applied,
                    &["recoverMetadata"],
                )
            })
            .collect())
    }

    fn inspect_mutation(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
    ) -> Result<CommandOutput, CliError> {
        let pending = crate::state::metadata_transaction::inspect_pending_metadata_transactions(
            &context.repo_root,
        )
        .map_err(|error| map_metadata_transaction_error(&error))?;
        let mut errors = Vec::new();
        if !matches!(request.command, Command::Init) && !self.is_initialized(&context.repo_root)? {
            errors.push(CliError::new(
                ErrorCode::NotInitialized,
                "Repository is not initialized. Run `vw init` first.",
            ));
        }
        if !pending.is_empty() {
            errors.push(CliError::new(
                ErrorCode::SafetyRejected,
                "pending metadata recovery must be resolved before this plan can be used",
            ));
        }
        let mut target = Value::Null;
        let mut planned_result = Value::Null;
        let evidence;
        let mut native_preview = None;
        match self.prepare_mutation(request, context) {
            Ok(plan) => {
                let hook = plan.hook_context(HookPhase::Post);
                if let Some(path) = &hook.worktree_path {
                    crate::app::target::ensure_path(path)?;
                }
                target = json!({"branch": hook.branch, "path": hook.worktree_path});
                evidence = plan.inspection_evidence;
                match &plan.command {
                    SystemMutationCommandPlan::Gone(plan) => {
                        let result = crate::app::mutations_delete::gone_dry_run_result(plan);
                        planned_result = json!(result);
                        native_preview =
                            Some(mutation_command_output(SystemMutationResult::Gone(result))?);
                    }
                    SystemMutationCommandPlan::Adopt(plan) => {
                        let result = crate::app::mutations_create::adopt_dry_run_result(plan);
                        planned_result = json!(result);
                        native_preview = Some(mutation_command_output(
                            SystemMutationResult::Adopt(result),
                        )?);
                    }
                    _ => {}
                }
            }
            Err(mut error) => {
                evidence = error
                    .details
                    .remove("preflightEvidence")
                    .unwrap_or(Value::Null);
                errors.push(error);
            }
        }
        if !request.common.dry_run
            && errors.is_empty()
            && let Some(output) = native_preview
        {
            return Ok(output);
        }
        crate::app::preflight::output(
            request,
            &target,
            &planned_result,
            &evidence,
            pending,
            errors,
        )
    }

    #[allow(clippy::too_many_lines)]
    fn prepare_mutation(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
    ) -> Result<Self::MutationPlan, CliError> {
        let config = self.mutation_config(context)?;
        let managed_root =
            resolve_managed_worktree_root(&context.repo_root, &config.paths.worktree_root);
        let action = request.command.name();
        let mut inspection_evidence = Value::Null;
        let (command, hooks, requires_hooks) = match &request.command {
            Command::Init => {
                let plan = prepare_init(context, managed_root)?;
                let target = plan.hook_target();
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    target.branch,
                    target.worktree_path,
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::Init(plan), hooks, true)
            }
            Command::New { branch } => {
                let registry = crate::app::snapshot::read_registry(&self.git, &context.repo_root)
                    .map_err(MapToCliError::map_to_cli_error)?;
                let branch = branch.clone().unwrap_or_else(generated_wip_branch);
                let base_branch = resolve_base_branch(
                    &self.git,
                    &context.repo_root,
                    config.git.base_branch.as_deref(),
                    &config.git.base_remote,
                )
                .map_err(MapToCliError::map_to_cli_error)?;
                let plan = prepare_new(
                    &self.git,
                    &context.repo_root,
                    &managed_root,
                    &registry,
                    &branch,
                    &base_branch,
                )?;
                let target = plan.hook_target();
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    target.branch,
                    target.worktree_path.clone(),
                    context.current_worktree_root.clone(),
                    target.worktree_path,
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::New(plan), hooks, true)
            }
            Command::Switch { branch } => {
                let registry = crate::app::snapshot::read_registry(&self.git, &context.repo_root)
                    .map_err(MapToCliError::map_to_cli_error)?;
                let base_branch = resolve_base_branch(
                    &self.git,
                    &context.repo_root,
                    config.git.base_branch.as_deref(),
                    &config.git.base_remote,
                )
                .map_err(MapToCliError::map_to_cli_error)?;
                let plan = prepare_switch(
                    &self.git,
                    &context.repo_root,
                    &managed_root,
                    &registry,
                    branch,
                    &base_branch,
                )?;
                let target = plan.hook_target();
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    target.branch,
                    target.worktree_path.clone(),
                    context.current_worktree_root.clone(),
                    target.worktree_path,
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::Switch(plan), hooks, true)
            }
            Command::Get { remote_branch } => {
                let registry = crate::app::snapshot::read_registry(&self.git, &context.repo_root)
                    .map_err(MapToCliError::map_to_cli_error)?;
                let base_branch = resolve_base_branch(
                    &self.git,
                    &context.repo_root,
                    config.git.base_branch.as_deref(),
                    &config.git.base_remote,
                )
                .map_err(MapToCliError::map_to_cli_error)?;
                let plan = prepare_get(
                    &self.git,
                    &context.repo_root,
                    &managed_root,
                    &registry,
                    remote_branch,
                    &base_branch,
                )?;
                let target = plan.hook_target();
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    target.branch,
                    target.worktree_path.clone(),
                    context.current_worktree_root.clone(),
                    target.worktree_path,
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::Get(plan), hooks, true)
            }
            Command::Adopt { apply, .. } => {
                let snapshot = self.mutation_snapshot(context, &config, false)?;
                let plan = prepare_adopt(&context.repo_root, &managed_root, &snapshot, !apply)?;
                let target = plan.hook_target();
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    target.branch,
                    target.worktree_path.clone(),
                    context.repo_root.clone(),
                    target.worktree_path,
                    BTreeMap::new(),
                );
                let requires_hooks = !plan.dry_run;
                (
                    SystemMutationCommandPlan::Adopt(plan),
                    hooks,
                    requires_hooks,
                )
            }
            Command::Mv { new_branch } => {
                let snapshot = self.mutation_snapshot(context, &config, false)?;
                let target_path = managed_root.join(new_branch);
                let plan = prepare_mv(
                    &self.git,
                    context,
                    &managed_root,
                    &snapshot,
                    new_branch,
                    &target_path,
                )?;
                let extra_env = match &plan {
                    MvPlan::Apply(plan) => BTreeMap::from([
                        ("WT_OLD_BRANCH".to_owned(), plan.old_branch.clone()),
                        ("WT_NEW_BRANCH".to_owned(), plan.new_branch.clone()),
                    ]),
                    MvPlan::Noop(_) => BTreeMap::new(),
                };
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    Some(plan.branch().to_owned()),
                    plan.target_path().to_path_buf(),
                    context.current_worktree_root.clone(),
                    plan.target_path().to_path_buf(),
                    extra_env,
                );
                let requires_hooks = plan.requires_hooks();
                (SystemMutationCommandPlan::Mv(plan), hooks, requires_hooks)
            }
            Command::Extract { stash, .. } => {
                let snapshot = self.mutation_snapshot(context, &config, false)?;
                let base_branch = snapshot.base_branch.as_deref().ok_or_else(|| {
                    CliError::new(ErrorCode::InvalidArgument, "base branch is unavailable")
                })?;
                let current = snapshot
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.path == context.current_worktree_root)
                    .ok_or_else(|| {
                        CliError::new(
                            ErrorCode::WorktreeNotFound,
                            "current worktree was not found",
                        )
                    })?;
                let branch = current.branch.as_deref().ok_or_else(|| {
                    CliError::new(
                        ErrorCode::DetachedHead,
                        "extract requires a branch checkout",
                    )
                })?;
                let target_path = managed_root.join(branch);
                let plan = prepare_extract(
                    &self.git,
                    context,
                    &managed_root,
                    &snapshot,
                    base_branch,
                    &target_path,
                    *stash,
                )?;
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    Some(plan.branch.clone()),
                    plan.target_path.clone(),
                    context.repo_root.clone(),
                    plan.target_path.clone(),
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::Extract(plan), hooks, true)
            }
            Command::Use {
                branch,
                allow_agent,
                allow_shared,
            } => {
                let snapshot = self.mutation_snapshot(context, &config, false)?;
                let invocation = if self.terminal.stdout_tty && self.terminal.stderr_tty {
                    UseInvocation::Interactive
                } else {
                    UseInvocation::NonInteractive {
                        allow_agent: *allow_agent,
                        allow_unsafe: request.common.allow_unsafe,
                    }
                };
                let plan = prepare_use(
                    &self.git,
                    context,
                    &snapshot,
                    branch,
                    UseOptions {
                        invocation,
                        sharing: if *allow_shared {
                            UseSharing::Allow
                        } else {
                            UseSharing::Reject
                        },
                    },
                )?;
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    Some(plan.branch.clone()),
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::Use(plan), hooks, true)
            }
            Command::Del {
                branch,
                force,
                force_dirty,
                allow_unpushed,
                force_unmerged,
                force_locked,
            } => {
                let snapshot = self.selected_mutation_snapshot(
                    context,
                    &config,
                    branch.as_deref(),
                    request.common.gh_enabled(config.github.enabled),
                )?;
                let force = DeleteForceOptions {
                    force_dirty: *force || *force_dirty,
                    allow_unpushed: *force || *allow_unpushed,
                    force_unmerged: *force || *force_unmerged,
                    force_locked: *force || *force_locked,
                };
                inspection_evidence = crate::app::preflight::deletion_evidence(
                    &context.repo_root,
                    &managed_root,
                    &snapshot,
                    force,
                    false,
                )?;
                let plan = prepare_del(
                    &context.repo_root,
                    &context.current_worktree_root,
                    &managed_root,
                    &snapshot,
                    branch.as_deref(),
                    force,
                    DeleteInvocation {
                        interactive: self.terminal.stdout_tty && self.terminal.stderr_tty,
                        allow_unsafe: request.common.allow_unsafe,
                    },
                )
                .map_err(|mut error| {
                    error
                        .details
                        .insert("preflightEvidence".to_owned(), inspection_evidence.clone());
                    error
                })?;
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    Some(plan.branch.clone()),
                    plan.path.clone(),
                    plan.path.clone(),
                    context.repo_root.clone(),
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::Del(Box::new(plan)), hooks, true)
            }
            Command::Gone { apply, .. } => {
                let snapshot = self.mutation_snapshot(
                    context,
                    &config,
                    request.common.gh_enabled(config.github.enabled),
                )?;
                inspection_evidence = crate::app::preflight::deletion_evidence(
                    &context.repo_root,
                    &managed_root,
                    &snapshot,
                    DeleteForceOptions::default(),
                    true,
                )?;
                let plan = prepare_gone(&context.repo_root, &managed_root, &snapshot, !apply)
                    .map_err(|mut error| {
                        error
                            .details
                            .insert("preflightEvidence".to_owned(), inspection_evidence.clone());
                        error
                    })?;
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    None,
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    BTreeMap::new(),
                );
                let requires_hooks = plan.requires_hooks();
                (SystemMutationCommandPlan::Gone(plan), hooks, requires_hooks)
            }
            Command::Absorb {
                branch,
                from,
                keep_stash,
                allow_agent,
            } => {
                let registry = crate::app::snapshot::read_registry(&self.git, &context.repo_root)
                    .map_err(MapToCliError::map_to_cli_error)?;
                let options = TransferOptions {
                    invocation: transfer_invocation(
                        self.terminal,
                        *allow_agent,
                        request.common.allow_unsafe,
                    ),
                    requested_worktree: from.as_deref().map(PathBuf::from),
                    retention: if *keep_stash {
                        StashRetention::Keep
                    } else {
                        StashRetention::DropAfterApply
                    },
                };
                let plan = prepare_absorb(
                    &self.git,
                    context,
                    &registry,
                    &managed_root,
                    branch,
                    &options,
                )?;
                let hooks = transfer_hooks(self.terminal, &context.repo_root, action, &plan);
                (SystemMutationCommandPlan::Transfer(plan), hooks, true)
            }
            Command::Unabsorb {
                branch,
                to,
                keep_stash,
                allow_agent,
            } => {
                let registry = crate::app::snapshot::read_registry(&self.git, &context.repo_root)
                    .map_err(MapToCliError::map_to_cli_error)?;
                let options = TransferOptions {
                    invocation: transfer_invocation(
                        self.terminal,
                        *allow_agent,
                        request.common.allow_unsafe,
                    ),
                    requested_worktree: to.as_deref().map(PathBuf::from),
                    retention: if *keep_stash {
                        StashRetention::Keep
                    } else {
                        StashRetention::DropAfterApply
                    },
                };
                let plan = prepare_unabsorb(
                    &self.git,
                    context,
                    &registry,
                    &managed_root,
                    branch,
                    &options,
                )?;
                let hooks = transfer_hooks(self.terminal, &context.repo_root, action, &plan);
                (SystemMutationCommandPlan::Transfer(plan), hooks, true)
            }
            Command::Lock {
                branch,
                owner,
                reason,
            } => {
                let registry = crate::app::snapshot::read_registry(&self.git, &context.repo_root)
                    .map_err(MapToCliError::map_to_cli_error)?;
                let owner = owner.clone().map_or_else(default_lock_owner, Ok)?;
                let reason = reason.clone().unwrap_or_else(|| "locked".to_owned());
                let plan = prepare_lock(
                    &context.repo_root,
                    &registry,
                    branch,
                    &reason,
                    &owner,
                    &local_host_name()?,
                    std::process::id(),
                )?;
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    Some(branch.clone()),
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::Lock(plan), hooks, false)
            }
            Command::Unlock {
                branch,
                owner,
                force,
            } => {
                let owner = owner.clone().map_or_else(default_lock_owner, Ok)?;
                let plan = prepare_unlock(&context.repo_root, branch, &owner, *force)?;
                let hooks = mutation_hooks(
                    self.terminal,
                    &context.repo_root,
                    action,
                    Some(branch.clone()),
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    context.repo_root.clone(),
                    BTreeMap::new(),
                );
                (SystemMutationCommandPlan::Unlock(plan), hooks, false)
            }
            _ => {
                return Err(CliError::new(
                    ErrorCode::InternalError,
                    format!("{} mutation is not implemented yet", request.command.name()),
                ));
            }
        };
        Ok(SystemMutationPlan {
            command,
            inspection_evidence,
            hooks,
            requires_hooks,
        })
    }

    fn apply_mutation(
        &self,
        request: &ParsedRequest,
        context: &RepoContext,
        plan: &Self::MutationPlan,
        stage: Self::MutationStage,
    ) -> Result<Self::MutationResult, CliError> {
        match &plan.command {
            SystemMutationCommandPlan::Init(plan) => {
                apply_init(plan).map(SystemMutationResult::Init)
            }
            SystemMutationCommandPlan::New(plan) => apply_new_git(&self.git, plan)
                .map(Box::new)
                .map(SystemMutationResult::WorktreeGitApplied),
            SystemMutationCommandPlan::Switch(plan) => apply_switch_git(&self.git, plan)
                .map(Box::new)
                .map(SystemMutationResult::WorktreeGitApplied),
            SystemMutationCommandPlan::Get(plan) => apply_get_git(&self.git, plan)
                .map(Box::new)
                .map(SystemMutationResult::WorktreeGitApplied),
            SystemMutationCommandPlan::Adopt(plan) => {
                apply_adopt(&self.git, plan).map(SystemMutationResult::Adopt)
            }
            SystemMutationCommandPlan::Mv(_) => match stage {
                SystemMutationStage::Mv(staged) => {
                    apply_mv_git(&self.git, &context.repo_root, staged)
                        .map(Box::new)
                        .map(SystemMutationResult::MvGitApplied)
                }
                _ => Err(CliError::new(
                    ErrorCode::InternalError,
                    "mv mutation stage is missing",
                )),
            },
            SystemMutationCommandPlan::Extract(_) => match stage {
                SystemMutationStage::Extract(staged) => apply_extract_git(&self.git, staged)
                    .map(Box::new)
                    .map(SystemMutationResult::ExtractGitApplied),
                _ => Err(CliError::new(
                    ErrorCode::InternalError,
                    "extract mutation stage is missing",
                )),
            },
            SystemMutationCommandPlan::Use(plan) => {
                apply_use(&self.git, plan.clone()).map(SystemMutationResult::Use)
            }
            SystemMutationCommandPlan::Lock(plan) => {
                apply_lock(plan).map(SystemMutationResult::Lock)
            }
            SystemMutationCommandPlan::Unlock(plan) => {
                apply_unlock(plan.clone())?;
                let branch = match plan {
                    UnlockPlan::Noop { branch }
                    | UnlockPlan::RemoveValid { branch, .. }
                    | UnlockPlan::RemoveInvalid { branch, .. } => branch.clone(),
                };
                Ok(SystemMutationResult::Unlock { branch })
            }
            SystemMutationCommandPlan::Del(plan) => {
                let config = self.mutation_config(context)?;
                let latest = self.selected_mutation_snapshot(
                    context,
                    &config,
                    Some(&plan.branch),
                    request.common.gh_enabled(config.github.enabled),
                )?;
                let revalidated = revalidate_del(plan, &latest)?;
                apply_del_git(&self.git, revalidated)
                    .map(Box::new)
                    .map(SystemMutationResult::DelGitApplied)
            }
            SystemMutationCommandPlan::Gone(plan) => {
                let snapshots = |candidate: &crate::app::mutations_delete::GoneCandidate| {
                    let config = self.mutation_config(context)?;
                    self.selected_mutation_snapshot(
                        context,
                        &config,
                        Some(&candidate.branch),
                        request.common.gh_enabled(config.github.enabled),
                    )
                };
                Ok(SystemMutationResult::GoneGitApplied(Box::new(
                    apply_gone_git(&self.git, &snapshots, plan),
                )))
            }
            SystemMutationCommandPlan::Transfer(_) => match stage {
                SystemMutationStage::Transfer(staged) => {
                    apply_transfer(&self.git, staged).map(SystemMutationResult::Transfer)
                }
                _ => Err(CliError::new(
                    ErrorCode::InternalError,
                    "transfer mutation stage is missing",
                )),
            },
        }
    }

    fn stage_mutation(
        &self,
        _request: &ParsedRequest,
        _context: &RepoContext,
        plan: &Self::MutationPlan,
    ) -> Result<Self::MutationStage, CliError> {
        match &plan.command {
            SystemMutationCommandPlan::Mv(plan) => {
                stage_mv_for_hook(plan.clone()).map(SystemMutationStage::Mv)
            }
            SystemMutationCommandPlan::Extract(plan) => {
                stage_extract_for_hook(&self.git, plan.clone()).map(SystemMutationStage::Extract)
            }
            SystemMutationCommandPlan::Transfer(plan) => {
                stage_transfer(&self.git, plan.clone()).map(SystemMutationStage::Transfer)
            }
            _ => Ok(SystemMutationStage::None),
        }
    }

    fn rollback_mutation_stage(
        &self,
        _request: &ParsedRequest,
        _context: &RepoContext,
        _plan: &Self::MutationPlan,
        stage: &Self::MutationStage,
    ) -> Result<(), CliError> {
        match stage {
            SystemMutationStage::Mv(staged) => rollback_mv_after_pre_hook_failure(staged),
            SystemMutationStage::Extract(staged) => {
                restore_extract_after_pre_hook_failure(&self.git, staged)
            }
            SystemMutationStage::Transfer(staged) => {
                rollback_transfer_after_pre_hook_failure(&self.git, staged)
            }
            SystemMutationStage::None => Ok(()),
        }
    }

    fn update_mutation_state(
        &self,
        _request: &ParsedRequest,
        _context: &RepoContext,
        _plan: &Self::MutationPlan,
        result: Self::MutationResult,
    ) -> Result<CommandOutput, CliError> {
        match result {
            SystemMutationResult::WorktreeGitApplied(applied) => {
                let result = finalize_worktree_state(&self.git, *applied)?;
                mutation_command_output(SystemMutationResult::Worktree(result))
            }
            SystemMutationResult::MvGitApplied(applied) => {
                let result = finalize_mv_state(*applied)?;
                mutation_command_output(SystemMutationResult::Mv(result))
            }
            SystemMutationResult::ExtractGitApplied(applied) => {
                let result = finalize_extract_state(&self.git, *applied)?;
                mutation_command_output(SystemMutationResult::Extract(result))
            }
            SystemMutationResult::DelGitApplied(applied) => {
                let result = finalize_del_state(*applied, &FilesystemDeleteMutationState)?;
                mutation_command_output(SystemMutationResult::Del(result))
            }
            SystemMutationResult::GoneGitApplied(applied) => {
                let result = finalize_gone_state(*applied, &FilesystemDeleteMutationState);
                mutation_command_output(SystemMutationResult::Gone(result))
            }
            result => mutation_command_output(result),
        }
    }

    fn execute(
        &self,
        request: &ParsedRequest,
        context: Option<&RepoContext>,
    ) -> Result<CommandOutput, CliError> {
        if matches!(request.command, Command::Doctor) {
            return Ok(crate::app::diagnostics::doctor_output(
                request,
                &self.cwd,
                &self.resolve_repo_context(),
                &self.git,
                &StdProcessRunner,
            ));
        }
        if matches!(request.command, Command::Context) {
            return crate::app::diagnostics::context_output(
                request,
                &self.cwd,
                context.expect("context command resolves repository"),
                &self.git,
            );
        }
        if let Command::Describe { command } = &request.command {
            let data = crate::cli::contract::describe(command.as_deref())?;
            let mut output = CommandOutput::new(data);
            let mut definition = crate::cli::clap_command();
            definition.build();
            let help = if let Some(command) = command {
                definition
                    .find_subcommand_mut(command)
                    .expect("validated command")
            } else {
                &mut definition
            };
            output.human_stdout = help.render_long_help().to_string();
            output.human_stdout.push('\n');
            return Ok(output);
        }
        if let Some(result) = execute_completion(request, self.home.as_deref()) {
            return result.map(Into::into);
        }
        if let Some(context) = context {
            let loaded = load_resolved_config(&self.cwd, &context.repo_root)
                .map_err(MapToCliError::map_to_cli_error)?;
            if let Some(result) = execute_misc_command(
                request,
                context,
                &loaded.config,
                &self.git,
                &StdProcessRunner,
                self.terminal.stdout_tty && self.terminal.stderr_tty,
            ) {
                return result.map(Into::into);
            }
            let runtime = ReadCommandRuntime {
                git: &self.git,
                pr_lookup: &self.gh,
                fzf: &self.fzf,
                terminal: self.terminal,
                home: self.home.as_deref(),
                in_tmux: self.in_tmux,
            };
            if let Some(result) = execute_read_command(request, context, &loaded.config, &runtime) {
                return result;
            }
        }
        Err(CliError::new(
            ErrorCode::InternalError,
            format!(
                "{} is not implemented in the Rust migration build yet",
                request.command.name()
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use serde_json::Value;

    use super::*;
    use crate::app::error_mapper::MapToCliError;
    use crate::cli::{CliParseResult, parse_from};
    use crate::ports::process::{
        ProcessCommand, ProcessError, ProcessOutput as ChildProcessOutput, ProcessRunner,
    };

    struct FailingGitRunner;

    impl ProcessRunner for FailingGitRunner {
        fn run(&self, _command: &ProcessCommand) -> Result<ChildProcessOutput, ProcessError> {
            Ok(ChildProcessOutput {
                stdout: b"partial".to_vec(),
                stderr: b"fatal: fake adapter failure".to_vec(),
                exit_code: Some(128),
                timed_out: false,
            })
        }
    }

    #[derive(Clone, Debug)]
    struct FakeMutationPlan {
        hooks: MutationHookContexts,
    }

    impl PlannedMutation for FakeMutationPlan {
        fn hook_context(&self, phase: HookPhase) -> &HookContext {
            self.hooks.for_phase(phase)
        }
    }

    #[derive(Debug)]
    struct FakeBackend {
        context: Result<RepoContext, CliError>,
        config: ResolvedConfig,
        initialized: bool,
        lock_error: Option<CliError>,
        pre_error: Option<CliError>,
        post_error: Option<CliError>,
        post_warning: Option<String>,
        plan_error: Option<CliError>,
        apply_error: Option<CliError>,
        rollback_error: Option<CliError>,
        execute_result: Result<CommandOutput, CliError>,
        initialized_calls: Cell<usize>,
        lock_calls: Cell<usize>,
        lock_timeouts: RefCell<Vec<Duration>>,
        plan_calls: Cell<usize>,
        hook_calls: RefCell<Vec<HookPhase>>,
        hook_timeouts: RefCell<Vec<Duration>>,
        hook_contexts: RefCell<Vec<(HookPhase, HookContext)>>,
        apply_calls: Cell<usize>,
        state_calls: Cell<usize>,
        execute_calls: Cell<usize>,
        trace: RefCell<Vec<&'static str>>,
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                context: Ok(RepoContext {
                    repo_root: PathBuf::from("/repo"),
                    current_worktree_root: PathBuf::from("/repo"),
                    git_common_dir: PathBuf::from("/repo/.git"),
                }),
                config: ResolvedConfig::default(),
                initialized: true,
                lock_error: None,
                pre_error: None,
                post_error: None,
                post_warning: None,
                plan_error: None,
                apply_error: None,
                rollback_error: None,
                execute_result: Ok(CommandOutput::new(json!({"accepted": true}))),
                initialized_calls: Cell::new(0),
                lock_calls: Cell::new(0),
                lock_timeouts: RefCell::new(Vec::new()),
                plan_calls: Cell::new(0),
                hook_calls: RefCell::new(Vec::new()),
                hook_timeouts: RefCell::new(Vec::new()),
                hook_contexts: RefCell::new(Vec::new()),
                apply_calls: Cell::new(0),
                state_calls: Cell::new(0),
                execute_calls: Cell::new(0),
                trace: RefCell::new(Vec::new()),
            }
        }
    }

    impl ApplicationBackend for FakeBackend {
        type LockGuard = ();
        type MutationPlan = FakeMutationPlan;
        type MutationStage = ();
        type MutationResult = ();

        fn resolve_repo_context(&self) -> Result<RepoContext, CliError> {
            self.context.clone()
        }

        fn resolve_config(&self, _context: &RepoContext) -> Result<ResolvedConfig, CliError> {
            Ok(self.config.clone())
        }

        fn is_initialized(&self, _repo_root: &Path) -> Result<bool, CliError> {
            self.initialized_calls.set(self.initialized_calls.get() + 1);
            Ok(self.initialized)
        }

        fn acquire_repo_lock(
            &self,
            _context: &RepoContext,
            timeout: Duration,
            _command: &str,
        ) -> Result<Self::LockGuard, CliError> {
            self.lock_calls.set(self.lock_calls.get() + 1);
            self.lock_timeouts.borrow_mut().push(timeout);
            self.trace.borrow_mut().push("lock");
            self.lock_error.clone().map_or(Ok(()), Err)
        }

        fn run_hook(
            &self,
            phase: HookPhase,
            _request: &ParsedRequest,
            context: &HookContext,
            timeout: Duration,
        ) -> Result<ApplicationHookResult, CliError> {
            self.hook_calls.borrow_mut().push(phase);
            self.hook_timeouts.borrow_mut().push(timeout);
            self.hook_contexts
                .borrow_mut()
                .push((phase, context.clone()));
            self.trace.borrow_mut().push(match phase {
                HookPhase::Pre => "pre-hook",
                HookPhase::Post => "post-hook",
            });
            match phase {
                HookPhase::Pre => self
                    .pre_error
                    .clone()
                    .map_or(Ok(ApplicationHookResult::Continue), Err),
                HookPhase::Post => self.post_error.clone().map_or_else(
                    || {
                        Ok(self.post_warning.clone().map_or(
                            ApplicationHookResult::Continue,
                            |warning| {
                                ApplicationHookResult::Warning(CliError::new(
                                    ErrorCode::HookFailed,
                                    warning.trim().trim_start_matches("Warning: "),
                                ))
                            },
                        ))
                    },
                    Err,
                ),
            }
        }

        fn prepare_mutation(
            &self,
            request: &ParsedRequest,
            context: &RepoContext,
        ) -> Result<Self::MutationPlan, CliError> {
            self.plan_calls.set(self.plan_calls.get() + 1);
            self.trace.borrow_mut().push("plan");
            if let Some(error) = &self.plan_error {
                return Err(error.clone());
            }
            let branch = match &request.command {
                Command::New { branch } => branch
                    .clone()
                    .unwrap_or_else(|| "generated-branch".to_owned()),
                _ => "planned-target".to_owned(),
            };
            let target = context.repo_root.join(".worktrees").join(&branch);
            Ok(FakeMutationPlan {
                hooks: MutationHookContexts::new(
                    context.repo_root.clone(),
                    request.command.name(),
                    Some(branch),
                    Some(target.clone()),
                    context.current_worktree_root.clone(),
                    target,
                    false,
                    BTreeMap::from([("WT_PLAN".to_owned(), "fixed".to_owned())]),
                ),
            })
        }

        fn apply_mutation(
            &self,
            _request: &ParsedRequest,
            _context: &RepoContext,
            _plan: &Self::MutationPlan,
            _stage: Self::MutationStage,
        ) -> Result<Self::MutationResult, CliError> {
            self.apply_calls.set(self.apply_calls.get() + 1);
            self.trace.borrow_mut().push("apply");
            self.apply_error.clone().map_or(Ok(()), Err)
        }

        fn stage_mutation(
            &self,
            _request: &ParsedRequest,
            _context: &RepoContext,
            _plan: &Self::MutationPlan,
        ) -> Result<Self::MutationStage, CliError> {
            self.trace.borrow_mut().push("stage");
            Ok(())
        }

        fn rollback_mutation_stage(
            &self,
            _request: &ParsedRequest,
            _context: &RepoContext,
            _plan: &Self::MutationPlan,
            _stage: &Self::MutationStage,
        ) -> Result<(), CliError> {
            self.trace.borrow_mut().push("rollback-stage");
            self.rollback_error.clone().map_or(Ok(()), Err)
        }

        fn update_mutation_state(
            &self,
            _request: &ParsedRequest,
            _context: &RepoContext,
            _plan: &Self::MutationPlan,
            _result: Self::MutationResult,
        ) -> Result<CommandOutput, CliError> {
            self.state_calls.set(self.state_calls.get() + 1);
            self.trace.borrow_mut().push("state");
            self.execute_result.clone()
        }

        fn execute(
            &self,
            _request: &ParsedRequest,
            _context: Option<&RepoContext>,
        ) -> Result<CommandOutput, CliError> {
            self.execute_calls.set(self.execute_calls.get() + 1);
            self.execute_result.clone()
        }
    }

    #[derive(Debug, Default)]
    struct FailingGitBackend {
        base: FakeBackend,
    }

    impl ApplicationBackend for FailingGitBackend {
        type LockGuard = ();
        type MutationPlan = FakeMutationPlan;
        type MutationStage = ();
        type MutationResult = ();

        fn resolve_repo_context(&self) -> Result<RepoContext, CliError> {
            self.base.resolve_repo_context()
        }

        fn resolve_config(&self, context: &RepoContext) -> Result<ResolvedConfig, CliError> {
            self.base.resolve_config(context)
        }

        fn is_initialized(&self, repo_root: &Path) -> Result<bool, CliError> {
            self.base.is_initialized(repo_root)
        }

        fn acquire_repo_lock(
            &self,
            context: &RepoContext,
            timeout: Duration,
            command: &str,
        ) -> Result<Self::LockGuard, CliError> {
            self.base.acquire_repo_lock(context, timeout, command)
        }

        fn run_hook(
            &self,
            phase: HookPhase,
            request: &ParsedRequest,
            context: &HookContext,
            timeout: Duration,
        ) -> Result<ApplicationHookResult, CliError> {
            self.base.run_hook(phase, request, context, timeout)
        }

        fn prepare_mutation(
            &self,
            request: &ParsedRequest,
            context: &RepoContext,
        ) -> Result<Self::MutationPlan, CliError> {
            self.base.prepare_mutation(request, context)
        }

        fn apply_mutation(
            &self,
            request: &ParsedRequest,
            context: &RepoContext,
            plan: &Self::MutationPlan,
            stage: Self::MutationStage,
        ) -> Result<Self::MutationResult, CliError> {
            self.base.apply_mutation(request, context, plan, stage)
        }

        fn stage_mutation(
            &self,
            request: &ParsedRequest,
            context: &RepoContext,
            plan: &Self::MutationPlan,
        ) -> Result<Self::MutationStage, CliError> {
            self.base.stage_mutation(request, context, plan)
        }

        fn rollback_mutation_stage(
            &self,
            request: &ParsedRequest,
            context: &RepoContext,
            plan: &Self::MutationPlan,
            stage: &Self::MutationStage,
        ) -> Result<(), CliError> {
            self.base
                .rollback_mutation_stage(request, context, plan, stage)
        }

        fn update_mutation_state(
            &self,
            request: &ParsedRequest,
            context: &RepoContext,
            plan: &Self::MutationPlan,
            result: Self::MutationResult,
        ) -> Result<CommandOutput, CliError> {
            self.base
                .update_mutation_state(request, context, plan, result)
        }

        fn execute(
            &self,
            _request: &ParsedRequest,
            _context: Option<&RepoContext>,
        ) -> Result<CommandOutput, CliError> {
            GitCli::new(FailingGitRunner)
                .execute_checked(Path::new("/repo"), ["status", "--porcelain"])
                .map(|_| CommandOutput::new(json!({})))
                .map_err(MapToCliError::map_to_cli_error)
        }
    }

    fn request(args: &[&str]) -> ParsedRequest {
        let mut full_args = vec!["vw"];
        full_args.extend_from_slice(args);
        match parse_from(full_args) {
            CliParseResult::Parsed(request) => request,
            outcome => panic!("expected parsed request, got {outcome:?}"),
        }
    }

    fn error_code(output: &ProcessOutput) -> String {
        let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON output");
        value["error"]["code"]
            .as_str()
            .expect("error code string")
            .to_owned()
    }

    #[test]
    fn all_thirteen_non_init_write_commands_reject_uninitialized_repositories_first() {
        let cases: [&[&str]; 13] = [
            &["new"],
            &["switch", "main"],
            &["mv", "renamed"],
            &["del"],
            &["gone"],
            &["adopt"],
            &["get", "origin/main"],
            &["extract", "--current"],
            &["absorb", "main"],
            &["unabsorb", "main"],
            &["use", "main"],
            &["lock", "main"],
            &["unlock", "main"],
        ];

        for args in cases {
            let backend = FakeBackend {
                initialized: false,
                ..FakeBackend::default()
            };
            let mut args = args.to_vec();
            args.push("--json");
            let output = dispatch(&request(&args), &backend);

            assert_eq!(output.exit_code, 4, "{args:?}");
            assert_eq!(error_code(&output), "NOT_INITIALIZED", "{args:?}");
            assert_eq!(backend.initialized_calls.get(), 1, "{args:?}");
            assert_eq!(backend.lock_calls.get(), 0, "{args:?}");
            assert_eq!(backend.plan_calls.get(), 0, "{args:?}");
            assert!(backend.hook_calls.borrow().is_empty(), "{args:?}");
            assert_eq!(backend.apply_calls.get(), 0, "{args:?}");
            assert_eq!(backend.state_calls.get(), 0, "{args:?}");
            assert_eq!(backend.execute_calls.get(), 0, "{args:?}");
        }
    }

    #[test]
    fn lock_timeout_is_rendered_before_hooks_or_command_execution() {
        let backend = FakeBackend {
            lock_error: Some(CliError::new(
                ErrorCode::RepoLockTimeout,
                "repository lock timed out",
            )),
            ..FakeBackend::default()
        };

        let output = dispatch(
            &request(&["new", "feature/a", "--json", "--lock-timeout-ms", "25"]),
            &backend,
        );

        assert_eq!(output.exit_code, 6);
        assert_eq!(error_code(&output), "REPO_LOCK_TIMEOUT");
        assert_eq!(backend.lock_calls.get(), 1);
        assert_eq!(*backend.lock_timeouts.borrow(), [Duration::from_millis(25)]);
        assert_eq!(backend.plan_calls.get(), 0);
        assert!(backend.hook_calls.borrow().is_empty());
        assert_eq!(backend.apply_calls.get(), 0);
        assert_eq!(backend.state_calls.get(), 0);
        assert_eq!(backend.execute_calls.get(), 0);
    }

    #[test]
    fn resolved_config_controls_hooks_and_timeouts_with_cli_timeout_precedence() {
        let mut config = ResolvedConfig::default();
        config.hooks.timeout_ms = 123;
        config.locks.timeout_ms = 456;
        let backend = FakeBackend {
            config: config.clone(),
            ..FakeBackend::default()
        };

        let output = dispatch(&request(&["new", "feature/config", "--json"]), &backend);

        assert_eq!(output.exit_code, 0);
        assert_eq!(
            *backend.lock_timeouts.borrow(),
            [Duration::from_millis(456)]
        );
        assert_eq!(
            *backend.hook_timeouts.borrow(),
            [Duration::from_millis(123), Duration::from_millis(123)]
        );

        let cli_override = FakeBackend {
            config,
            ..FakeBackend::default()
        };
        let output = dispatch(
            &request(&[
                "new",
                "feature/cli",
                "--json",
                "--hook-timeout-ms",
                "25",
                "--lock-timeout-ms",
                "35",
            ]),
            &cli_override,
        );

        assert_eq!(output.exit_code, 0);
        assert_eq!(
            *cli_override.lock_timeouts.borrow(),
            [Duration::from_millis(35)]
        );
        assert_eq!(
            *cli_override.hook_timeouts.borrow(),
            [Duration::from_millis(25), Duration::from_millis(25)]
        );
    }

    #[test]
    fn explicit_hooks_flag_overrides_disabled_configuration() {
        let mut config = ResolvedConfig::default();
        config.hooks.enabled = false;
        let backend = FakeBackend {
            config,
            ..FakeBackend::default()
        };

        let output = dispatch(
            &request(&["new", "feature/no-hooks", "--hooks", "--json"]),
            &backend,
        );

        assert_eq!(output.exit_code, 0);
        assert_eq!(
            *backend.hook_calls.borrow(),
            [HookPhase::Pre, HookPhase::Post]
        );
        assert_eq!(
            *backend.trace.borrow(),
            [
                "lock",
                "plan",
                "stage",
                "pre-hook",
                "apply",
                "state",
                "post-hook"
            ]
        );
    }

    #[test]
    fn mutation_pipeline_fixes_target_context_before_hooks_and_runs_in_order() {
        let backend = FakeBackend::default();

        let output = dispatch(&request(&["new", "feature/future", "--json"]), &backend);

        assert_eq!(output.exit_code, 0);
        assert_eq!(
            *backend.trace.borrow(),
            [
                "lock",
                "plan",
                "stage",
                "pre-hook",
                "apply",
                "state",
                "post-hook"
            ]
        );
        let contexts = backend.hook_contexts.borrow();
        assert_eq!(contexts.len(), 2);
        let pre = &contexts[0].1;
        let post = &contexts[1].1;
        let target = PathBuf::from("/repo/.worktrees/feature/future");
        assert_eq!(pre.branch.as_deref(), Some("feature/future"));
        assert_eq!(pre.worktree_path.as_deref(), Some(target.as_path()));
        assert_eq!(pre.execution_cwd.as_deref(), Some(Path::new("/repo")));
        assert_eq!(post.worktree_path.as_deref(), Some(target.as_path()));
        assert_eq!(post.execution_cwd.as_deref(), Some(target.as_path()));
        assert_eq!(pre.environment()["WT_PLAN"], "fixed");
        assert_eq!(
            pre.environment()["WT_WORKTREE_PATH"],
            target.display().to_string()
        );
    }

    #[test]
    fn mutation_failures_never_run_a_later_pipeline_stage() {
        let plan_failure = FakeBackend {
            plan_error: Some(CliError::new(
                ErrorCode::LockConflict,
                "target already exists",
            )),
            ..FakeBackend::default()
        };
        let output = dispatch(
            &request(&["new", "feature/conflict", "--json"]),
            &plan_failure,
        );
        assert_eq!(output.exit_code, 4);
        assert_eq!(*plan_failure.trace.borrow(), ["lock", "plan"]);

        let apply_failure = FakeBackend {
            apply_error: Some(CliError::new(ErrorCode::GitCommandFailed, "apply failed")),
            ..FakeBackend::default()
        };
        let output = dispatch(
            &request(&["new", "feature/apply", "--json"]),
            &apply_failure,
        );
        assert_eq!(output.exit_code, 20);
        assert_eq!(
            *apply_failure.trace.borrow(),
            ["lock", "plan", "stage", "pre-hook", "apply"]
        );
        assert_eq!(apply_failure.state_calls.get(), 0);
        assert_eq!(*apply_failure.hook_calls.borrow(), [HookPhase::Pre]);

        let state_failure = FakeBackend {
            execute_result: Err(CliError::new(
                ErrorCode::InternalError,
                "state update failed",
            )),
            ..FakeBackend::default()
        };
        let output = dispatch(
            &request(&["new", "feature/state", "--json"]),
            &state_failure,
        );
        assert_eq!(output.exit_code, 30);
        assert_eq!(
            *state_failure.trace.borrow(),
            ["lock", "plan", "stage", "pre-hook", "apply", "state"]
        );
        assert_eq!(*state_failure.hook_calls.borrow(), [HookPhase::Pre]);
    }

    #[test]
    fn pre_hook_failure_stops_execution_and_strict_post_failure_changes_success_to_error() {
        let pre_failure = FakeBackend {
            pre_error: Some(CliError::new(ErrorCode::HookTimeout, "pre hook timed out")),
            ..FakeBackend::default()
        };
        let output = dispatch(
            &request(&["new", "feature/a", "--json", "--hook-timeout-ms", "25"]),
            &pre_failure,
        );
        assert_eq!(output.exit_code, 10);
        assert_eq!(error_code(&output), "HOOK_TIMEOUT");
        assert_eq!(*pre_failure.hook_calls.borrow(), [HookPhase::Pre]);
        assert_eq!(pre_failure.plan_calls.get(), 1);
        assert_eq!(pre_failure.apply_calls.get(), 0);
        assert_eq!(pre_failure.state_calls.get(), 0);
        assert_eq!(pre_failure.execute_calls.get(), 0);

        let post_failure = FakeBackend {
            post_error: Some(CliError::new(ErrorCode::HookFailed, "post hook failed")),
            ..FakeBackend::default()
        };
        let output = dispatch(
            &request(&["new", "feature/a", "--json", "--strict-post-hooks"]),
            &post_failure,
        );
        assert_eq!(output.exit_code, 10);
        assert_eq!(error_code(&output), "HOOK_FAILED");
        assert_eq!(
            *post_failure.hook_calls.borrow(),
            [HookPhase::Pre, HookPhase::Post]
        );
        assert_eq!(post_failure.plan_calls.get(), 1);
        assert_eq!(post_failure.apply_calls.get(), 1);
        assert_eq!(post_failure.state_calls.get(), 1);
        assert_eq!(post_failure.execute_calls.get(), 0);
    }

    #[test]
    fn pre_hook_error_remains_primary_when_stage_rollback_also_fails() {
        let backend = FakeBackend {
            pre_error: Some(CliError::new(ErrorCode::HookTimeout, "pre hook timed out")),
            rollback_error: Some(CliError::new(
                ErrorCode::StashApplyFailed,
                "automatic stash restore failed",
            )),
            ..FakeBackend::default()
        };

        let output = dispatch(&request(&["new", "feature/a", "--json"]), &backend);
        let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON output");

        assert_eq!(output.exit_code, 10);
        assert_eq!(value["error"]["code"], "HOOK_TIMEOUT");
        assert_eq!(value["error"]["details"]["autoRestoreFailed"], true);
        assert_eq!(
            value["error"]["details"]["autoRestoreError"]["code"],
            "STASH_APPLY_FAILED"
        );
        assert_eq!(
            *backend.trace.borrow(),
            ["lock", "plan", "stage", "pre-hook", "rollback-stage"]
        );
    }

    #[test]
    fn partial_result_survives_a_strict_post_hook_failure() {
        let backend = FakeBackend {
            post_error: Some(CliError::new(ErrorCode::HookFailed, "post hook failed")),
            execute_result: Ok(CommandOutput::partial(
                json!({"deleted": ["a"], "failed": ["b"]}),
                CliError::new(ErrorCode::GitCommandFailed, "partial mutation failure"),
            )),
            ..FakeBackend::default()
        };

        let output = dispatch(
            &request(&["new", "feature/a", "--json", "--strict-post-hooks"]),
            &backend,
        );
        let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON output");

        assert_eq!(output.exit_code, 20);
        assert_eq!(value["data"]["deleted"], json!(["a"]));
        assert_eq!(value["data"]["failed"], json!(["b"]));
        assert_eq!(value["error"]["code"], "GIT_COMMAND_FAILED");
        assert_eq!(
            value["error"]["details"]["postHookError"]["code"],
            "HOOK_FAILED"
        );
    }

    #[test]
    fn non_strict_post_hook_warning_reaches_human_and_json_stderr() {
        for args in [
            &["new", "feature/a"][..],
            &["new", "feature/a", "--json"][..],
        ] {
            let backend = FakeBackend {
                post_warning: Some("Warning: hook execution failed\n".to_owned()),
                ..FakeBackend::default()
            };

            let output = dispatch(&request(args), &backend);

            assert_eq!(output.exit_code, 0);
            assert_eq!(output.stderr, "Warning: hook execution failed\n");
            if args.contains(&"--json") {
                let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON output");
                assert_eq!(value["status"], "ok");
            }
        }
    }

    #[test]
    fn partial_json_result_preserves_non_strict_post_hook_warning_on_stderr() {
        let backend = FakeBackend {
            post_warning: Some("Warning: hook execution failed\n".to_owned()),
            execute_result: Ok(CommandOutput::partial(
                json!({"deleted": ["a"], "failed": ["b"]}),
                CliError::new(ErrorCode::GitCommandFailed, "partial mutation failure"),
            )),
            ..FakeBackend::default()
        };

        let output = dispatch(&request(&["new", "feature/a", "--json"]), &backend);
        let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON output");

        assert_eq!(output.exit_code, 20);
        assert_eq!(output.stderr, "Warning: hook execution failed\n");
        assert_eq!(value["status"], "error");
        assert_eq!(value["error"]["code"], "GIT_COMMAND_FAILED");
    }

    #[test]
    fn system_backend_classifies_a_real_non_strict_post_hook_failure_as_warning() {
        let directory = tempfile::tempdir().expect("temporary repository");
        let hooks_directory = directory.path().join(".vde/worktree/hooks");
        fs::create_dir_all(&hooks_directory).expect("hook directory");
        let hook_path = hooks_directory.join("post-new");
        fs::write(
            &hook_path,
            "#!/bin/sh\necho real-hook-warning >&2\nexit 7\n",
        )
        .expect("hook script");
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))
            .expect("executable hook");

        let backend = SystemBackend {
            cwd: directory.path().to_path_buf(),
            git: GitCli::new(StdProcessRunner),
            gh: GhCli::new(StdProcessRunner),
            fzf: FzfAdapter::new(StdProcessRunner),
            terminal: TerminalCapabilities {
                stdout_tty: false,
                stderr_tty: false,
                stdout_columns: None,
                no_color: false,
            },
            home: None,
            in_tmux: false,
        };
        let contexts = MutationHookContexts::new(
            directory.path().to_path_buf(),
            "new",
            Some("feature/a".to_owned()),
            Some(directory.path().to_path_buf()),
            directory.path().to_path_buf(),
            directory.path().to_path_buf(),
            false,
            BTreeMap::new(),
        );
        let result = backend
            .run_hook(
                HookPhase::Post,
                &request(&["new", "feature/a"]),
                contexts.for_phase(HookPhase::Post),
                Duration::from_secs(1),
            )
            .expect("non-strict post-hook failure is a warning");

        let ApplicationHookResult::Warning(warning) = result else {
            panic!("expected warning")
        };
        assert_eq!(warning.code, ErrorCode::HookFailed);
        assert_eq!(warning.details["hook"], "post-new");
        assert_eq!(warning.details["phase"], "post");
        assert!(warning.details["logPath"].as_str().is_some());
    }

    #[test]
    fn fake_git_adapter_failure_reaches_dispatch_exit_and_json_details() {
        let backend = FailingGitBackend::default();
        let output = dispatch(&request(&["list", "--json"]), &backend);
        let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON output");

        assert_eq!(output.exit_code, 20);
        assert_eq!(value["error"]["code"], "GIT_COMMAND_FAILED");
        assert_eq!(
            value["error"]["details"]["argv"],
            json!(["status", "--porcelain"])
        );
        assert_eq!(value["error"]["details"]["exitCode"], 128);
        assert_eq!(
            value["error"]["details"]["stderr"],
            "fatal: fake adapter failure"
        );
    }
}
