use std::collections::BTreeMap;
#[cfg(unix)]
use std::collections::BTreeSet;
use std::env;
#[cfg(unix)]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::fd::OwnedFd;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
#[cfg(not(unix))]
use tempfile::{Builder as TempBuilder, TempDir};

#[cfg(unix)]
use nix::dir::Dir;
#[cfg(unix)]
use nix::fcntl::{AtFlags, OFlag, open, openat, renameat};
#[cfg(unix)]
use nix::sys::stat::{FileStat, Mode, SFlag, fchmod, fstat, fstatat, mkdirat};
#[cfg(unix)]
use nix::unistd::{UnlinkatFlags, symlinkat, unlinkat};
#[cfg(all(
    unix,
    any(
        target_vendor = "apple",
        target_os = "linux",
        target_os = "android",
        target_os = "redox"
    )
))]
use rustix::fs::{RenameFlags, renameat_with};

use crate::adapters::git_cli::GitCli;
use crate::app::error_mapper::{MapToCliError, map_hook_report, map_transaction_error};
use crate::app::snapshot::parse_worktree_porcelain;
use crate::app::target;
use crate::cli::{Command, CompletionCandidateKind, ParsedRequest};
use crate::domain::error::{CliError, ErrorCode, ExecutionPhase, ExecutionState};
use crate::domain::repo::RepoContext;
use crate::domain::worktree::GitWorktree;
use crate::ports::process::{OutputPolicy, ProcessCommand, ProcessRunner, StdinPolicy};
use crate::state::config::ResolvedConfig;
use crate::state::hooks::{
    HookContext, HookDisposition, HookPhase, SystemHookProcessRunner, run_hook,
};

#[derive(Debug, Clone, PartialEq)]
pub struct MiscCommandOutput {
    pub data: Value,
    pub human_stdout: String,
    pub human_stderr: String,
    pub partial_error: Option<CliError>,
}

impl MiscCommandOutput {
    fn success(data: Value) -> Self {
        Self {
            data,
            human_stdout: String::new(),
            human_stderr: String::new(),
            partial_error: None,
        }
    }

    fn partial(data: Value, error: CliError) -> Self {
        Self {
            data,
            human_stdout: String::new(),
            human_stderr: String::new(),
            partial_error: Some(error),
        }
    }
}

pub fn execute_misc_command<R>(
    request: &ParsedRequest,
    context: &RepoContext,
    config: &ResolvedConfig,
    git: &GitCli<R>,
    process_runner: &dyn ProcessRunner,
    terminal_is_tty: bool,
) -> Option<Result<MiscCommandOutput, CliError>>
where
    R: ProcessRunner,
{
    let result = match &request.command {
        Command::Exec { branch, argv, .. } => execute_in_worktree(
            request,
            context,
            git,
            process_runner,
            branch.as_deref(),
            argv,
        ),
        Command::Invoke { hook, argv } => {
            invoke_hook(request, context, config, git, hook, argv, terminal_is_tty)
        }
        Command::Copy { paths } => copy_or_link(request, context, git, paths, FilePlacement::Copy),
        Command::Link { paths } => copy_or_link(request, context, git, paths, FilePlacement::Link),
        Command::CompletionCandidates { kind, .. } => {
            completion_candidates(context, config, git, *kind)
        }
        _ => return None,
    };
    Some(result)
}

fn execute_in_worktree<R: ProcessRunner>(
    request: &ParsedRequest,
    context: &RepoContext,
    git: &GitCli<R>,
    process_runner: &dyn ProcessRunner,
    branch: Option<&str>,
    argv: &[OsString],
) -> Result<MiscCommandOutput, CliError> {
    let worktrees = list_worktrees(git, &context.repo_root)?;
    let target = target::resolve(
        &worktrees,
        branch,
        request.common.worktree.as_deref(),
        &context.current_worktree_root,
    )?;
    let target_path = target::ensure_path(&target.path)?;
    let branch = target.branch.as_deref();
    let (program, child_arguments) = argv.split_first().ok_or_else(|| {
        CliError::new(
            ErrorCode::InvalidArgument,
            "exec requires an executable after --",
        )
    })?;
    let Command::Exec { options, .. } = &request.command else {
        unreachable!("exec options")
    };
    let mut command = ProcessCommand::new(program);
    command.args = child_arguments.to_vec();
    command.cwd = Some(target.path.clone());
    command.timeout = Some(Duration::from_millis(options.timeout_ms));
    command.max_output_bytes = usize::try_from(options.max_output_bytes.unwrap_or(1024 * 1024))
        .map_err(|_| {
            CliError::new(
                ErrorCode::InvalidArgument,
                "max-output-bytes exceeds the supported size",
            )
        })?;
    command.stdin = match options.stdin {
        crate::cli::ExecStdin::Null => StdinPolicy::Null,
        crate::cli::ExecStdin::Inherit => StdinPolicy::Inherit,
    };
    command.stdout = if request.common.json {
        OutputPolicy::Capture
    } else {
        OutputPolicy::Inherit
    };
    command.stderr = command.stdout;
    let child = process_runner.run(&command).map_err(|error| {
        CliError::new(
            ErrorCode::ChildProcessFailed,
            format!("failed to execute target command: {error}"),
        )
        .with_details(BTreeMap::from([
            ("branch".to_owned(), json!(branch)),
            ("path".to_owned(), json!(target_path)),
        ]))
        .at_phase(ExecutionPhase::Process, ExecutionState::Unknown, &[])
    })?;
    let child_exit_code = child.exit_code;
    let data = json!({
        "branch": branch,
        "path": target_path,
        "childExitCode": child_exit_code,
        "childSignal": child.signal,
        "timedOut": child.timed_out,
        "stdoutTruncated": child.stdout_truncated,
        "stderrTruncated": child.stderr_truncated,
        "childStdout": String::from_utf8_lossy(&child.stdout),
        "childStderr": String::from_utf8_lossy(&child.stderr),
    });
    if child_exit_code == Some(0) && !child.timed_out {
        return Ok(MiscCommandOutput::success(data));
    }
    let error = CliError::new(
        ErrorCode::ChildProcessFailed,
        if child.timed_out {
            "target command exceeded its timeout"
        } else if child.signal.is_some() {
            "target command terminated by signal"
        } else {
            "target command exited with non-zero status"
        },
    )
    .with_details(BTreeMap::from([
        ("branch".to_owned(), json!(branch)),
        ("path".to_owned(), json!(target_path)),
        ("childExitCode".to_owned(), json!(child_exit_code)),
        ("childSignal".to_owned(), json!(child.signal)),
        ("timedOut".to_owned(), json!(child.timed_out)),
        ("stdoutTruncated".to_owned(), json!(child.stdout_truncated)),
        ("stderrTruncated".to_owned(), json!(child.stderr_truncated)),
    ]))
    .at_phase(ExecutionPhase::Process, ExecutionState::Unknown, &["spawn"]);
    Ok(MiscCommandOutput::partial(data, error))
}

fn invoke_hook<R: ProcessRunner>(
    request: &ParsedRequest,
    context: &RepoContext,
    config: &ResolvedConfig,
    git: &GitCli<R>,
    hook: &crate::domain::hook::HookName,
    argv: &[OsString],
    terminal_is_tty: bool,
) -> Result<MiscCommandOutput, CliError> {
    let worktrees = list_worktrees(git, &context.repo_root)?;
    let current = worktrees
        .iter()
        .find(|worktree| worktree.path == context.current_worktree_root)
        .ok_or_else(|| {
            CliError::new(
                ErrorCode::WorktreeNotFound,
                "current worktree is not present in Git worktree metadata",
            )
            .with_details(BTreeMap::from([(
                "path".to_owned(),
                json!(context.current_worktree_root),
            )]))
        })?;
    let hook_arguments = argv
        .iter()
        .map(|argument| {
            argument.to_str().map(ToOwned::to_owned).ok_or_else(|| {
                CliError::new(
                    ErrorCode::InvalidArgument,
                    "invoke arguments must be valid UTF-8",
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut hook_context = HookContext::new(
        context.repo_root.clone(),
        format!("invoke:{}", hook.as_str()),
    );
    hook_context.branch.clone_from(&current.branch);
    hook_context.worktree_path = Some(current.path.clone());
    hook_context.execution_cwd = Some(current.path.clone());
    hook_context.is_tty = terminal_is_tty;
    hook_context.timeout = Duration::from_millis(
        request
            .common
            .hook_timeout_ms
            .unwrap_or(config.hooks.timeout_ms),
    );
    let phase = if hook.as_str().starts_with("pre-") {
        HookPhase::Pre
    } else {
        HookPhase::Post
    };
    let report = run_hook(
        phase,
        hook,
        &hook_arguments,
        &hook_context,
        &SystemHookProcessRunner,
        true,
        true,
    )
    .map_err(MapToCliError::map_to_cli_error)?;
    if report.disposition == HookDisposition::Fatal {
        return Err(map_hook_report(&report));
    }
    Ok(MiscCommandOutput::success(json!({ "hook": hook.as_str() })))
}

#[derive(Clone, Copy)]
enum FilePlacement {
    Copy,
    Link,
}

/// Copies repository-relative files into a freshly-created worktree without replacing files
/// that already exist there. The batch uses the same transactional placement path as `vw copy`.
pub(crate) fn copy_worktree_include_paths(
    repo_root: &Path,
    target_root: &Path,
    paths: &[PathBuf],
) -> Result<(), CliError> {
    let plans = paths
        .iter()
        .filter_map(|path| {
            PlacementPlan::validate_if_destination_absent(repo_root, target_root, path).transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    if plans.is_empty() {
        return Ok(());
    }
    validate_disjoint_destinations(&plans)?;
    match execute_placement_batch(&plans, FilePlacement::Copy, &NoopPlacementObserver) {
        Ok(None) => Ok(()),
        Ok(Some(cleanup_error)) | Err(cleanup_error) => Err(cleanup_error),
    }
}

fn copy_or_link<R: ProcessRunner>(
    request: &ParsedRequest,
    context: &RepoContext,
    git: &GitCli<R>,
    paths: &[PathBuf],
    placement: FilePlacement,
) -> Result<MiscCommandOutput, CliError> {
    let worktrees = list_worktrees(git, &context.repo_root)?;
    let target_root = resolve_file_target(request, context, &worktrees)?;
    let plans = paths
        .iter()
        .map(|path| PlacementPlan::validate(&context.repo_root, &target_root, path))
        .collect::<Result<Vec<_>, _>>()?;
    validate_disjoint_destinations(&plans)?;
    let values = paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let data = match placement {
        FilePlacement::Copy => json!({ "copied": values, "worktreePath": target_root }),
        FilePlacement::Link => json!({ "linked": values, "worktreePath": target_root }),
    };
    match execute_placement_batch(&plans, placement, &NoopPlacementObserver) {
        Ok(None) => Ok(MiscCommandOutput::success(data)),
        Ok(Some(cleanup_error)) => Ok(MiscCommandOutput::partial(data, cleanup_error)),
        Err(error) if error.details.get("recoveryRequired") == Some(&json!(true)) => {
            Ok(MiscCommandOutput::partial(
                json!({
                    "attempted": values,
                    "worktreePath": target_root,
                    "transactionState": "recovery-required",
                }),
                error,
            ))
        }
        Err(error) => Err(error),
    }
}

fn resolve_file_target(
    request: &ParsedRequest,
    context: &RepoContext,
    worktrees: &[GitWorktree],
) -> Result<PathBuf, CliError> {
    let env_target = env::var_os("WT_WORKTREE_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let explicit = request.common.worktree.as_ref().or(env_target.as_ref());
    let cwd = request
        .common
        .directory
        .as_deref()
        .unwrap_or(&context.current_worktree_root);
    let absolute = explicit.map(|path| cwd.join(path));
    let target = target::resolve(
        worktrees,
        None,
        absolute.as_deref(),
        &context.current_worktree_root,
    )?;
    target::ensure_path(&target.path)?;
    Ok(target.path.clone())
}

#[derive(Debug)]
struct PlacementPlan {
    repo_root: PathBuf,
    target_root: PathBuf,
    relative: PathBuf,
    source_guard: Vec<SourceEntryIdentity>,
    destination_guard: DestinationGuard,
}

impl PlacementPlan {
    fn validate(repo_root: &Path, target_root: &Path, relative: &Path) -> Result<Self, CliError> {
        validate_relative(relative)?;
        let repo_root = canonicalize(repo_root, ErrorCode::PathOutsideRepo)?;
        let target_root = canonicalize(target_root, ErrorCode::PathOutsideRepo)?;
        let source = repo_root.join(relative);
        let destination = target_root.join(relative);
        validate_source(&source, &repo_root, &target_root)?;
        reject_same_source_and_destination(&source, &destination)?;
        let source_guard = capture_source_tree(&source, &repo_root)?;
        let destination_guard = DestinationGuard::capture(&destination, &target_root)?;
        Ok(Self {
            repo_root,
            target_root,
            relative: relative.to_path_buf(),
            source_guard,
            destination_guard,
        })
    }

    fn validate_if_destination_absent(
        repo_root: &Path,
        target_root: &Path,
        relative: &Path,
    ) -> Result<Option<Self>, CliError> {
        validate_relative(relative)?;
        let repo_root = canonicalize(repo_root, ErrorCode::PathOutsideRepo)?;
        let target_root = canonicalize(target_root, ErrorCode::PathOutsideRepo)?;
        let source = repo_root.join(relative);
        let destination = target_root.join(relative);
        validate_source(&source, &repo_root, &target_root)?;
        reject_same_source_and_destination(&source, &destination)?;
        let source_guard = capture_source_tree(&source, &repo_root)?;
        if destination_has_collision(&target_root, relative)? {
            return Ok(None);
        }
        let destination_guard = match DestinationGuard::capture(&destination, &target_root) {
            Ok(guard) => guard,
            Err(_) if destination_has_collision(&target_root, relative)? => return Ok(None),
            Err(error) => return Err(error),
        };
        if destination_guard.destination_identity.is_some() {
            return Ok(None);
        }
        Ok(Some(Self {
            repo_root,
            target_root,
            relative: relative.to_path_buf(),
            source_guard,
            destination_guard,
        }))
    }

    fn revalidate_source(&self) -> Result<PathBuf, CliError> {
        let source = self.repo_root.join(&self.relative);
        validate_source(&source, &self.repo_root, &self.target_root)?;
        let current = capture_source_tree(&source, &self.repo_root)?;
        if current != self.source_guard {
            return Err(path_changed(
                "copy/link source changed after validation",
                &source,
            ));
        }
        Ok(source)
    }

    fn revalidate_destination(&self) -> Result<PathBuf, CliError> {
        let destination = self.target_root.join(&self.relative);
        self.destination_guard
            .revalidate(&destination, &self.target_root)?;
        Ok(destination)
    }

    #[cfg(test)]
    fn copy(&self) -> Result<(), CliError> {
        execute_placement_batch(
            std::slice::from_ref(self),
            FilePlacement::Copy,
            &NoopPlacementObserver,
        )?
        .map_or(Ok(()), Err)
    }

    #[cfg(test)]
    fn link(&self) -> Result<(), CliError> {
        execute_placement_batch(
            std::slice::from_ref(self),
            FilePlacement::Link,
            &NoopPlacementObserver,
        )?
        .map_or(Ok(()), Err)
    }
}

fn destination_has_collision(target_root: &Path, relative: &Path) -> Result<bool, CliError> {
    let mut current = target_root.to_path_buf();
    let component_count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                let is_destination = index + 1 == component_count;
                if is_destination || !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Ok(true);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(error.kind() == std::io::ErrorKind::NotADirectory);
            }
            Err(error) => {
                return Err(io_error(
                    ErrorCode::PathOutsideRepo,
                    "failed to inspect .worktreeinclude destination",
                    &current,
                    error,
                ));
            }
        }
    }
    Ok(false)
}

fn reject_same_source_and_destination(source: &Path, destination: &Path) -> Result<(), CliError> {
    let same_lexical_path = source == destination;
    let destination_is_symlink =
        fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.file_type().is_symlink());
    let same_canonical_path = !destination_is_symlink
        && fs::canonicalize(source)
            .ok()
            .zip(fs::canonicalize(destination).ok())
            .is_some_and(|(source, destination)| source == destination);
    let same_identity = !destination_is_symlink
        && match (
            MetadataIdentity::capture(source, true),
            MetadataIdentity::capture(destination, true),
        ) {
            (Ok(source), Ok(destination)) => source.same_object(&destination),
            _ => false,
        };
    if same_lexical_path || same_canonical_path || same_identity {
        return Err(CliError::new(
            ErrorCode::InvalidArgument,
            "copy/link source and destination must be different filesystem objects",
        )
        .with_details(BTreeMap::from([
            ("source".to_owned(), json!(source)),
            ("destination".to_owned(), json!(destination)),
        ])));
    }
    Ok(())
}

fn validate_source(source: &Path, repo_root: &Path, target_root: &Path) -> Result<(), CliError> {
    let resolved = canonicalize(source, ErrorCode::PathOutsideRepo)?;
    if resolved == repo_root || !resolved.starts_with(repo_root) {
        return Err(path_outside("source path escapes repository root", source));
    }
    validate_tree_symlinks(source, repo_root)?;
    if source.is_dir() && target_root.starts_with(&resolved) {
        return Err(CliError::new(
            ErrorCode::PathOutsideRepo,
            "copy source directory cannot contain the target worktree",
        )
        .with_details(BTreeMap::from([
            ("source".to_owned(), json!(source)),
            ("targetRoot".to_owned(), json!(target_root)),
        ])));
    }
    Ok(())
}

fn validate_disjoint_destinations(plans: &[PlacementPlan]) -> Result<(), CliError> {
    for (index, plan) in plans.iter().enumerate() {
        for other in &plans[index + 1..] {
            if plan.relative == other.relative
                || plan.relative.starts_with(&other.relative)
                || other.relative.starts_with(&plan.relative)
            {
                return Err(CliError::new(
                    ErrorCode::InvalidArgument,
                    "copy/link paths must not duplicate or contain one another",
                )
                .with_details(BTreeMap::from([
                    ("path".to_owned(), json!(plan.relative)),
                    ("conflictingPath".to_owned(), json!(other.relative)),
                ])));
            }
        }
    }
    Ok(())
}

trait PlacementObserver {
    fn after_source_validation(&self, _index: usize, _plan: &PlacementPlan) {}
    #[cfg(unix)]
    fn after_destination_validation(&self, _index: usize, _plan: &PlacementPlan) {}
    #[cfg(unix)]
    fn before_commit(&self, _index: usize, _plan: &PlacementPlan) -> Result<(), CliError> {
        Ok(())
    }
    #[cfg(unix)]
    fn before_destination_move(&self, _index: usize, _plan: &PlacementPlan) {}
    #[cfg(unix)]
    fn after_destination_backup(&self, _index: usize, _plan: &PlacementPlan) {}
    #[cfg(unix)]
    fn before_source_open(&self, _relative: &Path) {}
    #[cfg(unix)]
    fn after_source_open(&self, _relative: &Path) {}
    #[cfg(unix)]
    fn before_rollback(&self, _index: usize) -> Result<(), String> {
        Ok(())
    }
    #[cfg(unix)]
    fn before_transaction_cleanup(&self, _target_root: &Path) {}
}

struct NoopPlacementObserver;

impl PlacementObserver for NoopPlacementObserver {}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlacementInitializationPoint {
    DirectoryCreated,
    DirectoryOpened,
    IdentityVerified,
    StagedCreated,
    BackupCreated,
    StagedOpened,
    BackupOpened,
}

#[cfg(unix)]
trait PlacementInitializationObserver {
    fn checkpoint(&self, _point: PlacementInitializationPoint) -> Result<(), CliError> {
        Ok(())
    }
}

#[cfg(unix)]
struct NoopPlacementInitializationObserver;

#[cfg(unix)]
impl PlacementInitializationObserver for NoopPlacementInitializationObserver {}

#[cfg(unix)]
struct PlacementInitializationGuard {
    parent_fd: Option<OwnedFd>,
    name: OsString,
    directory_fd: Option<OwnedFd>,
    identity: Option<MetadataIdentity>,
    armed: bool,
}

#[cfg(unix)]
impl PlacementInitializationGuard {
    fn new(parent_fd: OwnedFd, name: OsString) -> Self {
        Self {
            parent_fd: Some(parent_fd),
            name,
            directory_fd: None,
            identity: None,
            armed: true,
        }
    }

    fn parent_fd(&self) -> &OwnedFd {
        self.parent_fd
            .as_ref()
            .expect("initialization parent exists")
    }

    fn directory_fd(&self) -> &OwnedFd {
        self.directory_fd
            .as_ref()
            .expect("initialization directory exists")
    }

    fn set_directory_fd(&mut self, fd: OwnedFd) {
        self.directory_fd = Some(fd);
    }

    fn set_identity(&mut self, identity: MetadataIdentity) {
        self.identity = Some(identity);
    }

    fn cleanup(&self) -> Result<(), std::io::Error> {
        let named = fstatat(
            self.parent_fd(),
            self.name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map(|stat| metadata_identity_from_stat(&stat))
        .map_err(errno_io)?;
        let expected = self.identity.as_ref().unwrap_or(&named);
        if !named.same_object(expected) {
            return Err(std::io::Error::other(
                "placement initialization entry changed; replacement was not removed",
            ));
        }
        let fallback_fd;
        let directory_fd = if let Some(directory_fd) = &self.directory_fd {
            directory_fd
        } else {
            fallback_fd = openat(
                self.parent_fd(),
                self.name.as_os_str(),
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(errno_io)?;
            &fallback_fd
        };
        let opened = metadata_identity_from_stat(&fstat(directory_fd).map_err(errno_io)?);
        if !opened.same_object(expected) {
            return Err(std::io::Error::other(
                "opened placement initialization directory changed",
            ));
        }
        clear_directory_fd(directory_fd)?;
        let named = fstatat(
            self.parent_fd(),
            self.name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map(|stat| metadata_identity_from_stat(&stat))
        .map_err(errno_io)?;
        if !named.same_object(expected) {
            return Err(std::io::Error::other(
                "placement initialization entry changed during cleanup",
            ));
        }
        unlinkat(
            self.parent_fd(),
            self.name.as_os_str(),
            UnlinkatFlags::RemoveDir,
        )
        .map_err(errno_io)
    }

    fn abort(mut self) -> Result<(), std::io::Error> {
        let result = self.cleanup();
        self.armed = false;
        result
    }

    fn finish(mut self) -> (OwnedFd, OsString, OwnedFd, MetadataIdentity) {
        self.armed = false;
        (
            self.parent_fd.take().expect("initialization parent exists"),
            self.name.clone(),
            self.directory_fd
                .take()
                .expect("initialization directory exists"),
            self.identity
                .take()
                .expect("placement initialization identity exists"),
        )
    }
}

#[cfg(unix)]
impl Drop for PlacementInitializationGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.cleanup();
        }
    }
}

struct PlacementTransaction {
    root_identity: MetadataIdentity,
    #[cfg(unix)]
    name: OsString,
    #[cfg(unix)]
    target_fd: OwnedFd,
    #[cfg(unix)]
    transaction_fd: OwnedFd,
    #[cfg(unix)]
    transaction_identity: MetadataIdentity,
    #[cfg(unix)]
    staged_fd: OwnedFd,
    #[cfg(unix)]
    backup_fd: OwnedFd,
    #[cfg(unix)]
    preserve: bool,
    #[cfg(not(unix))]
    directory: TempDir,
}

#[cfg(unix)]
impl PlacementTransaction {
    fn create(target_root: &Path) -> Result<Self, CliError> {
        Self::create_with_observer(target_root, &NoopPlacementInitializationObserver)
    }

    #[allow(clippy::too_many_lines)]
    fn create_with_observer(
        target_root: &Path,
        observer: &dyn PlacementInitializationObserver,
    ) -> Result<Self, CliError> {
        let root_identity = MetadataIdentity::capture(target_root, true)?;
        let target_fd = open_directory_handle(target_root)?;
        let target_identity = metadata_identity_from_stat(
            &fstat(&target_fd)
                .map_err(|error| nix_io("inspect opened target root", target_root, error))?,
        );
        if !target_identity.same_object(&root_identity) {
            return Err(path_changed(
                "target worktree changed while its directory handle was opened",
                target_root,
            ));
        }
        let name = create_transaction_directory(&target_fd, ".vde-placement-", target_root)?;
        let mut guard = PlacementInitializationGuard::new(target_fd, name);
        let initialization = (|| {
            let transaction_identity = metadata_identity_from_stat(
                &fstatat(
                    guard.parent_fd(),
                    guard.name.as_os_str(),
                    AtFlags::AT_SYMLINK_NOFOLLOW,
                )
                .map_err(|error| {
                    nix_io("inspect created placement transaction", target_root, error)
                })?,
            );
            guard.set_identity(transaction_identity);
            observer.checkpoint(PlacementInitializationPoint::DirectoryCreated)?;
            let transaction_fd = openat(
                guard.parent_fd(),
                guard.name.as_os_str(),
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| {
                nix_io(
                    "open placement transaction through target handle",
                    target_root,
                    error,
                )
            })?;
            guard.set_directory_fd(transaction_fd);
            observer.checkpoint(PlacementInitializationPoint::DirectoryOpened)?;
            let opened_identity =
                metadata_identity_from_stat(&fstat(guard.directory_fd()).map_err(|error| {
                    nix_io("inspect placement transaction handle", target_root, error)
                })?);
            if !opened_identity.same_object(
                guard
                    .identity
                    .as_ref()
                    .expect("placement initialization identity exists"),
            ) {
                return Err(path_changed(
                    "placement transaction changed while its handle was opened",
                    target_root,
                ));
            }
            observer.checkpoint(PlacementInitializationPoint::IdentityVerified)?;
            mkdirat(
                guard.directory_fd(),
                "staged",
                Mode::from_bits_truncate(0o700),
            )
            .map_err(|error| {
                nix_io(
                    "create placement staging through transaction handle",
                    target_root,
                    error,
                )
            })?;
            observer.checkpoint(PlacementInitializationPoint::StagedCreated)?;
            mkdirat(
                guard.directory_fd(),
                "backup",
                Mode::from_bits_truncate(0o700),
            )
            .map_err(|error| {
                nix_io(
                    "create placement backup through transaction handle",
                    target_root,
                    error,
                )
            })?;
            observer.checkpoint(PlacementInitializationPoint::BackupCreated)?;
            let staged_fd = openat(
                guard.directory_fd(),
                "staged",
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| nix_io("open placement staging handle", target_root, error))?;
            observer.checkpoint(PlacementInitializationPoint::StagedOpened)?;
            let backup_fd = openat(
                guard.directory_fd(),
                "backup",
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| nix_io("open placement backup handle", target_root, error))?;
            observer.checkpoint(PlacementInitializationPoint::BackupOpened)?;
            Ok::<_, CliError>((staged_fd, backup_fd))
        })();
        let (staged_fd, backup_fd) = match initialization {
            Ok(handles) => handles,
            Err(mut error) => {
                attach_transaction_cleanup(&mut error, guard.abort());
                return Err(error);
            }
        };
        let (target_fd, name, transaction_fd, transaction_identity) = guard.finish();
        let transaction = Self {
            root_identity,
            name,
            target_fd,
            transaction_fd,
            transaction_identity,
            staged_fd,
            backup_fd,
            preserve: false,
        };
        transaction.revalidate_root(target_root)?;
        Ok(transaction)
    }

    fn resolve_path(&self, target_root: &Path) -> Result<PathBuf, CliError> {
        self.revalidate_root(target_root)?;
        let name = find_entry_name_by_identity(&self.target_fd, &self.transaction_identity)
            .map_err(|error| {
                io_error(
                    ErrorCode::InternalError,
                    "locate placement transaction by identity",
                    target_root,
                    error,
                )
            })?
            .ok_or_else(|| {
                path_changed(
                    "placement transaction is no longer reachable from the target root",
                    target_root,
                )
            })?;
        Ok(target_root.join(name))
    }

    fn revalidate_root(&self, target_root: &Path) -> Result<(), CliError> {
        let current = MetadataIdentity::capture(target_root, true)?;
        let held = metadata_identity_from_stat(
            &fstat(&self.target_fd)
                .map_err(|error| nix_io("inspect held target root", target_root, error))?,
        );
        if !current.same_object(&self.root_identity) || !held.same_object(&self.root_identity) {
            return Err(path_changed(
                "target worktree changed during placement transaction",
                target_root,
            ));
        }
        Ok(())
    }

    fn cleanup(&self) -> Result<(), std::io::Error> {
        let named = fstatat(
            &self.target_fd,
            self.name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map(|stat| metadata_identity_from_stat(&stat))
        .map_err(errno_io)?;
        if !named.same_object(&self.transaction_identity) {
            return Err(std::io::Error::other(
                "placement transaction name changed; replacement was not removed",
            ));
        }
        clear_directory_fd(&self.transaction_fd)?;
        let named = fstatat(
            &self.target_fd,
            self.name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map(|stat| metadata_identity_from_stat(&stat))
        .map_err(errno_io)?;
        if !named.same_object(&self.transaction_identity) {
            return Err(std::io::Error::other(
                "placement transaction name changed during cleanup; replacement was not removed",
            ));
        }
        unlinkat(
            &self.target_fd,
            self.name.as_os_str(),
            UnlinkatFlags::RemoveDir,
        )
        .map_err(errno_io)
    }

    fn close(mut self) -> Result<(), std::io::Error> {
        let result = self.cleanup();
        self.preserve = true;
        result
    }

    fn keep(mut self, target_root: &Path) -> Result<PathBuf, CliError> {
        let result = self.resolve_path(target_root);
        self.preserve = true;
        result
    }
}

#[cfg(unix)]
impl Drop for PlacementTransaction {
    fn drop(&mut self) {
        if !self.preserve {
            let _ = self.cleanup();
        }
    }
}

#[cfg(not(unix))]
impl PlacementTransaction {
    fn create(target_root: &Path) -> Result<Self, CliError> {
        let directory = TempBuilder::new()
            .prefix(".vde-placement-")
            .tempdir_in(target_root)
            .map_err(|error| {
                io_error(
                    ErrorCode::InternalError,
                    "create placement transaction",
                    target_root,
                    error,
                )
            })?;
        Ok(Self {
            root_identity: MetadataIdentity::capture(target_root, true)?,
            directory,
        })
    }

    fn staged_root(&self) -> PathBuf {
        self.directory.path().join("staged")
    }

    fn display_path(&self, target_root: &Path) -> PathBuf {
        target_root.join(
            self.directory
                .path()
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new(".vde-placement-unknown")),
        )
    }

    fn resolve_path(&self, target_root: &Path) -> Result<PathBuf, CliError> {
        self.revalidate_root(target_root)?;
        Ok(self.display_path(target_root))
    }

    fn revalidate_root(&self, target_root: &Path) -> Result<(), CliError> {
        let current = MetadataIdentity::capture(target_root, true)?;
        if current.same_object(&self.root_identity) {
            Ok(())
        } else {
            Err(path_changed("target worktree changed", target_root))
        }
    }

    fn close(self) -> Result<(), std::io::Error> {
        self.directory.close()
    }

    fn keep(self, target_root: &Path) -> Result<PathBuf, CliError> {
        self.revalidate_root(target_root)?;
        Ok(self.directory.keep())
    }
}

#[cfg(unix)]
fn open_directory_handle(path: &Path) -> Result<OwnedFd, CliError> {
    open(
        path,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_io("open directory handle", path, error))
}

#[cfg(unix)]
static TRANSACTION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
fn create_transaction_directory(
    parent: &OwnedFd,
    prefix: &str,
    display_parent: &Path,
) -> Result<OsString, CliError> {
    for _ in 0..128 {
        let counter = TRANSACTION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = OsString::from(format!(
            "{prefix}{:x}-{nanos:x}-{counter:x}",
            std::process::id()
        ));
        match mkdirat(parent, name.as_os_str(), Mode::from_bits_truncate(0o700)) {
            Ok(()) => return Ok(name),
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => {
                return Err(nix_io(
                    "create transaction directory through held parent",
                    display_parent,
                    error,
                ));
            }
        }
    }
    Err(CliError::new(
        ErrorCode::InternalError,
        "failed to allocate a unique transaction directory",
    ))
}

#[cfg(unix)]
fn clear_directory_fd(directory: &OwnedFd) -> Result<(), std::io::Error> {
    let duplicate = openat(
        directory,
        Path::new("."),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(errno_io)?;
    let mut entries = Dir::from_fd(duplicate).map_err(errno_io)?;
    let names = entries
        .iter()
        .map(|entry| {
            entry
                .map(|entry| OsString::from_vec(entry.file_name().to_bytes().to_vec()))
                .map_err(errno_io)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for name in names {
        if name == "." || name == ".." {
            continue;
        }
        let stat =
            fstatat(directory, name.as_os_str(), AtFlags::AT_SYMLINK_NOFOLLOW).map_err(errno_io)?;
        let kind = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
        if kind == SFlag::S_IFDIR {
            let child = openat(
                directory,
                name.as_os_str(),
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(errno_io)?;
            clear_directory_fd(&child)?;
            unlinkat(directory, name.as_os_str(), UnlinkatFlags::RemoveDir).map_err(errno_io)?;
        } else {
            unlinkat(directory, name.as_os_str(), UnlinkatFlags::NoRemoveDir).map_err(errno_io)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn find_entry_name_by_identity(
    directory: &OwnedFd,
    identity: &MetadataIdentity,
) -> Result<Option<OsString>, std::io::Error> {
    let duplicate = openat(
        directory,
        Path::new("."),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(errno_io)?;
    let mut entries = Dir::from_fd(duplicate).map_err(errno_io)?;
    for entry in entries.iter() {
        let entry = entry.map_err(errno_io)?;
        let name = OsString::from_vec(entry.file_name().to_bytes().to_vec());
        if name == "." || name == ".." {
            continue;
        }
        let stat =
            fstatat(directory, name.as_os_str(), AtFlags::AT_SYMLINK_NOFOLLOW).map_err(errno_io)?;
        if metadata_identity_from_stat(&stat).same_object(identity) {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn errno_io(error: nix::errno::Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(error as i32)
}

#[derive(Debug)]
struct StagedPlacement;

#[cfg(unix)]
#[derive(Debug)]
struct CommittedPlacement {
    handle: DestinationHandle,
    index: usize,
    had_backup: bool,
    installed_identity: MetadataIdentity,
}

#[cfg(unix)]
#[derive(Debug)]
struct DestinationHandle {
    parent_fd: OwnedFd,
    parent_path: PathBuf,
    parent_identity: MetadataIdentity,
    name: OsString,
}

fn execute_placement_batch(
    plans: &[PlacementPlan],
    placement: FilePlacement,
    observer: &impl PlacementObserver,
) -> Result<Option<CliError>, CliError> {
    execute_placement_batch_inner(plans, placement, observer)
        .map(|error| error.map(|error| map_transaction_error(error, ExecutionPhase::Finalize)))
        .map_err(|error| map_transaction_error(error, ExecutionPhase::Apply))
}

fn execute_placement_batch_inner(
    plans: &[PlacementPlan],
    placement: FilePlacement,
    observer: &dyn PlacementObserver,
) -> Result<Option<CliError>, CliError> {
    let target_root = plans
        .first()
        .map(|plan| plan.target_root.as_path())
        .ok_or_else(|| CliError::new(ErrorCode::InvalidArgument, "copy/link requires a path"))?;
    let transaction = PlacementTransaction::create(target_root)?;
    let staged = match stage_all(plans, placement, &transaction, observer) {
        Ok(staged) => staged,
        Err(mut error) => {
            attach_transaction_cleanup(&mut error, transaction.close());
            return Err(error);
        }
    };
    let revalidation = plans.iter().try_for_each(|plan| {
        let _ = plan.revalidate_source()?;
        let _ = plan.revalidate_destination()?;
        Ok::<(), CliError>(())
    });
    if let Err(mut error) = revalidation {
        attach_transaction_cleanup(&mut error, transaction.close());
        return Err(error);
    }
    if let Err(mut error) = commit_all(plans, &staged, &transaction, observer) {
        if error.details.get("rollbackFailed") == Some(&json!(true)) {
            error
                .details
                .insert("recoveryRequired".to_owned(), json!(true));
            match transaction.keep(target_root) {
                Ok(recovery_path) => {
                    error
                        .details
                        .insert("recoveryPath".to_owned(), json!(&recovery_path));
                    error
                        .details
                        .insert("backupPath".to_owned(), json!(recovery_path.join("backup")));
                    error
                        .details
                        .insert("stagedPath".to_owned(), json!(recovery_path.join("staged")));
                }
                Err(recovery_error) => {
                    error
                        .details
                        .insert("recoveryPathUnavailable".to_owned(), json!(true));
                    error.details.insert(
                        "recoveryPathError".to_owned(),
                        json!(recovery_error.message),
                    );
                }
            }
            error.details.insert("phase".to_owned(), json!("rollback"));
            error
                .details
                .insert("committedState".to_owned(), json!("partial"));
            error.details.insert("committed".to_owned(), json!(true));
        } else {
            error.execution.state = ExecutionState::RolledBack;
            error
                .execution
                .completed
                .push("rollbackPlacement".to_owned());
            attach_transaction_cleanup(&mut error, transaction.close());
        }
        return Err(error);
    }
    #[cfg(unix)]
    observer.before_transaction_cleanup(target_root);
    let transaction_path = transaction.resolve_path(target_root);
    let cleanup = transaction.close();
    match (transaction_path, cleanup) {
        (Ok(_), Ok(())) => Ok(None),
        (Err(path_error), Ok(())) => Ok(Some(
            CliError::new(
                ErrorCode::PathOutsideRepo,
                "copy/link committed, but the target path changed before cleanup",
            )
            .with_details(BTreeMap::from([
                ("committed".to_owned(), json!(true)),
                ("pathError".to_owned(), json!(path_error.message)),
            ])),
        )),
        (transaction_path, Err(error)) => {
            let mut cleanup_error = CliError::new(
                ErrorCode::InternalError,
                "copy/link committed, but transaction cleanup failed",
            )
            .with_details(BTreeMap::from([
                ("committed".to_owned(), json!(true)),
                ("cleanupError".to_owned(), json!(error.to_string())),
            ]));
            if let Ok(transaction_path) = transaction_path {
                cleanup_error
                    .details
                    .insert("recoveryRequired".to_owned(), json!(true));
                cleanup_error
                    .details
                    .insert("recoveryPath".to_owned(), json!(transaction_path));
            }
            Ok(Some(cleanup_error))
        }
    }
}

fn attach_transaction_cleanup(error: &mut CliError, cleanup: Result<(), std::io::Error>) {
    if let Err(cleanup) = cleanup {
        error
            .details
            .insert("transactionCleanupFailed".to_owned(), json!(true));
        error.details.insert(
            "transactionCleanupError".to_owned(),
            json!(cleanup.to_string()),
        );
    }
}

fn stage_all(
    plans: &[PlacementPlan],
    placement: FilePlacement,
    transaction: &PlacementTransaction,
    observer: &dyn PlacementObserver,
) -> Result<Vec<StagedPlacement>, CliError> {
    let mut staged = Vec::with_capacity(plans.len());
    for (index, plan) in plans.iter().enumerate() {
        transaction.revalidate_root(&plan.target_root)?;
        let source = plan.revalidate_source()?;
        observer.after_source_validation(index, plan);
        let staged_name = index.to_string();
        match placement {
            FilePlacement::Copy => {
                #[cfg(unix)]
                copy_entry_secure(
                    &plan.repo_root,
                    &plan.relative,
                    &transaction.staged_fd,
                    OsStr::new(&staged_name),
                    &plan.source_guard,
                    observer,
                )?;
                #[cfg(not(unix))]
                copy_entry_secure(
                    &plan.repo_root,
                    &plan.relative,
                    &transaction.staged_root().join(&staged_name),
                    &plan.source_guard,
                )?;
            }
            FilePlacement::Link => {
                #[cfg(unix)]
                stage_link(
                    plan,
                    &source,
                    &transaction.staged_fd,
                    OsStr::new(&staged_name),
                )?;
                #[cfg(not(unix))]
                stage_link(plan, &source, &transaction.staged_root().join(&staged_name))?;
            }
        }
        let _ = plan.revalidate_source()?;
        transaction.revalidate_root(&plan.target_root)?;
        staged.push(StagedPlacement);
    }
    Ok(staged)
}

#[cfg(unix)]
fn stage_link(
    plan: &PlacementPlan,
    source: &Path,
    staged_parent: &OwnedFd,
    staged_name: &OsStr,
) -> Result<(), CliError> {
    let destination = plan.target_root.join(&plan.relative);
    let parent = destination
        .parent()
        .ok_or_else(|| CliError::new(ErrorCode::InternalError, "link destination has no parent"))?;
    let relative_target = relative_path(parent, source)?;
    symlinkat(&relative_target, staged_parent, staged_name).map_err(|error| {
        nix_io(
            "create staged symbolic link through transaction handle",
            &destination,
            error,
        )
    })
}

#[cfg(not(unix))]
fn stage_link(_plan: &PlacementPlan, _source: &Path, _staged: &Path) -> Result<(), CliError> {
    Err(CliError::new(
        ErrorCode::UnsupportedRepositoryLayout,
        "link is unsupported on this platform",
    ))
}

fn validate_relative(path: &Path) -> Result<(), CliError> {
    if path.is_absolute() {
        return Err(CliError::new(
            ErrorCode::AbsolutePathNotAllowed,
            format!("absolute path is not allowed: {}", path.display()),
        ));
    }
    let valid = !path.as_os_str().is_empty()
        && path
            .components()
            .any(|component| matches!(component, Component::Normal(_)))
        && path
            .components()
            .all(|component| matches!(component, Component::CurDir | Component::Normal(_)));
    if valid {
        Ok(())
    } else {
        Err(path_outside(
            "path must be repository-relative without traversal",
            path,
        ))
    }
}

fn validate_tree_symlinks(path: &Path, root: &Path) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        io_error(
            ErrorCode::PathOutsideRepo,
            "failed to inspect source path",
            path,
            error,
        )
    })?;
    if metadata.file_type().is_symlink() {
        let resolved = canonicalize(path, ErrorCode::PathOutsideRepo)?;
        if !resolved.starts_with(root) {
            return Err(path_outside("symbolic link escapes repository root", path));
        }
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| {
            io_error(
                ErrorCode::PathOutsideRepo,
                "failed to inspect source directory",
                path,
                error,
            )
        })? {
            let entry = entry.map_err(|error| {
                io_error(
                    ErrorCode::PathOutsideRepo,
                    "failed to inspect source directory entry",
                    path,
                    error,
                )
            })?;
            validate_tree_symlinks(&entry.path(), root)?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MetadataIdentity {
    kind: FileKind,
    len: u64,
    modified: Option<SystemTime>,
    readonly: bool,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl MetadataIdentity {
    fn capture(path: &Path, follow_symlink: bool) -> Result<Self, CliError> {
        let metadata = if follow_symlink {
            fs::metadata(path)
        } else {
            fs::symlink_metadata(path)
        }
        .map_err(|error| {
            io_error(
                ErrorCode::PathOutsideRepo,
                "failed to capture filesystem identity",
                path,
                error,
            )
        })?;
        let file_type = metadata.file_type();
        Ok(Self {
            kind: if file_type.is_symlink() {
                FileKind::Symlink
            } else if file_type.is_dir() {
                FileKind::Directory
            } else if file_type.is_file() {
                FileKind::File
            } else {
                FileKind::Special
            },
            len: metadata.len(),
            modified: metadata.modified().ok(),
            readonly: metadata.permissions().readonly(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
        })
    }

    fn same_object(&self, other: &Self) -> bool {
        #[cfg(unix)]
        return self.kind == other.kind && self.device == other.device && self.inode == other.inode;
        #[cfg(not(unix))]
        {
            self.kind == other.kind
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileKind {
    File,
    Directory,
    Symlink,
    Special,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceEntryIdentity {
    relative: PathBuf,
    metadata: MetadataIdentity,
    link_target: Option<PathBuf>,
}

fn capture_source_tree(
    path: &Path,
    repo_root: &Path,
) -> Result<Vec<SourceEntryIdentity>, CliError> {
    fn visit(
        root: &Path,
        path: &Path,
        repo_root: &Path,
        entries: &mut Vec<SourceEntryIdentity>,
    ) -> Result<(), CliError> {
        let metadata = MetadataIdentity::capture(path, false)?;
        let link_target = if metadata.kind == FileKind::Symlink {
            Some(fs::read_link(path).map_err(|error| {
                io_error(
                    ErrorCode::PathOutsideRepo,
                    "failed to capture source symlink",
                    path,
                    error,
                )
            })?)
        } else {
            None
        };
        entries.push(SourceEntryIdentity {
            relative: path
                .strip_prefix(root)
                .unwrap_or(Path::new(""))
                .to_path_buf(),
            metadata: metadata.clone(),
            link_target,
        });
        if metadata.kind == FileKind::Directory {
            let mut children = fs::read_dir(path)
                .map_err(|error| {
                    io_error(
                        ErrorCode::PathOutsideRepo,
                        "failed to capture source directory",
                        path,
                        error,
                    )
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    io_error(
                        ErrorCode::PathOutsideRepo,
                        "failed to capture source directory entry",
                        path,
                        error,
                    )
                })?;
            children.sort_by_key(fs::DirEntry::file_name);
            for child in children {
                let child = child.path();
                let resolved = canonicalize(&child, ErrorCode::PathOutsideRepo)?;
                if !resolved.starts_with(repo_root) {
                    return Err(path_outside("source entry escapes repository root", &child));
                }
                visit(root, &child, repo_root, entries)?;
            }
        }
        Ok(())
    }

    let mut entries = Vec::new();
    visit(path, path, repo_root, &mut entries)?;
    Ok(entries)
}

#[derive(Clone, Debug)]
struct DestinationGuard {
    ancestor: PathBuf,
    ancestor_identity: MetadataIdentity,
    destination_identity: Option<MetadataIdentity>,
}

impl DestinationGuard {
    fn capture(destination: &Path, target_root: &Path) -> Result<Self, CliError> {
        let parent = destination.parent().ok_or_else(|| {
            CliError::new(ErrorCode::InternalError, "destination path has no parent")
        })?;
        let ancestor = existing_ancestor(parent)?.to_path_buf();
        validate_contained_directory(&ancestor, target_root)?;
        Ok(Self {
            ancestor_identity: MetadataIdentity::capture(&ancestor, false)?,
            ancestor,
            destination_identity: optional_identity(destination)?,
        })
    }

    fn revalidate(&self, destination: &Path, target_root: &Path) -> Result<(), CliError> {
        validate_contained_directory(&self.ancestor, target_root)?;
        let current_ancestor = MetadataIdentity::capture(&self.ancestor, false)?;
        if !current_ancestor.same_object(&self.ancestor_identity) {
            return Err(path_changed(
                "destination ancestor changed after validation",
                &self.ancestor,
            ));
        }
        if optional_identity(destination)? != self.destination_identity {
            return Err(path_changed(
                "destination changed after validation",
                destination,
            ));
        }
        Ok(())
    }
}

fn optional_identity(path: &Path) -> Result<Option<MetadataIdentity>, CliError> {
    match fs::symlink_metadata(path) {
        Ok(_) => MetadataIdentity::capture(path, false).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(
            ErrorCode::PathOutsideRepo,
            "failed to inspect destination identity",
            path,
            error,
        )),
    }
}

#[cfg(unix)]
fn optional_identity_at(
    parent: &OwnedFd,
    name: &std::ffi::OsStr,
) -> Result<Option<MetadataIdentity>, CliError> {
    match fstatat(parent, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(metadata_identity_from_stat(&stat))),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(error) => Err(nix_io(
            "inspect destination through parent directory handle",
            Path::new(name),
            error,
        )),
    }
}

#[cfg(unix)]
// libc stat field widths vary across Unix targets, so this conversion is
// redundant on Linux but required on macOS.
#[allow(clippy::useless_conversion)]
fn metadata_identity_from_stat(stat: &FileStat) -> MetadataIdentity {
    let kind = SFlag::from_bits_truncate(stat.st_mode);
    MetadataIdentity {
        kind: if kind & SFlag::S_IFMT == SFlag::S_IFLNK {
            FileKind::Symlink
        } else if kind & SFlag::S_IFMT == SFlag::S_IFDIR {
            FileKind::Directory
        } else if kind & SFlag::S_IFMT == SFlag::S_IFREG {
            FileKind::File
        } else {
            FileKind::Special
        },
        len: stat.st_size.try_into().unwrap_or_default(),
        modified: None,
        readonly: stat.st_mode & 0o222 == 0,
        device: stat.st_dev.try_into().unwrap_or_default(),
        inode: stat.st_ino,
    }
}

fn validate_contained_directory(path: &Path, root: &Path) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        io_error(
            ErrorCode::PathOutsideRepo,
            "failed to inspect destination directory",
            path,
            error,
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(path_outside(
            "destination ancestor is not a real directory",
            path,
        ));
    }
    let resolved = canonicalize(path, ErrorCode::PathOutsideRepo)?;
    if !resolved.starts_with(root) {
        return Err(path_outside(
            "destination path escapes target worktree",
            path,
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn copy_entry_secure(
    repo_root: &Path,
    relative: &Path,
    destination_parent: &OwnedFd,
    destination_name: &OsStr,
    source_guard: &[SourceEntryIdentity],
    observer: &dyn PlacementObserver,
) -> Result<(), CliError> {
    let root = open(
        repo_root,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_io("open repository root", repo_root, error))?;
    let parent = open_existing_parent(&root, relative.parent().unwrap_or(Path::new("")))?;
    let name = relative.file_name().ok_or_else(|| {
        CliError::new(
            ErrorCode::InvalidArgument,
            "copy source path has no file name",
        )
    })?;
    copy_node_at(
        &parent,
        name,
        destination_parent,
        destination_name,
        &repo_root.join(relative),
        Path::new(""),
        source_guard,
        observer,
    )
}

#[cfg(not(unix))]
fn copy_entry_secure(
    _repo_root: &Path,
    _relative: &Path,
    _destination: &Path,
    _source_guard: &[SourceEntryIdentity],
) -> Result<(), CliError> {
    Err(CliError::new(
        ErrorCode::UnsupportedRepositoryLayout,
        "copy/link placement is unsupported on this platform",
    ))
}

#[cfg(unix)]
fn open_existing_parent(root: &OwnedFd, relative: &Path) -> Result<OwnedFd, CliError> {
    let mut current = openat(
        root,
        Path::new("."),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_io("duplicate repository directory handle", relative, error))?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        current = openat(
            &current,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            nix_io(
                "open source directory without following symlinks",
                relative,
                error,
            )
        })?;
    }
    Ok(current)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn copy_node_at(
    source_parent: &impl std::os::fd::AsFd,
    source_name: &std::ffi::OsStr,
    destination_parent: &impl std::os::fd::AsFd,
    destination_name: &std::ffi::OsStr,
    display_destination: &Path,
    relative: &Path,
    source_guard: &[SourceEntryIdentity],
    observer: &dyn PlacementObserver,
) -> Result<(), CliError> {
    let stat =
        fstatat(source_parent, source_name, AtFlags::AT_SYMLINK_NOFOLLOW).map_err(|error| {
            nix_io(
                "inspect source through directory handle",
                display_destination,
                error,
            )
        })?;
    let kind = SFlag::from_bits_truncate(stat.st_mode);
    let expected = source_guard
        .iter()
        .find(|entry| entry.relative == relative)
        .ok_or_else(|| path_changed("source entry appeared during staging", display_destination))?;
    let current = metadata_identity_from_stat(&stat);
    if !current.same_object(&expected.metadata) || current.kind != expected.metadata.kind {
        return Err(path_changed(
            "source entry changed before it was opened through a directory handle",
            display_destination,
        ));
    }
    if kind & SFlag::S_IFMT == SFlag::S_IFLNK {
        let target = expected.link_target.as_ref().ok_or_else(|| {
            path_changed(
                "validated source symlink target is unavailable",
                display_destination,
            )
        })?;
        return symlinkat(target, destination_parent, destination_name).map_err(|error| {
            nix_io(
                "stage source symlink through transaction handle",
                display_destination,
                error,
            )
        });
    }
    if kind & SFlag::S_IFMT == SFlag::S_IFREG {
        observer.before_source_open(relative);
        let source = openat(
            source_parent,
            source_name,
            OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            nix_io(
                "open source file without following symlinks",
                display_destination,
                error,
            )
        })?;
        observer.after_source_open(relative);
        let opened_identity =
            metadata_identity_from_stat(&fstat(&source).map_err(|error| {
                nix_io("inspect opened source file", display_destination, error)
            })?);
        if !opened_identity.same_object(&expected.metadata) {
            return Err(path_changed(
                "opened source file differs from its validated identity",
                display_destination,
            ));
        }
        let mut source = fs::File::from(source);
        let output = openat(
            destination_parent,
            destination_name,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(stat.st_mode & 0o7777),
        )
        .map_err(|error| {
            nix_io(
                "create staged copy through transaction handle",
                display_destination,
                error,
            )
        })?;
        let mut output = fs::File::from(output);
        io::copy(&mut source, &mut output).map_err(|error| {
            io_error(
                ErrorCode::InternalError,
                "copy source through validated file handle",
                display_destination,
                error,
            )
        })?;
        fchmod(&output, Mode::from_bits_truncate(stat.st_mode & 0o7777)).map_err(|error| {
            nix_io(
                "preserve staged file permissions",
                display_destination,
                error,
            )
        })?;
        return Ok(());
    }
    if kind & SFlag::S_IFMT != SFlag::S_IFDIR {
        return Err(CliError::new(
            ErrorCode::InvalidArgument,
            format!(
                "unsupported copy source type: {}",
                source_name.to_string_lossy()
            ),
        ));
    }
    observer.before_source_open(relative);
    let directory = openat(
        source_parent,
        source_name,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        nix_io(
            "open source directory without following symlinks",
            display_destination,
            error,
        )
    })?;
    observer.after_source_open(relative);
    let opened_identity = metadata_identity_from_stat(&fstat(&directory).map_err(|error| {
        nix_io(
            "inspect opened source directory",
            display_destination,
            error,
        )
    })?);
    if !opened_identity.same_object(&expected.metadata) {
        return Err(path_changed(
            "opened source directory differs from its validated identity",
            display_destination,
        ));
    }
    mkdirat(
        destination_parent,
        destination_name,
        Mode::from_bits_truncate(stat.st_mode & 0o7777),
    )
    .map_err(|error| {
        nix_io(
            "create staged directory through transaction handle",
            display_destination,
            error,
        )
    })?;
    let destination_directory = openat(
        destination_parent,
        destination_name,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_io("open staged directory handle", display_destination, error))?;
    let mut directory = Dir::from_fd(directory)
        .map_err(|error| nix_io("read source directory handle", display_destination, error))?;
    let names = directory
        .iter()
        .map(|entry| {
            let entry = entry
                .map_err(|error| nix_io("read source directory", display_destination, error))?;
            Ok(OsString::from_vec(entry.file_name().to_bytes().to_vec()))
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    for child in names {
        if child == "." || child == ".." {
            continue;
        }
        copy_node_at(
            &directory,
            &child,
            &destination_directory,
            &child,
            &display_destination.join(&child),
            &relative.join(&child),
            source_guard,
            observer,
        )?;
    }
    fchmod(
        &destination_directory,
        Mode::from_bits_truncate(stat.st_mode & 0o7777),
    )
    .map_err(|error| {
        nix_io(
            "preserve staged directory permissions",
            display_destination,
            error,
        )
    })
}

#[cfg(unix)]
fn nix_io(action: &str, path: &Path, error: nix::errno::Errno) -> CliError {
    io_error(
        ErrorCode::PathOutsideRepo,
        action,
        path,
        std::io::Error::from_raw_os_error(error as i32),
    )
}

#[cfg(all(
    unix,
    any(
        target_vendor = "apple",
        target_os = "linux",
        target_os = "android",
        target_os = "redox"
    )
))]
fn rename_noreplace<OldFd: std::os::fd::AsFd, NewFd: std::os::fd::AsFd>(
    old_fd: &OldFd,
    old_name: &OsStr,
    new_fd: &NewFd,
    new_name: &OsStr,
    destination_parent: &Path,
) -> Result<(), CliError> {
    renameat_with(old_fd, old_name, new_fd, new_name, RenameFlags::NOREPLACE).map_err(|error| {
        io_error(
            ErrorCode::PathOutsideRepo,
            "atomically install without replacing a concurrent destination",
            &destination_parent.join(new_name),
            std::io::Error::from_raw_os_error(error.raw_os_error()),
        )
    })
}

#[cfg(all(
    unix,
    not(any(
        target_vendor = "apple",
        target_os = "linux",
        target_os = "android",
        target_os = "redox"
    ))
))]
fn rename_noreplace<OldFd: std::os::fd::AsFd, NewFd: std::os::fd::AsFd>(
    _old_fd: &OldFd,
    _old_name: &OsStr,
    _new_fd: &NewFd,
    _new_name: &OsStr,
    _destination_parent: &Path,
) -> Result<(), CliError> {
    Err(CliError::new(
        ErrorCode::UnsupportedRepositoryLayout,
        "atomic no-clobber placement is unsupported on this Unix platform",
    ))
}

#[cfg(unix)]
fn commit_all(
    plans: &[PlacementPlan],
    staged: &[StagedPlacement],
    transaction: &PlacementTransaction,
    observer: &dyn PlacementObserver,
) -> Result<(), CliError> {
    let mut committed = Vec::with_capacity(plans.len());
    let mut created_directories = BTreeSet::new();
    for (index, (plan, staged)) in plans.iter().zip(staged).enumerate() {
        let result = commit_one(
            index,
            plan,
            staged,
            transaction,
            observer,
            &mut created_directories,
        );
        match result {
            Ok(entry) => {
                committed.push(entry);
                if let Err(mut error) = transaction.revalidate_root(&plan.target_root) {
                    let failures = rollback_all(&mut committed, transaction, observer);
                    attach_rollback_failures(&mut error, &failures);
                    return Err(error);
                }
            }
            Err(mut error) => {
                let mut failures = rollback_all(&mut committed, transaction, observer);
                if failures.is_empty()
                    && let Err(cleanup) =
                        remove_created_directories_at(&transaction.target_fd, &created_directories)
                {
                    failures
                        .push(json!({ "phase": "remove-created-directory", "message": cleanup }));
                }
                attach_rollback_failures(&mut error, &failures);
                return Err(error);
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn commit_all(
    _plans: &[PlacementPlan],
    _staged: &[StagedPlacement],
    _transaction: &PlacementTransaction,
    _observer: &dyn PlacementObserver,
) -> Result<(), CliError> {
    Err(CliError::new(
        ErrorCode::UnsupportedRepositoryLayout,
        "copy/link transactional commit is unsupported on this platform",
    ))
}

#[cfg(unix)]
#[allow(clippy::too_many_lines)]
fn commit_one(
    index: usize,
    plan: &PlacementPlan,
    _staged: &StagedPlacement,
    transaction: &PlacementTransaction,
    observer: &dyn PlacementObserver,
    created_directories: &mut BTreeSet<PathBuf>,
) -> Result<CommittedPlacement, CliError> {
    transaction.revalidate_root(&plan.target_root)?;
    plan.destination_guard
        .revalidate(&plan.target_root.join(&plan.relative), &plan.target_root)?;
    let handle = open_destination_handle(plan, transaction, created_directories)?;
    observer.after_destination_validation(index, plan);
    revalidate_destination_handle(plan, &handle)?;
    observer.before_commit(index, plan)?;
    transaction.revalidate_root(&plan.target_root)?;
    revalidate_destination_handle(plan, &handle)?;
    observer.before_destination_move(index, plan);
    transaction.revalidate_root(&plan.target_root)?;
    let entry_name = index.to_string();
    let entry_name_os = OsStr::new(&entry_name);
    let had_backup = plan.destination_guard.destination_identity.is_some();
    let staged_identity =
        optional_identity_at(&transaction.staged_fd, entry_name_os)?.ok_or_else(|| {
            path_changed(
                "staged placement disappeared before install",
                &handle.parent_path,
            )
        })?;
    if had_backup {
        renameat(
            &handle.parent_fd,
            handle.name.as_os_str(),
            &transaction.backup_fd,
            entry_name.as_str(),
        )
        .map_err(|error| {
            nix_io(
                "move destination into transaction backup",
                &handle.parent_path,
                error,
            )
        })?;
        let moved = match optional_identity_at(&transaction.backup_fd, entry_name_os) {
            Ok(moved) => moved,
            Err(mut error) => {
                if let Err(restore_error) = rename_noreplace(
                    &transaction.backup_fd,
                    entry_name_os,
                    &handle.parent_fd,
                    handle.name.as_os_str(),
                    &handle.parent_path,
                ) {
                    attach_rollback_failures(
                        &mut error,
                        &[json!({
                            "phase": "restore-after-backup-inspection",
                            "message": restore_error.message
                        })],
                    );
                }
                return Err(error);
            }
        };
        let expected = plan.destination_guard.destination_identity.as_ref();
        if moved
            .as_ref()
            .is_none_or(|moved| expected.is_none_or(|expected| !moved.same_object(expected)))
        {
            let mut error = path_changed(
                "destination changed while it was moved into transaction backup",
                &handle.parent_path.join(&handle.name),
            );
            if let Err(restore_error) = rename_noreplace(
                &transaction.backup_fd,
                entry_name_os,
                &handle.parent_fd,
                handle.name.as_os_str(),
                &handle.parent_path,
            ) {
                attach_rollback_failures(
                    &mut error,
                    &[
                        json!({ "phase": "restore-mismatched-backup", "message": restore_error.message }),
                    ],
                );
            }
            return Err(error);
        }
        observer.after_destination_backup(index, plan);
    }
    if let Err(install_error) = rename_noreplace(
        &transaction.staged_fd,
        entry_name_os,
        &handle.parent_fd,
        handle.name.as_os_str(),
        &handle.parent_path,
    ) {
        let mut error = install_error;
        if had_backup
            && let Err(restore_error) = rename_noreplace(
                &transaction.backup_fd,
                entry_name_os,
                &handle.parent_fd,
                handle.name.as_os_str(),
                &handle.parent_path,
            )
        {
            attach_rollback_failures(
                &mut error,
                &[json!({ "phase": "restore-current", "message": restore_error.message })],
            );
        }
        return Err(error);
    }
    let installed_identity = match optional_identity_at(&handle.parent_fd, &handle.name) {
        Ok(Some(identity)) => identity,
        Ok(None) => {
            let mut error = path_changed("installed placement disappeared", &handle.parent_path);
            attach_rollback_failures(
                &mut error,
                &[json!({
                    "phase": "inspect-installed",
                    "message": "installed destination disappeared before identity confirmation"
                })],
            );
            return Err(error);
        }
        Err(mut error) => {
            let failures =
                rollback_current_install(index, &handle, transaction, had_backup, &staged_identity);
            attach_rollback_failures(&mut error, &failures);
            return Err(error);
        }
    };
    if !installed_identity.same_object(&staged_identity) {
        let mut error = path_changed(
            "destination changed immediately after atomic install",
            &handle.parent_path.join(&handle.name),
        );
        let failures =
            rollback_current_install(index, &handle, transaction, had_backup, &staged_identity);
        attach_rollback_failures(&mut error, &failures);
        return Err(error);
    }
    Ok(CommittedPlacement {
        handle,
        index,
        had_backup,
        installed_identity,
    })
}

#[cfg(unix)]
fn rollback_current_install(
    index: usize,
    handle: &DestinationHandle,
    transaction: &PlacementTransaction,
    had_backup: bool,
    expected_installed: &MetadataIdentity,
) -> Vec<Value> {
    let mut failures = Vec::new();
    let preserved_name = format!("failed-{index}");
    let preserved_name = OsStr::new(&preserved_name);
    if let Err(error) = renameat(
        &handle.parent_fd,
        handle.name.as_os_str(),
        &transaction.staged_fd,
        preserved_name,
    ) {
        failures.push(json!({
            "phase": "preserve-unverified-install",
            "message": error.to_string()
        }));
        return failures;
    }
    let moved = optional_identity_at(&transaction.staged_fd, preserved_name);
    if !matches!(moved, Ok(Some(ref identity)) if identity.same_object(expected_installed)) {
        let restore = rename_noreplace(
            &transaction.staged_fd,
            preserved_name,
            &handle.parent_fd,
            handle.name.as_os_str(),
            &handle.parent_path,
        );
        failures.push(json!({
            "phase": "unverified-install-identity",
            "message": restore.err().map_or_else(
                || "concurrent destination restored; validated backup retained".to_owned(),
                |error| format!("concurrent destination recovery failed: {}", error.message),
            )
        }));
        return failures;
    }
    if had_backup
        && let Err(error) = rename_noreplace(
            &transaction.backup_fd,
            OsStr::new(&index.to_string()),
            &handle.parent_fd,
            handle.name.as_os_str(),
            &handle.parent_path,
        )
    {
        failures.push(json!({
            "phase": "restore-after-install-inspection",
            "message": error.message
        }));
    }
    failures
}

#[cfg(unix)]
fn revalidate_destination_handle(
    plan: &PlacementPlan,
    handle: &DestinationHandle,
) -> Result<(), CliError> {
    let _ = plan.revalidate_source()?;
    validate_parent_identity(&handle.parent_path, &handle.parent_identity)?;
    let current = optional_identity_at(&handle.parent_fd, &handle.name)?;
    let expected = plan.destination_guard.destination_identity.as_ref();
    let matches = match (current.as_ref(), expected) {
        (None, None) => true,
        (Some(current), Some(expected)) => current.same_object(expected),
        _ => false,
    };
    if !matches {
        return Err(path_changed(
            "destination changed after its parent handle was opened",
            &handle.parent_path.join(&handle.name),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_parent_identity(parent: &Path, identity: &MetadataIdentity) -> Result<(), CliError> {
    let current = MetadataIdentity::capture(parent, false)?;
    if !current.same_object(identity) {
        return Err(path_changed(
            "destination parent changed during placement",
            parent,
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_destination_handle(
    plan: &PlacementPlan,
    transaction: &PlacementTransaction,
    created: &mut BTreeSet<PathBuf>,
) -> Result<DestinationHandle, CliError> {
    let mut current = openat(
        &transaction.target_fd,
        Path::new("."),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_io("duplicate target root handle", &plan.target_root, error))?;
    let mut relative_parent = PathBuf::new();
    for component in plan.relative.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(name) = component else {
            continue;
        };
        relative_parent.push(name);
        let absolute = plan.target_root.join(&relative_parent);
        let initially_existing = plan.destination_guard.ancestor.starts_with(&absolute);
        match openat(
            &current,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        ) {
            Ok(next) => {
                if !initially_existing && !created.contains(&relative_parent) {
                    return Err(path_changed(
                        "destination parent appeared concurrently",
                        &absolute,
                    ));
                }
                current = next;
            }
            Err(nix::errno::Errno::ENOENT) => {
                mkdirat(&current, name, Mode::from_bits_truncate(0o755)).map_err(|error| {
                    nix_io(
                        "create destination directory through handle",
                        &absolute,
                        error,
                    )
                })?;
                created.insert(relative_parent.clone());
                current = openat(
                    &current,
                    name,
                    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|error| nix_io("open created destination directory", &absolute, error))?;
            }
            Err(error) => {
                return Err(nix_io(
                    "open destination directory without following symlinks",
                    &absolute,
                    error,
                ));
            }
        }
    }
    let parent_path = plan.target_root.join(&relative_parent);
    let parent_identity = MetadataIdentity::capture(&parent_path, false)?;
    let held_parent_identity = metadata_identity_from_stat(
        &fstat(&current)
            .map_err(|error| nix_io("inspect held destination parent", &parent_path, error))?,
    );
    if !held_parent_identity.same_object(&parent_identity) {
        return Err(path_changed(
            "destination parent changed while its handle was opened",
            &parent_path,
        ));
    }
    Ok(DestinationHandle {
        parent_identity,
        parent_fd: current,
        parent_path,
        name: plan
            .relative
            .file_name()
            .ok_or_else(|| CliError::new(ErrorCode::InvalidArgument, "destination has no name"))?
            .to_os_string(),
    })
}

#[cfg(unix)]
fn rollback_all(
    committed: &mut Vec<CommittedPlacement>,
    transaction: &PlacementTransaction,
    observer: &dyn PlacementObserver,
) -> Vec<Value> {
    let mut failures = Vec::new();
    while let Some(entry) = committed.pop() {
        if let Err(error) = rollback_one(&entry, transaction, observer) {
            failures.push(json!({
                "path": entry.handle.parent_path.join(&entry.handle.name),
                "phase": "rollback-committed-destination",
                "message": error.clone(),
            }));
        }
    }
    failures
}

#[cfg(unix)]
fn rollback_one(
    entry: &CommittedPlacement,
    transaction: &PlacementTransaction,
    observer: &dyn PlacementObserver,
) -> Result<(), String> {
    let rollback_name = format!("rollback-{}", entry.index);
    let rollback_name_os = OsStr::new(&rollback_name);
    let current = optional_identity_at(&entry.handle.parent_fd, &entry.handle.name)
        .map_err(|error| error.message)?
        .ok_or_else(|| "installed destination disappeared before rollback".to_owned())?;
    if !current.same_object(&entry.installed_identity) {
        return Err("installed destination changed before rollback".to_owned());
    }
    renameat(
        &entry.handle.parent_fd,
        entry.handle.name.as_os_str(),
        &transaction.staged_fd,
        rollback_name_os,
    )
    .map_err(|error| format!("preserve installed value before rollback: {error}"))?;
    let moved = optional_identity_at(&transaction.staged_fd, rollback_name_os)
        .map_err(|error| error.message)?
        .ok_or_else(|| "installed destination disappeared while preserving rollback".to_owned())?;
    if !moved.same_object(&entry.installed_identity) {
        let restore = rename_noreplace(
            &transaction.staged_fd,
            rollback_name_os,
            &entry.handle.parent_fd,
            entry.handle.name.as_os_str(),
            &entry.handle.parent_path,
        );
        return Err(format!(
            "installed destination changed while preserving rollback; restore={}",
            restore
                .err()
                .map_or_else(|| "completed".to_owned(), |error| error.message)
        ));
    }
    observer.before_rollback(entry.index)?;
    if entry.had_backup {
        rename_noreplace(
            &transaction.backup_fd,
            OsStr::new(&entry.index.to_string()),
            &entry.handle.parent_fd,
            entry.handle.name.as_os_str(),
            &entry.handle.parent_path,
        )
        .map_err(|error| format!("restore transaction backup: {}", error.message))?;
    }
    Ok(())
}

#[cfg(unix)]
fn attach_rollback_failures(error: &mut CliError, failures: &[Value]) {
    if !failures.is_empty() {
        error
            .details
            .insert("rollbackFailed".to_owned(), json!(true));
        error
            .details
            .insert("rollbackFailures".to_owned(), json!(failures));
    }
}

#[cfg(unix)]
fn remove_created_directories_at(
    root: &OwnedFd,
    directories: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let mut directories = directories.iter().collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        unlinkat(root, directory, UnlinkatFlags::RemoveDir)
            .map_err(|error| format!("remove {}: {error}", directory.display()))?;
    }
    Ok(())
}

fn existing_ancestor(mut path: &Path) -> Result<&Path, CliError> {
    loop {
        match fs::symlink_metadata(path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                path = path
                    .parent()
                    .ok_or_else(|| path_outside("path has no existing ancestor", path))?;
            }
            Err(error) => {
                return Err(io_error(
                    ErrorCode::PathOutsideRepo,
                    "failed to inspect path ancestor",
                    path,
                    error,
                ));
            }
        }
    }
}

#[cfg(unix)]
fn relative_path(from: &Path, to: &Path) -> Result<PathBuf, CliError> {
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let shared = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    if shared == 0 {
        return Err(CliError::new(
            ErrorCode::UnsupportedRepositoryLayout,
            "source and destination are on different filesystem roots",
        ));
    }
    let mut relative = PathBuf::new();
    for _ in shared..from_components.len() {
        relative.push("..");
    }
    for component in &to_components[shared..] {
        relative.push(component.as_os_str());
    }
    Ok(relative)
}

fn completion_candidates<R: ProcessRunner>(
    context: &RepoContext,
    config: &ResolvedConfig,
    git: &GitCli<R>,
    kind: CompletionCandidateKind,
) -> Result<MiscCommandOutput, CliError> {
    let rows = match kind {
        CompletionCandidateKind::Worktrees => list_worktrees(git, &context.repo_root)?
            .into_iter()
            .filter_map(|worktree| {
                worktree.branch.map(|branch| {
                    format!(
                        "{}\tpath={}",
                        sanitize(&branch),
                        sanitize(&worktree.path.to_string_lossy())
                    )
                })
            })
            .collect::<Vec<_>>(),
        CompletionCandidateKind::UseBranches => {
            let output = git
                .execute_checked(
                    &context.repo_root,
                    ["for-each-ref", "--format=%(refname:short)", "refs/heads"],
                )
                .map_err(MapToCliError::map_to_cli_error)?;
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(sanitize)
                .collect()
        }
        CompletionCandidateKind::RemoteBranches => {
            let output = git
                .execute_checked(
                    &context.repo_root,
                    ["for-each-ref", "--format=%(refname:short)", "refs/remotes"],
                )
                .map_err(MapToCliError::map_to_cli_error)?;
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|branch| !branch.ends_with("/HEAD"))
                .map(sanitize)
                .collect()
        }
        CompletionCandidateKind::Hooks => {
            let hooks = context.repo_root.join(".vde/worktree/hooks");
            let mut rows = match fs::read_dir(&hooks) {
                Ok(entries) => entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .filter(|name| name.starts_with("pre-") || name.starts_with("post-"))
                    .map(|name| sanitize(&name))
                    .collect::<Vec<_>>(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => {
                    return Err(io_error(
                        ErrorCode::InternalError,
                        "failed to list hooks",
                        &hooks,
                        error,
                    ));
                }
            };
            rows.sort();
            rows
        }
        CompletionCandidateKind::ManagedWorktrees => {
            let managed_root = if Path::new(&config.paths.worktree_root).is_absolute() {
                PathBuf::from(&config.paths.worktree_root)
            } else {
                context.repo_root.join(&config.paths.worktree_root)
            };
            list_worktrees(git, &context.repo_root)?
                .into_iter()
                .filter_map(|worktree| {
                    let relative = worktree.path.strip_prefix(&managed_root).ok()?;
                    (!relative.as_os_str().is_empty()).then(|| {
                        format!(
                            "{}\tbranch={}",
                            sanitize(&relative.to_string_lossy()),
                            sanitize(worktree.branch.as_deref().unwrap_or("(detached)"))
                        )
                    })
                })
                .collect()
        }
    };
    let mut output = MiscCommandOutput::success(json!({ "candidates": rows }));
    output.human_stdout = if rows.is_empty() {
        String::new()
    } else {
        format!("{}\n", rows.join("\n"))
    };
    Ok(output)
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            other => other,
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn list_worktrees<R: ProcessRunner>(
    git: &GitCli<R>,
    repo_root: &Path,
) -> Result<Vec<GitWorktree>, CliError> {
    let output = git
        .execute_checked(repo_root, ["worktree", "list", "--porcelain", "-z"])
        .map_err(MapToCliError::map_to_cli_error)?;
    let mut registry = parse_worktree_porcelain(&output.stdout)
        .map_err(|error| CliError::new(ErrorCode::InternalError, error.to_string()))?;
    if let Some(primary) = registry.first_mut() {
        primary.path = repo_root.to_path_buf();
    }
    Ok(registry)
}

fn canonicalize(path: &Path, code: ErrorCode) -> Result<PathBuf, CliError> {
    fs::canonicalize(path)
        .map_err(|error| io_error(code, "failed to resolve filesystem path", path, error))
}

fn path_outside(message: &str, path: &Path) -> CliError {
    CliError::new(ErrorCode::PathOutsideRepo, message)
        .with_details(BTreeMap::from([("path".to_owned(), json!(path))]))
}

fn path_changed(message: &str, path: &Path) -> CliError {
    CliError::new(ErrorCode::PathOutsideRepo, message)
        .with_details(BTreeMap::from([("path".to_owned(), json!(path))]))
}

fn io_error(code: ErrorCode, message: &str, path: &Path, error: std::io::Error) -> CliError {
    let rendered = format!("{message}: {error}");
    let cause = error.to_string();
    drop(error);
    CliError::new(code, rendered).with_details(BTreeMap::from([
        ("path".to_owned(), json!(path)),
        ("cause".to_owned(), json!(cause)),
    ]))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::cli::{CliParseResult, parse_from};
    use crate::ports::process::{ProcessError, ProcessOutput};

    struct QueueRunner {
        outputs: RefCell<VecDeque<ProcessOutput>>,
        requests: RefCell<Vec<ProcessCommand>>,
    }

    impl QueueRunner {
        fn new(outputs: impl IntoIterator<Item = ProcessOutput>) -> Self {
            Self {
                outputs: RefCell::new(outputs.into_iter().collect()),
                requests: RefCell::new(Vec::new()),
            }
        }
    }

    impl ProcessRunner for QueueRunner {
        fn run(&self, command: &ProcessCommand) -> Result<ProcessOutput, ProcessError> {
            self.requests.borrow_mut().push(command.clone());
            Ok(self.outputs.borrow_mut().pop_front().unwrap())
        }
    }

    fn output(stdout: &[u8], stderr: &[u8], exit_code: i32) -> ProcessOutput {
        ProcessOutput {
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
            exit_code: Some(exit_code),
            timed_out: false,
            ..Default::default()
        }
    }

    fn request(args: &[&str]) -> ParsedRequest {
        let mut full = vec!["vw"];
        full.extend_from_slice(args);
        let CliParseResult::Parsed(request) = parse_from(full) else {
            panic!("request did not parse");
        };
        request
    }

    fn repo_context() -> RepoContext {
        RepoContext {
            repo_root: PathBuf::from("/repo"),
            current_worktree_root: PathBuf::from("/repo"),
            git_common_dir: PathBuf::from("/repo/.git"),
        }
    }

    fn porcelain() -> &'static [u8] {
        b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0worktree /repo/.worktree/topic\0HEAD def\0branch refs/heads/topic\0\0"
    }

    #[test]
    fn exec_uses_shell_free_argv_and_captures_json_streams() {
        let git_runner = QueueRunner::new([output(porcelain(), b"", 0)]);
        let git = GitCli::new(git_runner);
        let child = QueueRunner::new([output(b"hello\n", b"warning\n", 0)]);
        let request = request(&["exec", "topic", "--json", "--", "tool", "$HOME", "a b"]);

        let result = execute_in_worktree(
            &request,
            &repo_context(),
            &git,
            &child,
            Some("topic"),
            match &request.command {
                Command::Exec { argv, .. } => argv,
                _ => unreachable!(),
            },
        )
        .unwrap();

        assert_eq!(result.data["childExitCode"], 0);
        assert_eq!(result.data["childStdout"], "hello\n");
        assert_eq!(result.data["childStderr"], "warning\n");
        let command = &child.requests.borrow()[0];
        assert_eq!(command.program, "tool");
        assert_eq!(command.args, ["$HOME", "a b"].map(OsString::from));
        assert_eq!(command.cwd, Some(PathBuf::from("/repo/.worktree/topic")));
        assert_eq!(command.stdin, StdinPolicy::Null);
        assert_eq!(command.timeout, Some(Duration::from_millis(300_000)));
        assert_eq!(command.max_output_bytes, 1024 * 1024);
        assert_eq!(command.stdout, OutputPolicy::Capture);
        assert_eq!(command.stderr, OutputPolicy::Capture);
    }

    #[test]
    fn exec_nonzero_is_a_partial_child_process_failure() {
        let git_runner = QueueRunner::new([output(porcelain(), b"", 0)]);
        let git = GitCli::new(git_runner);
        let child = QueueRunner::new([output(b"partial", b"failed", 7)]);
        let request = request(&["exec", "topic", "--json", "--", "false"]);

        let result = execute_in_worktree(
            &request,
            &repo_context(),
            &git,
            &child,
            Some("topic"),
            match &request.command {
                Command::Exec { argv, .. } => argv,
                _ => unreachable!(),
            },
        )
        .unwrap();

        let error = result.partial_error.unwrap();
        assert_eq!(error.code, ErrorCode::ChildProcessFailed);
        assert_eq!(error.exit_code(), 21);
        assert_eq!(result.data["childExitCode"], 7);
    }

    #[test]
    fn exec_rejects_non_utf8_target_before_starting_the_child() {
        let mut metadata =
            b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0worktree ".to_vec();
        metadata.extend_from_slice(b"/repo/.worktree/non-utf8-");
        metadata.push(0xff);
        metadata.extend_from_slice(b"\0HEAD def\0branch refs/heads/topic\0\0");
        let git_runner = QueueRunner::new([output(&metadata, b"", 0)]);
        let git = GitCli::new(git_runner);
        let child = QueueRunner::new([]);
        let request = request(&["exec", "topic", "--json", "--", "tool"]);

        let error = execute_in_worktree(
            &request,
            &repo_context(),
            &git,
            &child,
            Some("topic"),
            match &request.command {
                Command::Exec { argv, .. } => argv,
                _ => unreachable!(),
            },
        )
        .expect_err("non-UTF-8 target must be rejected");

        assert_eq!(error.code, ErrorCode::UnsupportedRepositoryLayout);
        assert_eq!(error.exit_code(), 4);
        assert!(child.requests.borrow().is_empty());
    }

    #[test]
    fn rejects_absolute_and_traversing_paths() {
        assert_eq!(
            validate_relative(Path::new("/tmp/file")).unwrap_err().code,
            ErrorCode::AbsolutePathNotAllowed
        );
        for path in ["../secret", "a/../../secret", "."] {
            assert_eq!(
                validate_relative(Path::new(path)).unwrap_err().code,
                ErrorCode::PathOutsideRepo
            );
        }
    }

    #[test]
    fn copy_replaces_files_and_copies_directories() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(repo.join("config/nested")).unwrap();
        fs::create_dir_all(target.join("config")).unwrap();
        fs::write(repo.join("config/nested/value"), "new").unwrap();
        fs::write(target.join("config/old"), "old").unwrap();

        let plan = PlacementPlan::validate(&repo, &target, Path::new("config")).unwrap();
        plan.copy().unwrap();

        assert_eq!(
            fs::read_to_string(target.join("config/nested/value")).unwrap(),
            "new"
        );
        assert!(!target.join("config/old").exists());
    }

    #[test]
    fn source_and_destination_symlink_escapes_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        let outside = directory.path().join("outside");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), "secret").unwrap();
        symlink(outside.join("secret"), repo.join("escaped")).unwrap();
        assert_eq!(
            PlacementPlan::validate(&repo, &target, Path::new("escaped"))
                .unwrap_err()
                .code,
            ErrorCode::PathOutsideRepo
        );

        fs::write(repo.join("safe"), "safe").unwrap();
        symlink(&outside, target.join("nested")).unwrap();
        assert_eq!(
            PlacementPlan::validate(&repo, &target, Path::new("nested/safe"))
                .unwrap_err()
                .code,
            ErrorCode::PathOutsideRepo
        );
    }

    #[test]
    fn link_is_relative_and_points_at_the_validated_source() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("config"), "value").unwrap();

        let plan = PlacementPlan::validate(&repo, &target, Path::new("config")).unwrap();
        plan.link().unwrap();

        let link = fs::read_link(target.join("config")).unwrap();
        assert!(link.is_relative());
        assert_eq!(fs::read_to_string(target.join("config")).unwrap(), "value");

        PlacementPlan::validate(&repo, &target, Path::new("config"))
            .unwrap()
            .link()
            .unwrap();
        PlacementPlan::validate(&repo, &target, Path::new("config"))
            .unwrap()
            .copy()
            .unwrap();
        assert!(
            !fs::symlink_metadata(target.join("config"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(target.join("config")).unwrap(), "value");
    }

    #[test]
    fn copy_preserves_a_symlink_whose_target_stays_inside_the_repository() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("source"), "value").unwrap();
        symlink("source", repo.join("alias")).unwrap();

        let plan = PlacementPlan::validate(&repo, &target, Path::new("alias")).unwrap();
        plan.copy().unwrap();

        assert_eq!(
            fs::read_link(target.join("alias")).unwrap(),
            Path::new("source")
        );
    }

    #[test]
    fn failed_staging_and_failed_install_preserve_existing_destination() {
        use std::os::unix::net::UnixListener;

        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        let source = repo.join("socket");
        let _listener = UnixListener::bind(&source).unwrap();
        fs::write(target.join("socket"), "original").unwrap();

        let plan = PlacementPlan::validate(&repo, &target, Path::new("socket")).unwrap();
        assert!(plan.copy().is_err());
        assert_eq!(
            fs::read_to_string(target.join("socket")).unwrap(),
            "original"
        );
    }

    #[test]
    fn batch_staging_failure_keeps_every_destination_unchanged() {
        use std::os::unix::net::UnixListener;

        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("first"), "new first").unwrap();
        let _listener = UnixListener::bind(repo.join("second")).unwrap();
        fs::write(target.join("first"), "old first").unwrap();
        fs::write(target.join("second"), "old second").unwrap();
        let plans = ["first", "second"]
            .map(|path| PlacementPlan::validate(&repo, &target, Path::new(path)).unwrap());

        let error = execute_placement_batch(&plans, FilePlacement::Copy, &NoopPlacementObserver)
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidArgument, "{error:?}");
        assert_eq!(
            fs::read_to_string(target.join("first")).unwrap(),
            "old first"
        );
        assert_eq!(
            fs::read_to_string(target.join("second")).unwrap(),
            "old second"
        );
        assert!(fs::read_dir(&target).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vde-placement-")
        }));
    }

    struct FailSecondCommit;

    impl PlacementObserver for FailSecondCommit {
        fn before_commit(&self, index: usize, _plan: &PlacementPlan) -> Result<(), CliError> {
            if index == 1 {
                return Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected second commit failure",
                ));
            }
            Ok(())
        }
    }

    #[test]
    fn commit_failure_rolls_back_all_earlier_destinations() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        for name in ["first", "second"] {
            fs::write(repo.join(name), format!("new {name}")).unwrap();
            fs::write(target.join(name), format!("old {name}")).unwrap();
        }
        let plans = ["first", "second"]
            .map(|path| PlacementPlan::validate(&repo, &target, Path::new(path)).unwrap());

        let error =
            execute_placement_batch(&plans, FilePlacement::Copy, &FailSecondCommit).unwrap_err();

        assert_eq!(error.code, ErrorCode::InternalError);
        assert_eq!(
            fs::read_to_string(target.join("first")).unwrap(),
            "old first"
        );
        assert_eq!(
            fs::read_to_string(target.join("second")).unwrap(),
            "old second"
        );
        assert_ne!(error.details.get("rollbackFailed"), Some(&json!(true)));
    }

    struct FailSecondCommitAndRollback;

    impl PlacementObserver for FailSecondCommitAndRollback {
        fn before_commit(&self, index: usize, _plan: &PlacementPlan) -> Result<(), CliError> {
            if index == 1 {
                return Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected second commit failure",
                ));
            }
            Ok(())
        }

        fn before_rollback(&self, index: usize) -> Result<(), String> {
            if index == 0 {
                return Err("injected rollback failure".to_owned());
            }
            Ok(())
        }
    }

    #[test]
    fn rollback_failure_keeps_transaction_backup_and_staged_value_for_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        for name in ["first", "second"] {
            fs::write(repo.join(name), format!("new {name}")).unwrap();
            fs::write(target.join(name), format!("old {name}")).unwrap();
        }
        let plans = ["first", "second"]
            .map(|path| PlacementPlan::validate(&repo, &target, Path::new(path)).unwrap());

        let error =
            execute_placement_batch(&plans, FilePlacement::Copy, &FailSecondCommitAndRollback)
                .unwrap_err();

        assert_eq!(error.details["recoveryRequired"], true);
        assert_eq!(error.details["rollbackFailed"], true);
        assert_eq!(error.details["phase"], "rollback");
        assert_eq!(error.details["committedState"], "partial");
        let recovery = PathBuf::from(error.details["recoveryPath"].as_str().unwrap());
        assert_eq!(
            fs::read_to_string(recovery.join("backup/0")).unwrap(),
            "old first"
        );
        assert_eq!(
            fs::read_to_string(recovery.join("staged/rollback-0")).unwrap(),
            "new first"
        );
        assert_eq!(
            fs::read_to_string(target.join("second")).unwrap(),
            "old second"
        );
    }

    #[test]
    fn shared_missing_destination_parent_is_created_once_for_the_batch() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(repo.join("shared")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("shared/x"), "x").unwrap();
        fs::write(repo.join("shared/y"), "y").unwrap();
        let plans = ["shared/x", "shared/y"]
            .map(|path| PlacementPlan::validate(&repo, &target, Path::new(path)).unwrap());

        execute_placement_batch(&plans, FilePlacement::Copy, &NoopPlacementObserver).unwrap();

        assert_eq!(fs::read_to_string(target.join("shared/x")).unwrap(), "x");
        assert_eq!(fs::read_to_string(target.join("shared/y")).unwrap(), "y");
    }

    #[test]
    fn source_equal_to_destination_is_rejected_without_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("config"), "original").unwrap();

        let error = PlacementPlan::validate(&repo, &repo, Path::new("config")).unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(fs::read_to_string(repo.join("config")).unwrap(), "original");
        assert!(fs::read_dir(&repo).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vde-placement-")
        }));
    }

    struct SwapSourceAfterValidation {
        source: PathBuf,
        original: PathBuf,
        outside: PathBuf,
    }

    impl PlacementObserver for SwapSourceAfterValidation {
        fn after_source_validation(&self, _index: usize, _plan: &PlacementPlan) {
            fs::rename(&self.source, &self.original).unwrap();
            symlink(&self.outside, &self.source).unwrap();
        }
    }

    #[test]
    fn source_swap_after_validation_never_commits_outside_content() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        let outside = directory.path().join("outside-secret");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("source"), "inside").unwrap();
        fs::write(&outside, "outside sentinel").unwrap();
        fs::write(target.join("source"), "old target").unwrap();
        let plan = PlacementPlan::validate(&repo, &target, Path::new("source")).unwrap();
        let observer = SwapSourceAfterValidation {
            source: repo.join("source"),
            original: repo.join("source-original"),
            outside: outside.clone(),
        };

        let error = execute_placement_batch(&[plan], FilePlacement::Copy, &observer).unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert_eq!(
            fs::read_to_string(target.join("source")).unwrap(),
            "old target"
        );
        assert_eq!(fs::read_to_string(outside).unwrap(), "outside sentinel");
    }

    #[test]
    fn source_symlink_swap_after_validation_never_stages_the_raced_raw_target() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        let outside = directory.path().join("outside-secret");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("inside"), "inside").unwrap();
        fs::write(&outside, "outside sentinel").unwrap();
        symlink("inside", repo.join("alias")).unwrap();
        fs::write(target.join("alias"), "old target").unwrap();
        let plan = PlacementPlan::validate(&repo, &target, Path::new("alias")).unwrap();
        let observer = SwapSourceAfterValidation {
            source: repo.join("alias"),
            original: repo.join("alias-original"),
            outside: outside.clone(),
        };

        let error = execute_placement_batch(&[plan], FilePlacement::Copy, &observer).unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert_eq!(
            fs::read_to_string(target.join("alias")).unwrap(),
            "old target"
        );
        assert_eq!(fs::read_to_string(outside).unwrap(), "outside sentinel");
    }

    struct SwapRegularSourceAcrossOpen {
        source: PathBuf,
        original: PathBuf,
        raced: PathBuf,
        raced_after_open: PathBuf,
    }

    impl PlacementObserver for SwapRegularSourceAcrossOpen {
        fn before_source_open(&self, relative: &Path) {
            if relative.as_os_str().is_empty() {
                fs::rename(&self.source, &self.original).unwrap();
                fs::rename(&self.raced, &self.source).unwrap();
            }
        }

        fn after_source_open(&self, relative: &Path) {
            if relative.as_os_str().is_empty() {
                fs::rename(&self.source, &self.raced_after_open).unwrap();
                fs::rename(&self.original, &self.source).unwrap();
            }
        }
    }

    #[test]
    fn regular_source_swapped_only_across_open_never_stages_raced_content() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        let source = repo.join("source");
        let raced = repo.join("raced");
        fs::write(&source, "validated content").unwrap();
        fs::write(&raced, "raced content").unwrap();
        fs::write(target.join("source"), "old target").unwrap();
        let plan = PlacementPlan::validate(&repo, &target, Path::new("source")).unwrap();
        let observer = SwapRegularSourceAcrossOpen {
            source: source.clone(),
            original: repo.join("source-original"),
            raced,
            raced_after_open: repo.join("raced-after-open"),
        };

        let error = execute_placement_batch(&[plan], FilePlacement::Copy, &observer).unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert_eq!(fs::read_to_string(source).unwrap(), "validated content");
        assert_eq!(
            fs::read_to_string(target.join("source")).unwrap(),
            "old target"
        );
        assert_eq!(
            fs::read_to_string(repo.join("raced-after-open")).unwrap(),
            "raced content"
        );
    }

    struct SwapTargetRootAfterSourceValidation {
        target: PathBuf,
        original: PathBuf,
    }

    impl PlacementObserver for SwapTargetRootAfterSourceValidation {
        fn after_source_validation(&self, _index: usize, _plan: &PlacementPlan) {
            fs::rename(&self.target, &self.original).unwrap();
            fs::create_dir(&self.target).unwrap();
        }
    }

    #[test]
    fn target_root_swap_never_stages_or_cleans_up_through_the_replacement_path() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        let original = directory.path().join("target-original");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("value"), "new value").unwrap();
        fs::write(target.join("value"), "old value").unwrap();
        let plan = PlacementPlan::validate(&repo, &target, Path::new("value")).unwrap();

        let error = execute_placement_batch(
            &[plan],
            FilePlacement::Copy,
            &SwapTargetRootAfterSourceValidation {
                target: target.clone(),
                original: original.clone(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert!(fs::read_dir(&target).unwrap().next().is_none());
        assert_eq!(
            fs::read_to_string(original.join("value")).unwrap(),
            "old value"
        );
        assert!(fs::read_dir(&original).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vde-placement-")
        }));
    }

    struct CreateDestinationBeforeMove {
        destination: PathBuf,
    }

    impl PlacementObserver for CreateDestinationBeforeMove {
        fn before_destination_move(&self, _index: usize, _plan: &PlacementPlan) {
            fs::write(&self.destination, "concurrent value").unwrap();
        }
    }

    #[test]
    fn absent_destination_created_after_validation_is_never_clobbered() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("value"), "new value").unwrap();
        let destination = target.join("value");
        let plan = PlacementPlan::validate(&repo, &target, Path::new("value")).unwrap();

        let error = execute_placement_batch(
            &[plan],
            FilePlacement::Copy,
            &CreateDestinationBeforeMove {
                destination: destination.clone(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert_eq!(fs::read_to_string(destination).unwrap(), "concurrent value");
    }

    struct SwapDestinationLeafBeforeMove {
        destination: PathBuf,
        original: PathBuf,
    }

    impl PlacementObserver for SwapDestinationLeafBeforeMove {
        fn before_destination_move(&self, _index: usize, _plan: &PlacementPlan) {
            fs::rename(&self.destination, &self.original).unwrap();
            fs::write(&self.destination, "concurrent value").unwrap();
        }
    }

    #[test]
    fn existing_destination_swapped_before_backup_is_detected_and_restored_without_clobber() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("value"), "new value").unwrap();
        let destination = target.join("value");
        let original = target.join("value-original");
        fs::write(&destination, "old value").unwrap();
        let plan = PlacementPlan::validate(&repo, &target, Path::new("value")).unwrap();

        let error = execute_placement_batch(
            &[plan],
            FilePlacement::Copy,
            &SwapDestinationLeafBeforeMove {
                destination: destination.clone(),
                original: original.clone(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert_eq!(fs::read_to_string(destination).unwrap(), "concurrent value");
        assert_eq!(fs::read_to_string(original).unwrap(), "old value");
        assert_ne!(error.details.get("recoveryRequired"), Some(&json!(true)));
    }

    struct CreateDestinationAfterBackup {
        destination: PathBuf,
    }

    impl PlacementObserver for CreateDestinationAfterBackup {
        fn after_destination_backup(&self, _index: usize, _plan: &PlacementPlan) {
            fs::write(&self.destination, "concurrent value").unwrap();
        }
    }

    #[test]
    fn destination_created_after_verified_backup_preserves_all_values_for_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(repo.join("value"), "new value").unwrap();
        let destination = target.join("value");
        fs::write(&destination, "old value").unwrap();
        let plan = PlacementPlan::validate(&repo, &target, Path::new("value")).unwrap();

        let error = execute_placement_batch(
            &[plan],
            FilePlacement::Copy,
            &CreateDestinationAfterBackup {
                destination: destination.clone(),
            },
        )
        .unwrap_err();

        assert_eq!(error.details["recoveryRequired"], true);
        assert_eq!(fs::read_to_string(destination).unwrap(), "concurrent value");
        let recovery = PathBuf::from(error.details["recoveryPath"].as_str().unwrap());
        assert_eq!(
            fs::read_to_string(recovery.join("backup/0")).unwrap(),
            "old value"
        );
        assert_eq!(
            fs::read_to_string(recovery.join("staged/0")).unwrap(),
            "new value"
        );
    }

    struct SwapDestinationParentAfterValidation {
        parent: PathBuf,
        original: PathBuf,
        outside: PathBuf,
    }

    impl PlacementObserver for SwapDestinationParentAfterValidation {
        fn after_destination_validation(&self, _index: usize, _plan: &PlacementPlan) {
            fs::rename(&self.parent, &self.original).unwrap();
            symlink(&self.outside, &self.parent).unwrap();
        }
    }

    #[test]
    fn destination_parent_swap_after_validation_never_mutates_outside() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        let outside = directory.path().join("outside");
        fs::create_dir_all(repo.join("nested")).unwrap();
        fs::create_dir_all(target.join("nested")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(repo.join("nested/value"), "new value").unwrap();
        fs::write(target.join("nested/value"), "old value").unwrap();
        fs::write(outside.join("value"), "outside sentinel").unwrap();
        let plan = PlacementPlan::validate(&repo, &target, Path::new("nested/value")).unwrap();
        let observer = SwapDestinationParentAfterValidation {
            parent: target.join("nested"),
            original: target.join("nested-original"),
            outside: outside.clone(),
        };

        let error = execute_placement_batch(&[plan], FilePlacement::Copy, &observer).unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert_eq!(
            fs::read_to_string(outside.join("value")).unwrap(),
            "outside sentinel"
        );
        assert_eq!(
            fs::read_to_string(target.join("nested-original/value")).unwrap(),
            "old value"
        );
    }

    struct SwapPlacementTransactionBeforeCleanup;

    impl PlacementObserver for SwapPlacementTransactionBeforeCleanup {
        fn before_transaction_cleanup(&self, target_root: &Path) {
            let transaction = fs::read_dir(target_root)
                .unwrap()
                .map(Result::unwrap)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".vde-placement-")
                })
                .unwrap()
                .path();
            fs::rename(&transaction, target_root.join("placement-recovery")).unwrap();
            fs::create_dir(&transaction).unwrap();
            fs::write(transaction.join("replacement-sentinel"), "untouched\n").unwrap();
        }
    }

    #[test]
    fn transaction_name_swap_preserves_replacement_and_reports_identity_resolved_path() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let target = directory.path().join("target");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(repo.join("value"), "new value\n").unwrap();
        let plan = PlacementPlan::validate(&repo, &target, Path::new("value")).unwrap();

        let partial = execute_placement_batch(
            &[plan],
            FilePlacement::Copy,
            &SwapPlacementTransactionBeforeCleanup,
        )
        .unwrap()
        .expect("the renamed transaction must be reported for recovery");

        assert_eq!(partial.details["committed"], true);
        assert_eq!(partial.details["recoveryRequired"], true);
        assert_eq!(
            partial.details["recoveryPath"],
            json!(target.canonicalize().unwrap().join("placement-recovery"))
        );
        assert_eq!(
            fs::read_to_string(target.join("value")).unwrap(),
            "new value\n"
        );
        let replacement = fs::read_dir(&target)
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vde-placement-")
            })
            .unwrap()
            .path();
        assert_eq!(
            fs::read_to_string(replacement.join("replacement-sentinel")).unwrap(),
            "untouched\n"
        );
    }

    struct FailPlacementInitializationAt(PlacementInitializationPoint);

    impl PlacementInitializationObserver for FailPlacementInitializationAt {
        fn checkpoint(&self, point: PlacementInitializationPoint) -> Result<(), CliError> {
            if point == self.0 {
                Err(CliError::new(
                    ErrorCode::InternalError,
                    format!("injected placement initialization failure at {point:?}"),
                ))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn every_placement_initialization_failure_removes_the_hidden_transaction() {
        for point in [
            PlacementInitializationPoint::DirectoryCreated,
            PlacementInitializationPoint::DirectoryOpened,
            PlacementInitializationPoint::IdentityVerified,
            PlacementInitializationPoint::StagedCreated,
            PlacementInitializationPoint::BackupCreated,
            PlacementInitializationPoint::StagedOpened,
            PlacementInitializationPoint::BackupOpened,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let target = directory.path().join("target");
            fs::create_dir(&target).unwrap();

            let error = PlacementTransaction::create_with_observer(
                &target,
                &FailPlacementInitializationAt(point),
            )
            .err()
            .unwrap();

            assert_eq!(error.code, ErrorCode::InternalError, "point={point:?}");
            assert!(
                fs::read_dir(&target).unwrap().all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".vde-placement-")
                }),
                "point={point:?}"
            );
            assert_eq!(error.details.get("transactionCleanupFailed"), None);
        }
    }

    struct SwapPlacementInitializationEntry {
        target: PathBuf,
    }

    impl PlacementInitializationObserver for SwapPlacementInitializationEntry {
        fn checkpoint(&self, point: PlacementInitializationPoint) -> Result<(), CliError> {
            if point == PlacementInitializationPoint::DirectoryCreated {
                let transaction = fs::read_dir(&self.target)
                    .unwrap()
                    .map(Result::unwrap)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".vde-placement-")
                    })
                    .unwrap()
                    .path();
                fs::rename(&transaction, self.target.join("original-transaction")).unwrap();
                fs::create_dir(&transaction).unwrap();
                fs::write(transaction.join("sentinel"), "replacement\n").unwrap();
                return Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected replacement race",
                ));
            }
            Ok(())
        }
    }

    #[test]
    fn placement_initialization_cleanup_never_deletes_a_replacement_entry() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        fs::create_dir(&target).unwrap();

        let error = PlacementTransaction::create_with_observer(
            &target,
            &SwapPlacementInitializationEntry {
                target: target.clone(),
            },
        )
        .err()
        .unwrap();

        assert_eq!(error.details["transactionCleanupFailed"], true);
        let replacement = fs::read_dir(&target)
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vde-placement-")
            })
            .unwrap()
            .path();
        assert_eq!(
            fs::read_to_string(replacement.join("sentinel")).unwrap(),
            "replacement\n"
        );
        assert!(target.join("original-transaction").is_dir());
    }

    #[test]
    fn candidate_sanitization_preserves_single_tsv_record() {
        assert_eq!(sanitize("feature/a\tbad\nline"), "feature/a bad line");
    }
}
