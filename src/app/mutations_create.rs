use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::adapters::git_cli::GitCli;
use crate::app::error_mapper::MapToCliError;
use crate::app::misc_commands::copy_worktree_include_paths;
use crate::app::target::WorktreeIdentity;
use crate::domain::error::{CliError, ErrorCode, ExecutionPhase, ExecutionReport, ExecutionState};
use crate::domain::path::{ValidatedManagedPath, ValidatedPathOperationError};
use crate::domain::repo::RepoContext;
use crate::domain::worktree::WorktreeSnapshot;
use crate::ports::process::{ProcessOutput, ProcessRunner};
use crate::state::lifecycle::merge_lifecycle_observation;

const EXCLUDE_MARKER: &str = "# vde-worktree (managed)";
const WORKTREE_INCLUDE_PATH: &str = ".worktreeinclude";
const POST_NEW_HOOK: &str = "#!/usr/bin/env bash\nset -eu\n\n# example:\n#   vde-worktree copy .envrc .claude/settings.local.json\n\nexit 0\n";
const POST_SWITCH_HOOK: &str =
    "#!/usr/bin/env bash\nset -eu\n\n# example:\n#   vde-worktree link .envrc\n\nexit 0\n";

/// Git operations required by the create/connect mutation plans.
///
/// The unchecked operation is reserved for probes with meaningful non-zero
/// statuses. All actual mutations use `run_git_checked`.
pub trait CreateMutationGit {
    fn run_git(&self, cwd: &Path, args: &[OsString]) -> Result<ProcessOutput, CliError>;

    fn run_git_checked(&self, cwd: &Path, args: &[OsString]) -> Result<ProcessOutput, CliError>;
}

impl<R> CreateMutationGit for GitCli<R>
where
    R: ProcessRunner,
{
    fn run_git(&self, cwd: &Path, args: &[OsString]) -> Result<ProcessOutput, CliError> {
        self.execute(cwd, args)
            .map_err(MapToCliError::map_to_cli_error)
    }

    fn run_git_checked(&self, cwd: &Path, args: &[OsString]) -> Result<ProcessOutput, CliError> {
        self.execute_checked(cwd, args)
            .map_err(MapToCliError::map_to_cli_error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationHookTarget {
    pub branch: Option<String>,
    pub worktree_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitPlan {
    pub repo_root: PathBuf,
    pub managed_worktree_root: PathBuf,
    pub metadata_directories: Vec<PathBuf>,
    pub exclude_path: PathBuf,
    pub exclude_block: Option<String>,
    pub already_initialized: bool,
}

impl InitPlan {
    pub fn hook_target(&self) -> MutationHookTarget {
        MutationHookTarget {
            branch: None,
            worktree_path: self.repo_root.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitResult {
    pub already_initialized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchCreationMode {
    NewBranch,
    ExistingLocalBranch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPlan {
    pub repo_root: PathBuf,
    pub managed_worktree_root: PathBuf,
    pub branch: String,
    pub target_path: PathBuf,
    pub base_branch: String,
}

impl NewPlan {
    pub fn hook_target(&self) -> MutationHookTarget {
        MutationHookTarget {
            branch: Some(self.branch.clone()),
            worktree_path: self.target_path.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchPlan {
    Existing {
        repo_root: PathBuf,
        branch: String,
        path: PathBuf,
        base_branch: String,
    },
    Create {
        repo_root: PathBuf,
        managed_worktree_root: PathBuf,
        branch: String,
        target_path: PathBuf,
        base_branch: String,
        mode: BranchCreationMode,
    },
}

impl SwitchPlan {
    pub fn hook_target(&self) -> MutationHookTarget {
        match self {
            Self::Existing { branch, path, .. } => MutationHookTarget {
                branch: Some(branch.clone()),
                worktree_path: path.clone(),
            },
            Self::Create {
                branch,
                target_path,
                ..
            } => MutationHookTarget {
                branch: Some(branch.clone()),
                worktree_path: target_path.clone(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorktreeDisposition {
    Created,
    Existing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeMutationResult {
    pub branch: String,
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disposition: Option<WorktreeDisposition>,
}

/// A successful Git/filesystem transition whose lifecycle state is not durable yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeGitApplied {
    repo_root: PathBuf,
    managed_worktree_root: Option<PathBuf>,
    branch: String,
    path: PathBuf,
    base_branch: String,
    disposition: Option<WorktreeDisposition>,
    created_branch_oid: Option<String>,
    remove_worktree_on_state_failure: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GetPlan {
    Existing {
        repo_root: PathBuf,
        remote: String,
        branch: String,
        path: PathBuf,
        base_branch: String,
    },
    Create {
        repo_root: PathBuf,
        managed_worktree_root: PathBuf,
        remote: String,
        branch: String,
        target_path: PathBuf,
        base_branch: String,
        mode: BranchCreationMode,
    },
}

impl GetPlan {
    pub fn hook_target(&self) -> MutationHookTarget {
        match self {
            Self::Existing { branch, path, .. } => MutationHookTarget {
                branch: Some(branch.clone()),
                worktree_path: path.clone(),
            },
            Self::Create {
                branch,
                target_path,
                ..
            } => MutationHookTarget {
                branch: Some(branch.clone()),
                worktree_path: target_path.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptCandidate {
    pub branch: String,
    pub from_path: PathBuf,
    pub to_path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptSkippedReason {
    Detached,
    Locked,
    TargetExists,
    TargetConflict,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptSkipped {
    pub branch: Option<String>,
    pub from_path: PathBuf,
    pub to_path: Option<PathBuf>,
    pub reason: AdoptSkippedReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptFailed {
    pub details: BTreeMap<String, Value>,
    pub execution: ExecutionReport,
    pub branch: String,
    pub from_path: PathBuf,
    pub to_path: PathBuf,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdoptPlan {
    pub repo_root: PathBuf,
    pub managed_worktree_root: PathBuf,
    pub dry_run: bool,
    pub candidates: Vec<AdoptCandidate>,
    pub skipped: Vec<AdoptSkipped>,
}

impl AdoptPlan {
    pub fn hook_target(&self) -> MutationHookTarget {
        MutationHookTarget {
            branch: None,
            worktree_path: self.managed_worktree_root.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptResult {
    pub dry_run: bool,
    pub managed_worktree_root: PathBuf,
    pub candidates: Vec<AdoptCandidate>,
    pub moved: Vec<AdoptCandidate>,
    pub skipped: Vec<AdoptSkipped>,
    pub failed: Vec<AdoptFailed>,
}

impl AdoptResult {
    pub const fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }
}

pub fn resolve_managed_worktree_root(repo_root: &Path, configured_root: &str) -> PathBuf {
    let configured = Path::new(configured_root);
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        repo_root.join(configured)
    }
}

pub fn prepare_init(
    context: &RepoContext,
    managed_worktree_root: PathBuf,
) -> Result<InitPlan, CliError> {
    let metadata_root = context.repo_root.join(".vde/worktree");
    let already_initialized = match fs::metadata(&metadata_root) {
        Ok(metadata) => metadata.is_dir(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(io_error(&metadata_root, &error)),
    };
    let exclude_path = context.git_common_dir.join("info/exclude");
    let exclude_block = managed_root_exclude_entry(&context.repo_root, &managed_worktree_root)
        .map(|entry| format!("{EXCLUDE_MARKER}\n{entry}\n.vde/worktree/\n"));
    Ok(InitPlan {
        repo_root: context.repo_root.clone(),
        managed_worktree_root,
        metadata_directories: vec![
            metadata_root.join("hooks"),
            metadata_root.join("logs"),
            metadata_root.join("locks"),
            metadata_root.join("state"),
        ],
        exclude_path,
        exclude_block,
        already_initialized,
    })
}

pub fn apply_init(plan: &InitPlan) -> Result<InitResult, CliError> {
    fs::create_dir_all(&plan.managed_worktree_root)
        .map_err(|error| io_error(&plan.managed_worktree_root, &error))?;
    for directory in &plan.metadata_directories {
        fs::create_dir_all(directory).map_err(|error| io_error(directory, &error))?;
    }
    if let Some(block) = &plan.exclude_block {
        ensure_exclude_block(&plan.exclude_path, block)?;
    }
    let hooks_directory = plan.repo_root.join(".vde/worktree/hooks");
    create_hook_template(&hooks_directory.join("post-new"), POST_NEW_HOOK)?;
    create_hook_template(&hooks_directory.join("post-switch"), POST_SWITCH_HOOK)?;
    Ok(InitResult {
        already_initialized: plan.already_initialized,
    })
}

pub fn prepare_new<G: CreateMutationGit, T: WorktreeIdentity>(
    git: &G,
    repo_root: &Path,
    managed_worktree_root: &Path,
    worktrees: &[T],
    branch: &str,
    base_branch: &str,
) -> Result<NewPlan, CliError> {
    validate_branch(git, repo_root, branch)?;
    if worktrees
        .iter()
        .any(|worktree| worktree.branch() == Some(branch))
    {
        return Err(branch_already_attached(branch));
    }
    if local_branch_exists(git, repo_root, branch)? {
        return Err(branch_already_exists(branch));
    }
    let target_path = validate_target_path(managed_worktree_root, branch)?;
    ensure_target_writable(&target_path)?;
    Ok(NewPlan {
        repo_root: repo_root.to_path_buf(),
        managed_worktree_root: managed_worktree_root.to_path_buf(),
        branch: branch.to_owned(),
        target_path,
        base_branch: base_branch.to_owned(),
    })
}

pub fn apply_new_git<G: CreateMutationGit>(
    git: &G,
    plan: &NewPlan,
) -> Result<WorktreeGitApplied, CliError> {
    revalidate_new_branch(git, &plan.repo_root, &plan.branch)?;
    let target =
        revalidate_writable_target(&plan.managed_worktree_root, &plan.branch, &plan.target_path)?;
    create_parent(&target)?;
    let args = os_args([
        "worktree",
        "add",
        "-b",
        &plan.branch,
        target_string(&target)?.as_str(),
        &plan.base_branch,
    ]);
    if let Err(error) = git.run_git_checked(&plan.repo_root, &args) {
        // `git worktree add -b` may fail because an independently-created branch won the race.
        // Its failure result does not prove ownership, so never delete the resulting ref here.
        prune_empty_target_parents(&target, &plan.managed_worktree_root);
        return Err(error);
    }
    let created_branch_oid = resolve_local_branch_oid(git, &plan.repo_root, &plan.branch)?;
    Ok(WorktreeGitApplied {
        repo_root: plan.repo_root.clone(),
        managed_worktree_root: Some(plan.managed_worktree_root.clone()),
        branch: plan.branch.clone(),
        path: target,
        disposition: None,
        base_branch: plan.base_branch.clone(),
        created_branch_oid: Some(created_branch_oid),
        remove_worktree_on_state_failure: true,
    })
}

pub fn prepare_switch<G: CreateMutationGit, T: WorktreeIdentity>(
    git: &G,
    repo_root: &Path,
    managed_worktree_root: &Path,
    worktrees: &[T],
    branch: &str,
    base_branch: &str,
) -> Result<SwitchPlan, CliError> {
    validate_branch(git, repo_root, branch)?;
    if let Some(existing) = crate::app::target::optional_branch(worktrees, branch)? {
        return Ok(SwitchPlan::Existing {
            repo_root: repo_root.to_path_buf(),
            branch: branch.to_owned(),
            path: existing.path().to_path_buf(),
            base_branch: base_branch.to_owned(),
        });
    }
    let target_path = validate_target_path(managed_worktree_root, branch)?;
    ensure_target_writable(&target_path)?;
    let mode = if local_branch_exists(git, repo_root, branch)? {
        BranchCreationMode::ExistingLocalBranch
    } else {
        BranchCreationMode::NewBranch
    };
    Ok(SwitchPlan::Create {
        repo_root: repo_root.to_path_buf(),
        managed_worktree_root: managed_worktree_root.to_path_buf(),
        branch: branch.to_owned(),
        target_path,
        base_branch: base_branch.to_owned(),
        mode,
    })
}

pub fn apply_switch_git<G: CreateMutationGit>(
    git: &G,
    plan: &SwitchPlan,
) -> Result<WorktreeGitApplied, CliError> {
    match plan {
        SwitchPlan::Existing {
            repo_root,
            branch,
            path,
            base_branch,
        } => {
            revalidate_existing_attachment(git, repo_root, branch, path)?;
            Ok(WorktreeGitApplied {
                repo_root: repo_root.clone(),
                managed_worktree_root: None,
                branch: branch.clone(),
                path: path.clone(),
                disposition: Some(WorktreeDisposition::Existing),
                base_branch: base_branch.clone(),
                created_branch_oid: None,
                remove_worktree_on_state_failure: false,
            })
        }
        SwitchPlan::Create {
            repo_root,
            managed_worktree_root,
            branch,
            target_path,
            base_branch,
            mode,
        } => {
            revalidate_branch_mode(git, repo_root, branch, *mode)?;
            let target = revalidate_writable_target(managed_worktree_root, branch, target_path)?;
            create_parent(&target)?;
            let target_text = target_string(&target)?;
            let args = match mode {
                BranchCreationMode::NewBranch => {
                    os_args(["worktree", "add", "-b", branch, &target_text, base_branch])
                }
                BranchCreationMode::ExistingLocalBranch => {
                    os_args(["worktree", "add", &target_text, branch])
                }
            };
            if let Err(error) = git.run_git_checked(repo_root, &args) {
                // A failed `worktree add -b` does not prove that any observed branch was ours.
                prune_empty_target_parents(&target, managed_worktree_root);
                return Err(error);
            }
            let created_branch_oid = (*mode == BranchCreationMode::NewBranch)
                .then(|| resolve_local_branch_oid(git, repo_root, branch))
                .transpose()?;
            Ok(WorktreeGitApplied {
                repo_root: repo_root.clone(),
                managed_worktree_root: Some(managed_worktree_root.clone()),
                branch: branch.clone(),
                path: target,
                disposition: Some(WorktreeDisposition::Created),
                base_branch: base_branch.clone(),
                created_branch_oid,
                remove_worktree_on_state_failure: true,
            })
        }
    }
}

pub fn parse_remote_branch(value: &str) -> Result<(String, String), CliError> {
    let Some((remote, branch)) = value.split_once('/') else {
        return Err(invalid_remote_branch(value));
    };
    if remote.is_empty() || branch.is_empty() {
        return Err(invalid_remote_branch(value));
    }
    Ok((remote.to_owned(), branch.to_owned()))
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_get<G: CreateMutationGit, T: WorktreeIdentity>(
    git: &G,
    repo_root: &Path,
    managed_worktree_root: &Path,
    worktrees: &[T],
    remote_branch: &str,
    base_branch: &str,
) -> Result<GetPlan, CliError> {
    let (remote, branch) = parse_remote_branch(remote_branch)?;
    validate_branch(git, repo_root, &branch)?;
    validate_remote_exists(git, repo_root, &remote)?;
    validate_remote_branch_exists(git, repo_root, &remote, &branch)?;
    if let Some(existing) = crate::app::target::optional_branch(worktrees, &branch)? {
        return Ok(GetPlan::Existing {
            repo_root: repo_root.to_path_buf(),
            remote,
            branch,
            path: existing.path().to_path_buf(),
            base_branch: base_branch.to_owned(),
        });
    }
    let target_path = validate_target_path(managed_worktree_root, &branch)?;
    ensure_target_writable(&target_path)?;
    let mode = if local_branch_exists(git, repo_root, &branch)? {
        BranchCreationMode::ExistingLocalBranch
    } else {
        BranchCreationMode::NewBranch
    };
    Ok(GetPlan::Create {
        repo_root: repo_root.to_path_buf(),
        managed_worktree_root: managed_worktree_root.to_path_buf(),
        remote,
        branch,
        target_path,
        base_branch: base_branch.to_owned(),
        mode,
    })
}

pub fn apply_get_git<G: CreateMutationGit>(
    git: &G,
    plan: &GetPlan,
) -> Result<WorktreeGitApplied, CliError> {
    match plan {
        GetPlan::Existing {
            repo_root,
            remote,
            branch,
            path,
            base_branch,
        } => {
            revalidate_existing_attachment(git, repo_root, branch, path)?;
            fetch_remote_branch(git, repo_root, remote, branch)?;
            Ok(WorktreeGitApplied {
                repo_root: repo_root.clone(),
                managed_worktree_root: None,
                branch: branch.clone(),
                path: path.clone(),
                disposition: Some(WorktreeDisposition::Existing),
                base_branch: base_branch.clone(),
                created_branch_oid: None,
                remove_worktree_on_state_failure: false,
            })
        }
        GetPlan::Create {
            repo_root,
            managed_worktree_root,
            remote,
            branch,
            target_path,
            base_branch,
            mode,
        } => {
            revalidate_branch_mode(git, repo_root, branch, *mode)?;
            let target = revalidate_writable_target(managed_worktree_root, branch, target_path)?;
            fetch_remote_branch(git, repo_root, remote, branch)?;
            let target_text = target_string(&target)?;
            let created_branch = *mode == BranchCreationMode::NewBranch;
            if created_branch {
                git.run_git_checked(
                    repo_root,
                    &os_args(["branch", "--track", branch, &format!("{remote}/{branch}")]),
                )?;
            }
            let created_branch_oid = created_branch
                .then(|| resolve_local_branch_oid(git, repo_root, branch))
                .transpose()?;
            if let Err(error) = create_parent(&target) {
                if let Some(oid) = created_branch_oid.as_deref() {
                    rollback_local_branch_if_unchanged(git, repo_root, branch, oid);
                }
                prune_empty_target_parents(&target, managed_worktree_root);
                return Err(error);
            }
            let add_result = git.run_git_checked(
                repo_root,
                &os_args(["worktree", "add", &target_text, branch]),
            );
            if let Err(error) = add_result {
                if let Some(oid) = created_branch_oid.as_deref() {
                    rollback_local_branch_if_unchanged(git, repo_root, branch, oid);
                }
                prune_empty_target_parents(&target, managed_worktree_root);
                return Err(error);
            }
            Ok(WorktreeGitApplied {
                repo_root: repo_root.clone(),
                managed_worktree_root: Some(managed_worktree_root.clone()),
                branch: branch.clone(),
                path: target,
                disposition: Some(WorktreeDisposition::Created),
                base_branch: base_branch.clone(),
                created_branch_oid,
                remove_worktree_on_state_failure: true,
            })
        }
    }
}

/// Persists lifecycle state after the Git phase. If state persistence fails for a newly attached
/// worktree, the worktree is removed and an app-created branch is deleted only when its OID is
/// still exactly the one captured after creation.
pub fn finalize_worktree_state<G: CreateMutationGit>(
    git: &G,
    applied: WorktreeGitApplied,
) -> Result<WorktreeMutationResult, CliError> {
    if applied.remove_worktree_on_state_failure {
        let managed_root = applied
            .managed_worktree_root
            .as_deref()
            .expect("created worktree always records its managed root");
        if let Err(error) =
            copy_worktree_include(git, &applied.repo_root, managed_root, &applied.path)
        {
            return Err(rollback_created_worktree(
                git,
                &applied.repo_root,
                &applied.path,
                managed_root,
                &applied.branch,
                applied.created_branch_oid.as_deref(),
                error,
            ));
        }
    }
    if let Err(error) = merge_lifecycle_observation(
        &applied.repo_root,
        &applied.branch,
        &applied.base_branch,
        None,
    )
    .map_err(MapToCliError::map_to_cli_error)
    {
        if applied.remove_worktree_on_state_failure {
            let managed_root = applied
                .managed_worktree_root
                .as_deref()
                .expect("created worktree always records its managed root");
            return Err(rollback_created_worktree(
                git,
                &applied.repo_root,
                &applied.path,
                managed_root,
                &applied.branch,
                applied.created_branch_oid.as_deref(),
                error,
            ));
        }
        return Err(error);
    }
    Ok(WorktreeMutationResult {
        branch: applied.branch,
        path: applied.path,
        disposition: applied.disposition,
    })
}

fn copy_worktree_include<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    managed_root: &Path,
    target_root: &Path,
) -> Result<(), CliError> {
    let include_path = repo_root.join(WORKTREE_INCLUDE_PATH);
    let result = (|| {
        let metadata = match fs::symlink_metadata(&include_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(&include_path, &error)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(CliError::new(
                ErrorCode::InvalidArgument,
                ".worktreeinclude must be a regular file in the repository root",
            ));
        }

        let included_args = os_args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-from=.worktreeinclude",
            "-z",
            "--",
        ]);
        let included = git
            .run_git_checked(repo_root, &included_args)
            .map(|output| paths_from_nul(&output.stdout))?;
        let ignored_args = os_args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
            "--",
        ]);
        let ignored = git
            .run_git_checked(repo_root, &ignored_args)
            .map(|output| paths_from_nul(&output.stdout))?;

        let mut paths = Vec::new();
        for relative in included.intersection(&ignored) {
            let source = repo_root.join(relative);
            if source.starts_with(managed_root)
                || source.starts_with(repo_root.join(".vde/worktree"))
            {
                continue;
            }
            match fs::symlink_metadata(&source) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    paths.push(relative.clone());
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(&source, &error)),
            }
        }
        copy_worktree_include_paths(repo_root, target_root, &paths)
    })();
    result.map_err(|mut error| {
        error.details.insert(
            "worktreeIncludePath".to_owned(),
            json!(include_path.to_string_lossy()),
        );
        error
    })
}

fn paths_from_nul(output: &[u8]) -> BTreeSet<PathBuf> {
    output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(path_from_bytes)
        .collect()
}

pub fn prepare_adopt(
    repo_root: &Path,
    managed_worktree_root: &Path,
    snapshot: &WorktreeSnapshot,
    dry_run: bool,
) -> Result<AdoptPlan, CliError> {
    let mut worktrees = snapshot.worktrees.iter().collect::<Vec<_>>();
    worktrees.sort_by(|left, right| left.path.cmp(&right.path));
    let mut unresolved = Vec::new();
    let mut skipped = Vec::new();
    for worktree in worktrees {
        if worktree.path == repo_root || path_is_managed(&worktree.path, managed_worktree_root) {
            continue;
        }
        let Some(branch) = worktree.branch.as_deref() else {
            skipped.push(AdoptSkipped {
                branch: None,
                from_path: worktree.path.clone(),
                to_path: None,
                reason: AdoptSkippedReason::Detached,
            });
            continue;
        };
        if worktree.locked.value {
            skipped.push(AdoptSkipped {
                branch: Some(branch.to_owned()),
                from_path: worktree.path.clone(),
                to_path: None,
                reason: AdoptSkippedReason::Locked,
            });
            continue;
        }
        unresolved.push(AdoptCandidate {
            branch: branch.to_owned(),
            from_path: worktree.path.clone(),
            to_path: validate_target_path(managed_worktree_root, branch)?,
        });
    }

    let mut reserved = BTreeSet::new();
    let mut candidates = Vec::new();
    for candidate in unresolved {
        if path_exists(&candidate.to_path)? {
            skipped.push(AdoptSkipped {
                branch: Some(candidate.branch),
                from_path: candidate.from_path,
                to_path: Some(candidate.to_path),
                reason: AdoptSkippedReason::TargetExists,
            });
        } else if !reserved.insert(candidate.to_path.clone()) {
            skipped.push(AdoptSkipped {
                branch: Some(candidate.branch),
                from_path: candidate.from_path,
                to_path: Some(candidate.to_path),
                reason: AdoptSkippedReason::TargetConflict,
            });
        } else {
            candidates.push(candidate);
        }
    }
    Ok(AdoptPlan {
        repo_root: repo_root.to_path_buf(),
        managed_worktree_root: managed_worktree_root.to_path_buf(),
        dry_run,
        candidates,
        skipped,
    })
}

pub fn apply_adopt<G: CreateMutationGit>(
    git: &G,
    plan: &AdoptPlan,
) -> Result<AdoptResult, CliError> {
    if plan.dry_run {
        return Ok(AdoptResult {
            dry_run: true,
            managed_worktree_root: plan.managed_worktree_root.clone(),
            candidates: plan.candidates.clone(),
            moved: Vec::new(),
            skipped: plan.skipped.clone(),
            failed: Vec::new(),
        });
    }
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    for candidate in &plan.candidates {
        let result =
            apply_adopt_candidate(git, &plan.repo_root, &plan.managed_worktree_root, candidate)
                .map_err(|error| {
                    error.at_phase(ExecutionPhase::Apply, ExecutionState::Unknown, &[])
                });
        match result {
            Ok(()) => moved.push(candidate.clone()),
            Err(error) => failed.push(AdoptFailed {
                branch: candidate.branch.clone(),
                from_path: candidate.from_path.clone(),
                to_path: candidate.to_path.clone(),
                code: error.code.to_string(),
                message: error.message,
                details: error.details,
                execution: error.execution,
            }),
        }
    }
    Ok(AdoptResult {
        dry_run: false,
        managed_worktree_root: plan.managed_worktree_root.clone(),
        candidates: plan.candidates.clone(),
        moved,
        skipped: plan.skipped.clone(),
        failed,
    })
}

fn apply_adopt_candidate<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    managed_worktree_root: &Path,
    candidate: &AdoptCandidate,
) -> Result<(), CliError> {
    revalidate_existing_attachment(git, repo_root, &candidate.branch, &candidate.from_path)
        .map_err(|error| {
            error.at_phase(ExecutionPhase::Preflight, ExecutionState::NotStarted, &[])
        })?;
    let target =
        revalidate_absent_target(managed_worktree_root, &candidate.branch, &candidate.to_path)
            .map_err(|error| {
                error.at_phase(ExecutionPhase::Preflight, ExecutionState::NotStarted, &[])
            })?;
    create_parent(&target)?;
    git.run_git_checked(
        repo_root,
        &os_args([
            "worktree",
            "move",
            &target_string(&candidate.from_path)?,
            &target_string(&target)?,
        ]),
    )?;
    Ok(())
}

fn validate_branch<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
) -> Result<(), CliError> {
    if branch.is_empty() || branch.chars().any(char::is_control) {
        return Err(invalid_branch(branch));
    }
    let args = os_args(["check-ref-format", "--branch", branch]);
    let output = git.run_git(repo_root, &args)?;
    if output.timed_out {
        return Err(git_output_error(repo_root, &args, &output));
    }
    if output.exit_code == Some(0) {
        Ok(())
    } else {
        Err(invalid_branch(branch))
    }
}

fn local_branch_exists<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
) -> Result<bool, CliError> {
    let args = os_args([
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ]);
    expected_probe(git, repo_root, &args)
}

fn expected_probe<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    args: &[OsString],
) -> Result<bool, CliError> {
    let output = git.run_git(repo_root, args)?;
    if output.timed_out {
        return Err(git_output_error(repo_root, args, &output));
    }
    match output.exit_code {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(git_output_error(repo_root, args, &output)),
    }
}

fn validate_remote_exists<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    remote: &str,
) -> Result<(), CliError> {
    let args = os_args(["remote", "get-url", remote]);
    let output = git.run_git(repo_root, &args)?;
    if !output.timed_out && output.exit_code == Some(0) {
        return Ok(());
    }
    if output.timed_out {
        return Err(git_output_error(repo_root, &args, &output));
    }
    Err(CliError::new(
        ErrorCode::RemoteNotFound,
        format!("remote not found: {remote}"),
    )
    .with_details(BTreeMap::from([("remote".to_owned(), json!(remote))])))
}

fn validate_remote_branch_exists<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    remote: &str,
    branch: &str,
) -> Result<(), CliError> {
    let remote_ref = format!("refs/heads/{branch}");
    let args = os_args(["ls-remote", "--exit-code", "--heads", remote, &remote_ref]);
    let output = git.run_git(repo_root, &args)?;
    if !output.timed_out && output.exit_code == Some(0) && !output.stdout.is_empty() {
        return Ok(());
    }
    if output.timed_out || !matches!(output.exit_code, Some(0 | 2)) {
        return Err(git_output_error(repo_root, &args, &output));
    }
    Err(CliError::new(
        ErrorCode::RemoteBranchNotFound,
        format!("remote branch not found: {remote}/{branch}"),
    )
    .with_details(BTreeMap::from([
        ("remote".to_owned(), json!(remote)),
        ("branch".to_owned(), json!(branch)),
    ])))
}

fn fetch_remote_branch<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    remote: &str,
    branch: &str,
) -> Result<(), CliError> {
    let refspec = format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    git.run_git_checked(repo_root, &os_args(["fetch", remote, &refspec]))?;
    Ok(())
}

fn revalidate_new_branch<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
) -> Result<(), CliError> {
    if attached_worktree_path(git, repo_root, branch)?.is_some() {
        return Err(branch_already_attached(branch));
    }
    if local_branch_exists(git, repo_root, branch)? {
        return Err(branch_already_exists(branch));
    }
    Ok(())
}

fn revalidate_branch_mode<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
    mode: BranchCreationMode,
) -> Result<(), CliError> {
    if attached_worktree_path(git, repo_root, branch)?.is_some() {
        return Err(branch_already_attached(branch));
    }
    let exists = local_branch_exists(git, repo_root, branch)?;
    match (mode, exists) {
        (BranchCreationMode::NewBranch, false)
        | (BranchCreationMode::ExistingLocalBranch, true) => Ok(()),
        (BranchCreationMode::NewBranch, true) => Err(branch_already_exists(branch)),
        (BranchCreationMode::ExistingLocalBranch, false) => Err(CliError::new(
            ErrorCode::WorktreeNotFound,
            format!("local branch disappeared after preflight: {branch}"),
        )),
    }
}

fn revalidate_existing_attachment<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
    expected_path: &Path,
) -> Result<(), CliError> {
    match attached_worktree_path(git, repo_root, branch)? {
        Some(actual) if paths_refer_to_same_location(&actual, expected_path) => Ok(()),
        Some(actual) => Err(CliError::new(
            ErrorCode::BranchInUse,
            format!("branch attachment changed after preflight: {branch}"),
        )
        .with_details(BTreeMap::from([
            ("branch".to_owned(), json!(branch)),
            ("expectedPath".to_owned(), json!(expected_path)),
            ("actualPath".to_owned(), json!(actual)),
        ]))),
        None => Err(CliError::new(
            ErrorCode::WorktreeNotFound,
            format!("worktree disappeared after preflight: {branch}"),
        )
        .with_details(BTreeMap::from([("branch".to_owned(), json!(branch))]))),
    }
}

fn attached_worktree_path<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
) -> Result<Option<PathBuf>, CliError> {
    let args = os_args(["worktree", "list", "--porcelain", "-z"]);
    let output = git.run_git_checked(repo_root, &args)?;
    let expected_ref = format!("refs/heads/{branch}").into_bytes();
    let mut current_path = None;
    for field in output.stdout.split(|byte| *byte == 0) {
        if let Some(path) = field.strip_prefix(b"worktree ") {
            current_path = Some(path_from_bytes(path));
        } else if let Some(branch_ref) = field.strip_prefix(b"branch ")
            && branch_ref == expected_ref
        {
            return Ok(current_path);
        }
    }
    Ok(None)
}

fn validate_target_path(managed_root: &Path, branch: &str) -> Result<PathBuf, CliError> {
    let validated = ValidatedManagedPath::validate(managed_root, Path::new(branch))
        .map_err(MapToCliError::map_to_cli_error)?;
    validated
        .with_revalidated_path(|path| Ok::<PathBuf, io::Error>(path.to_path_buf()))
        .map_err(map_validated_path_error)
}

fn revalidate_writable_target(
    managed_root: &Path,
    branch: &str,
    expected_path: &Path,
) -> Result<PathBuf, CliError> {
    let target = validate_target_path(managed_root, branch)?;
    if target != expected_path {
        return Err(target_changed(expected_path, &target));
    }
    ensure_target_writable(&target)?;
    Ok(target)
}

fn revalidate_absent_target(
    managed_root: &Path,
    branch: &str,
    expected_path: &Path,
) -> Result<PathBuf, CliError> {
    let target = validate_target_path(managed_root, branch)?;
    if target != expected_path {
        return Err(target_changed(expected_path, &target));
    }
    if path_exists(&target)? {
        return Err(target_not_empty(&target));
    }
    Ok(target)
}

fn map_validated_path_error(error: ValidatedPathOperationError<io::Error>) -> CliError {
    match error {
        ValidatedPathOperationError::Containment(error) => error.map_to_cli_error(),
        ValidatedPathOperationError::Operation(error) => {
            CliError::new(ErrorCode::InternalError, error.to_string())
        }
    }
}

fn ensure_target_writable(path: &Path) -> Result<(), CliError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, &error)),
        Ok(metadata) if metadata.is_dir() => {
            let mut entries = fs::read_dir(path).map_err(|error| io_error(path, &error))?;
            if entries.next().is_none() {
                Ok(())
            } else {
                Err(target_not_empty(path))
            }
        }
        Ok(_) => Err(target_not_empty(path)),
    }
}

fn path_exists(path: &Path) -> Result<bool, CliError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(path, &error)),
    }
}

fn path_is_managed(path: &Path, managed_root: &Path) -> bool {
    path != managed_root && path.starts_with(managed_root)
}

fn paths_refer_to_same_location(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn create_parent(path: &Path) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        CliError::new(
            ErrorCode::PathOutsideRepo,
            format!("target path has no parent: {}", path.display()),
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))
}

fn prune_empty_target_parents(target: &Path, managed_root: &Path) {
    let mut current = target.parent();
    while let Some(directory) = current {
        if directory == managed_root || !directory.starts_with(managed_root) {
            break;
        }
        if fs::remove_dir(directory).is_err() {
            break;
        }
        current = directory.parent();
    }
}

fn ensure_exclude_block(path: &Path, block: &str) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        CliError::new(
            ErrorCode::InternalError,
            format!("exclude path has no parent: {}", path.display()),
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    let current = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(io_error(path, &error)),
    };
    if current.contains(block) {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| io_error(path, &error))?;
    if !current.is_empty() && !current.ends_with('\n') {
        file.write_all(b"\n")
            .map_err(|error| io_error(path, &error))?;
    }
    file.write_all(block.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| io_error(path, &error))
}

fn create_hook_template(path: &Path, content: &str) -> Result<(), CliError> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => return Err(io_error(path, &error)),
    };
    file.write_all(content.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| io_error(path, &error))?;
    set_executable(path)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|error| io_error(path, &error))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), CliError> {
    Ok(())
}

fn managed_root_exclude_entry(repo_root: &Path, managed_root: &Path) -> Option<String> {
    let relative = managed_root.strip_prefix(repo_root).ok()?;
    if relative.as_os_str().is_empty() {
        return Some("./".to_owned());
    }
    let normalized = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Some(format!("{normalized}/"))
}

fn rollback_created_worktree<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    target: &Path,
    managed_root: &Path,
    branch: &str,
    created_branch_oid: Option<&str>,
    mut original: CliError,
) -> CliError {
    let mut failures = Vec::new();
    let worktree_removed = match git.run_git_checked(
        repo_root,
        &os_args(["worktree", "remove", "--force", &target.to_string_lossy()]),
    ) {
        Ok(_) => true,
        Err(error) => {
            failures.push(error.message);
            false
        }
    };
    if worktree_removed {
        for key in [
            "backupPath",
            "committed",
            "committedState",
            "recoveryPath",
            "recoveryPathError",
            "recoveryPathUnavailable",
            "recoveryRequired",
            "rollbackFailed",
            "rollbackFailures",
            "stagedPath",
            "transactionCleanupError",
            "transactionCleanupFailed",
        ] {
            original.details.remove(key);
        }
        original.execution.recovery.clear();
        original
            .execution
            .completed
            .push("removeCreatedWorktree".to_owned());
        original
            .details
            .insert("worktreeRolledBack".to_owned(), json!(true));
    }
    if let Some(oid) = created_branch_oid
        && let Err(error) = delete_local_branch_if_unchanged(git, repo_root, branch, oid)
    {
        failures.push(error.message);
    }
    original.execution.state = if failures.is_empty() {
        ExecutionState::RolledBack
    } else {
        ExecutionState::RecoveryRequired
    };
    if !failures.is_empty() {
        original
            .execution
            .recovery
            .insert("worktreePath".to_owned(), json!(target));
        original
            .execution
            .recovery
            .insert("branch".to_owned(), json!(branch));
        original
            .execution
            .recovery
            .insert("rollbackFailures".to_owned(), json!(failures));
        original
            .details
            .insert("worktreeRollbackFailures".to_owned(), json!(failures));
    }
    prune_empty_target_parents(target, managed_root);
    original
}

fn resolve_local_branch_oid<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
) -> Result<String, CliError> {
    let output = git.run_git_checked(
        repo_root,
        &os_args(["rev-parse", "--verify", &format!("refs/heads/{branch}")]),
    )?;
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if oid.is_empty() {
        return Err(CliError::new(
            ErrorCode::InternalError,
            format!("failed to resolve branch before rollback: {branch}"),
        ));
    }
    Ok(oid)
}

fn delete_local_branch_if_unchanged<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
    expected_oid: &str,
) -> Result<(), CliError> {
    git.run_git_checked(
        repo_root,
        &os_args([
            "update-ref",
            "-d",
            &format!("refs/heads/{branch}"),
            expected_oid,
        ]),
    )?;
    Ok(())
}

fn rollback_local_branch_if_unchanged<G: CreateMutationGit>(
    git: &G,
    repo_root: &Path,
    branch: &str,
    expected_oid: &str,
) {
    let _ = delete_local_branch_if_unchanged(git, repo_root, branch, expected_oid);
}

fn os_args<const N: usize>(args: [&str; N]) -> Vec<OsString> {
    args.into_iter().map(OsString::from).collect()
}

fn target_string(path: &Path) -> Result<String, CliError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        CliError::new(
            ErrorCode::UnsupportedRepositoryLayout,
            "non-UTF-8 worktree paths are unsupported",
        )
        .with_details(BTreeMap::from([(
            "path".to_owned(),
            json!(path.to_string_lossy()),
        )]))
    })
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;

    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn git_output_error(cwd: &Path, args: &[OsString], output: &ProcessOutput) -> CliError {
    CliError::new(ErrorCode::GitCommandFailed, "git command failed").with_details(BTreeMap::from([
        ("cwd".to_owned(), json!(cwd.to_string_lossy())),
        (
            "argv".to_owned(),
            json!(
                args.iter()
                    .map(|arg| arg.to_string_lossy())
                    .collect::<Vec<_>>()
            ),
        ),
        ("exitCode".to_owned(), json!(output.exit_code)),
        ("timedOut".to_owned(), json!(output.timed_out)),
        (
            "stdout".to_owned(),
            json!(String::from_utf8_lossy(&output.stdout)),
        ),
        (
            "stderr".to_owned(),
            json!(String::from_utf8_lossy(&output.stderr)),
        ),
    ]))
}

fn invalid_remote_branch(value: &str) -> CliError {
    CliError::new(
        ErrorCode::InvalidRemoteBranchFormat,
        format!("invalid remote branch format: {value}"),
    )
    .with_details(BTreeMap::from([("value".to_owned(), json!(value))]))
}

fn invalid_branch(branch: &str) -> CliError {
    CliError::new(
        ErrorCode::InvalidArgument,
        format!("invalid branch name: {branch}"),
    )
    .with_details(BTreeMap::from([("branch".to_owned(), json!(branch))]))
}

fn branch_already_attached(branch: &str) -> CliError {
    CliError::new(
        ErrorCode::BranchAlreadyAttached,
        format!("branch is already attached to a worktree: {branch}"),
    )
    .with_details(BTreeMap::from([("branch".to_owned(), json!(branch))]))
}

fn branch_already_exists(branch: &str) -> CliError {
    CliError::new(
        ErrorCode::BranchAlreadyExists,
        format!("branch already exists locally: {branch}"),
    )
    .with_details(BTreeMap::from([("branch".to_owned(), json!(branch))]))
}

fn target_not_empty(path: &Path) -> CliError {
    CliError::new(
        ErrorCode::TargetPathNotEmpty,
        format!("target path is not empty: {}", path.display()),
    )
    .with_details(BTreeMap::from([("path".to_owned(), json!(path))]))
}

fn target_changed(expected: &Path, actual: &Path) -> CliError {
    CliError::new(
        ErrorCode::PathOutsideRepo,
        "managed worktree target changed after preflight",
    )
    .with_details(BTreeMap::from([
        ("expectedPath".to_owned(), json!(expected)),
        ("actualPath".to_owned(), json!(actual)),
    ]))
}

fn io_error(path: &Path, error: &io::Error) -> CliError {
    CliError::new(
        ErrorCode::InternalError,
        format!("filesystem operation failed at {}: {error}", path.display()),
    )
    .with_details(BTreeMap::from([
        ("path".to_owned(), json!(path)),
        ("cause".to_owned(), json!(error.to_string())),
    ]))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;
    use crate::adapters::process::StdProcessRunner;
    use crate::domain::worktree::{
        PrState, WorktreeLockState, WorktreeMergedState, WorktreeStatus, WorktreeUpstreamState,
    };

    struct FailWorktreeAdd {
        inner: GitCli<StdProcessRunner>,
    }

    impl CreateMutationGit for FailWorktreeAdd {
        fn run_git(&self, cwd: &Path, args: &[OsString]) -> Result<ProcessOutput, CliError> {
            self.inner.run_git(cwd, args)
        }

        fn run_git_checked(
            &self,
            cwd: &Path,
            args: &[OsString],
        ) -> Result<ProcessOutput, CliError> {
            let is_worktree_add = args.first().is_some_and(|arg| arg == "worktree")
                && args.get(1).is_some_and(|arg| arg == "add");
            if is_worktree_add {
                return Err(CliError::new(
                    ErrorCode::GitCommandFailed,
                    "injected worktree add failure",
                ));
            }
            self.inner.run_git_checked(cwd, args)
        }
    }

    struct RawBranchWinsAddRace {
        inner: GitCli<StdProcessRunner>,
    }

    impl CreateMutationGit for RawBranchWinsAddRace {
        fn run_git(&self, cwd: &Path, args: &[OsString]) -> Result<ProcessOutput, CliError> {
            self.inner.run_git(cwd, args)
        }

        fn run_git_checked(
            &self,
            cwd: &Path,
            args: &[OsString],
        ) -> Result<ProcessOutput, CliError> {
            let is_worktree_add_new = args.first().is_some_and(|arg| arg == "worktree")
                && args.get(1).is_some_and(|arg| arg == "add")
                && args.get(2).is_some_and(|arg| arg == "-b");
            if is_worktree_add_new {
                let branch = args.get(3).expect("new branch argument");
                self.inner.run_git_checked(
                    cwd,
                    &[
                        OsString::from("branch"),
                        branch.clone(),
                        OsString::from("main"),
                    ],
                )?;
                return Err(CliError::new(
                    ErrorCode::GitCommandFailed,
                    "raw Git branch won the simulated race",
                ));
            }
            self.inner.run_git_checked(cwd, args)
        }
    }

    fn git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git must start");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn fixture() -> (TempDir, RepoContext, GitCli<StdProcessRunner>) {
        let temporary = tempfile::tempdir().expect("tempdir");
        git(temporary.path(), &["init", "-b", "main"]);
        git(temporary.path(), &["config", "user.name", "Test"]);
        git(
            temporary.path(),
            &["config", "user.email", "test@example.com"],
        );
        fs::write(temporary.path().join("README"), "test\n").expect("fixture file");
        git(temporary.path(), &["add", "README"]);
        git(temporary.path(), &["commit", "-m", "initial"]);
        let adapter = GitCli::new(StdProcessRunner);
        let context = adapter
            .resolve_repo_context(temporary.path())
            .expect("repo context");
        (temporary, context, adapter)
    }

    fn status(branch: Option<&str>, path: PathBuf, locked: bool) -> WorktreeStatus {
        WorktreeStatus {
            branch: branch.map(str::to_owned),
            path,
            head: "head".to_owned(),
            dirty: false,
            locked: WorktreeLockState {
                value: locked,
                reason: None,
                owner: None,
            },
            merged: WorktreeMergedState {
                by_ancestry: None,
                by_pr: None,
                overall: None,
            },
            pr: PrState::none(),
            upstream: WorktreeUpstreamState {
                ahead: None,
                behind: None,
                remote: None,
            },
        }
    }

    fn snapshot(repo_root: &Path, worktrees: Vec<WorktreeStatus>) -> WorktreeSnapshot {
        WorktreeSnapshot {
            repo_root: repo_root.to_path_buf(),
            base_branch: Some("main".to_owned()),
            worktrees,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn init_uses_common_git_directory_and_is_idempotent() {
        let (_temporary, context, _adapter) = fixture();
        let managed = context.repo_root.join(".worktree");
        let first = prepare_init(&context, managed.clone()).expect("prepare");
        assert!(!first.already_initialized);
        apply_init(&first).expect("first init");
        let second = prepare_init(&context, managed).expect("prepare again");
        assert!(second.already_initialized);
        apply_init(&second).expect("second init");

        let exclude =
            fs::read_to_string(context.git_common_dir.join("info/exclude")).expect("exclude");
        assert_eq!(exclude.matches(EXCLUDE_MARKER).count(), 1);
        assert!(
            context
                .repo_root
                .join(".vde/worktree/hooks/post-new")
                .is_file()
        );
    }

    #[test]
    fn new_preflight_rejects_attached_existing_and_non_empty_target_without_mutation() {
        let (_temporary, context, adapter) = fixture();
        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        let attached = snapshot(
            &context.repo_root,
            vec![status(Some("topic"), context.repo_root.clone(), false)],
        );
        assert_eq!(
            prepare_new(
                &adapter,
                &context.repo_root,
                &managed,
                &attached.worktrees,
                "topic",
                "main"
            )
            .expect_err("attached must fail")
            .code,
            ErrorCode::BranchAlreadyAttached
        );

        git(&context.repo_root, &["branch", "local", "main"]);
        let empty = snapshot(&context.repo_root, Vec::new());
        assert_eq!(
            prepare_new(
                &adapter,
                &context.repo_root,
                &managed,
                &empty.worktrees,
                "local",
                "main"
            )
            .expect_err("existing must fail")
            .code,
            ErrorCode::BranchAlreadyExists
        );
        let blocked = managed.join("feature/blocked");
        fs::create_dir_all(&blocked).expect("target");
        fs::write(blocked.join("file"), "blocked").expect("block target");
        assert_eq!(
            prepare_new(
                &adapter,
                &context.repo_root,
                &managed,
                &empty.worktrees,
                "feature/blocked",
                "main",
            )
            .expect_err("non-empty must fail")
            .code,
            ErrorCode::TargetPathNotEmpty
        );
    }

    #[test]
    fn failed_add_never_deletes_a_raw_git_branch_that_won_the_race() {
        let (_temporary, context, adapter) = fixture();
        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        let plan = prepare_new(
            &adapter,
            &context.repo_root,
            &managed,
            &snapshot(&context.repo_root, Vec::new()).worktrees,
            "feature/race",
            "main",
        )
        .expect("new plan");

        let racing = RawBranchWinsAddRace {
            inner: GitCli::new(StdProcessRunner),
        };
        assert_eq!(
            apply_new_git(&racing, &plan)
                .expect_err("injected add failure")
                .code,
            ErrorCode::GitCommandFailed
        );
        assert!(local_branch_exists(&adapter, &context.repo_root, "feature/race").unwrap());
    }

    #[test]
    fn new_git_phase_does_not_persist_lifecycle_before_state_finalize() {
        let (_temporary, context, adapter) = fixture();
        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        fs::create_dir_all(context.repo_root.join(".vde/worktree/state/branches"))
            .expect("state root");
        let plan = prepare_new(
            &adapter,
            &context.repo_root,
            &managed,
            &snapshot(&context.repo_root, Vec::new()).worktrees,
            "feature/state-phase",
            "main",
        )
        .expect("new plan");

        let applied = apply_new_git(&adapter, &plan).expect("Git phase");
        assert!(matches!(
            crate::state::lifecycle::read_worktree_lifecycle(
                &context.repo_root,
                "feature/state-phase"
            )
            .state,
            crate::state::json_store::JsonRecordState::Missing
        ));
        let result = finalize_worktree_state(&adapter, applied).expect("state phase");
        assert_eq!(result.branch, "feature/state-phase");
        assert!(matches!(
            crate::state::lifecycle::read_worktree_lifecycle(
                &context.repo_root,
                "feature/state-phase"
            )
            .state,
            crate::state::json_store::JsonRecordState::Valid(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn worktree_include_copies_only_matching_ignored_regular_files_on_switch_create() {
        let (_temporary, context, adapter) = fixture();
        fs::write(
            context.repo_root.join(".gitignore"),
            ".env*\n!.env.visible\nlocal/\nlink.env\n.vde/worktree/\n",
        )
        .expect("gitignore");
        fs::write(
            context.repo_root.join(WORKTREE_INCLUDE_PATH),
            ".env*\n!.env.skip\nlocal/**\ntracked.txt\nlink.env\n.vde/worktree/**\n",
        )
        .expect("worktree include");
        fs::write(context.repo_root.join("tracked.txt"), "committed\n").expect("tracked file");
        git(
            &context.repo_root,
            &["add", ".gitignore", WORKTREE_INCLUDE_PATH, "tracked.txt"],
        );
        git(
            &context.repo_root,
            &["commit", "-m", "worktree include fixture"],
        );

        fs::write(context.repo_root.join("tracked.txt"), "dirty primary\n")
            .expect("dirty tracked file");
        fs::write(context.repo_root.join(".env"), "primary env\n").expect("env file");
        fs::write(context.repo_root.join(".env.local"), "local env\n").expect("local env file");
        fs::write(context.repo_root.join(".env.skip"), "include-negated\n")
            .expect("include-negated env file");
        fs::write(
            context.repo_root.join(".env.visible"),
            "gitignore-negated\n",
        )
        .expect("gitignore-negated env file");
        fs::create_dir(context.repo_root.join("local")).expect("local directory");
        fs::write(
            context.repo_root.join("local/config.json"),
            "local config\n",
        )
        .expect("local config");
        symlink(".env.local", context.repo_root.join("link.env")).expect("source symlink");

        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(managed.join("stale")).expect("managed fixture");
        fs::write(managed.join("stale/.env.local"), "stale worktree\n")
            .expect("managed ignored file");
        fs::create_dir_all(context.repo_root.join(".vde/worktree/state/branches"))
            .expect("state root");
        fs::write(
            context.repo_root.join(".vde/worktree/private.env"),
            "internal metadata\n",
        )
        .expect("internal metadata file");
        let plan = prepare_switch(
            &adapter,
            &context.repo_root,
            &managed,
            &snapshot(&context.repo_root, Vec::new()).worktrees,
            "feature/include",
            "main",
        )
        .expect("switch plan");
        let applied = apply_switch_git(&adapter, &plan).expect("Git phase");
        fs::write(applied.path.join(".env"), "existing destination\n")
            .expect("existing destination");
        let created = finalize_worktree_state(&adapter, applied).expect("state phase");

        assert_eq!(
            fs::read_to_string(created.path.join(".env")).unwrap(),
            "existing destination\n"
        );
        assert_eq!(
            fs::read_to_string(created.path.join(".env.local")).unwrap(),
            "local env\n"
        );
        assert_eq!(
            fs::read_to_string(created.path.join("local/config.json")).unwrap(),
            "local config\n"
        );
        assert_eq!(
            fs::read_to_string(created.path.join("tracked.txt")).unwrap(),
            "committed\n"
        );
        assert!(!created.path.join("link.env").exists());
        assert!(!created.path.join(".env.skip").exists());
        assert!(!created.path.join(".env.visible").exists());
        assert!(!created.path.join(".worktree/stale/.env.local").exists());
        assert!(!created.path.join(".vde/worktree/private.env").exists());

        fs::remove_file(created.path.join(".env.local")).expect("remove copied file");
        let existing = prepare_switch(
            &adapter,
            &context.repo_root,
            &managed,
            &snapshot(
                &context.repo_root,
                vec![status(Some("feature/include"), created.path.clone(), false)],
            )
            .worktrees,
            "feature/include",
            "main",
        )
        .expect("existing switch plan");
        finalize_worktree_state(
            &adapter,
            apply_switch_git(&adapter, &existing).expect("existing switch Git phase"),
        )
        .expect("existing switch state phase");
        assert!(!created.path.join(".env.local").exists());
    }

    #[test]
    fn worktree_include_skips_a_path_when_the_destination_parent_is_a_file() {
        let (_temporary, context, adapter) = fixture();
        fs::write(context.repo_root.join(".gitignore"), "local/\n").expect("gitignore");
        fs::write(context.repo_root.join(WORKTREE_INCLUDE_PATH), "local/**\n")
            .expect("worktree include");
        git(
            &context.repo_root,
            &["add", ".gitignore", WORKTREE_INCLUDE_PATH],
        );
        git(&context.repo_root, &["commit", "-m", "include fixture"]);
        git(&context.repo_root, &["checkout", "-b", "collision-base"]);
        fs::write(context.repo_root.join("local"), "tracked destination\n")
            .expect("tracked destination");
        git(&context.repo_root, &["add", "local"]);
        git(
            &context.repo_root,
            &["commit", "-m", "destination collision"],
        );
        git(&context.repo_root, &["checkout", "main"]);
        fs::create_dir(context.repo_root.join("local")).expect("local source directory");
        fs::write(
            context.repo_root.join("local/config.json"),
            "ignored source\n",
        )
        .expect("ignored source");

        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        let plan = prepare_new(
            &adapter,
            &context.repo_root,
            &managed,
            &snapshot(&context.repo_root, Vec::new()).worktrees,
            "feature/parent-collision",
            "collision-base",
        )
        .expect("new plan");
        let created =
            finalize_worktree_state(&adapter, apply_new_git(&adapter, &plan).expect("Git phase"))
                .expect("state phase");

        assert_eq!(
            fs::read_to_string(created.path.join("local")).unwrap(),
            "tracked destination\n"
        );
    }

    #[test]
    fn invalid_worktree_include_rolls_back_the_created_worktree_and_branch() {
        let (_temporary, context, adapter) = fixture();
        fs::create_dir(context.repo_root.join(WORKTREE_INCLUDE_PATH))
            .expect("invalid worktree include directory");
        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        let plan = prepare_new(
            &adapter,
            &context.repo_root,
            &managed,
            &snapshot(&context.repo_root, Vec::new()).worktrees,
            "feature/invalid-include",
            "main",
        )
        .expect("new plan");
        let target = plan.target_path.clone();
        let applied = apply_new_git(&adapter, &plan).expect("Git phase");

        let error = finalize_worktree_state(&adapter, applied).expect_err("invalid include");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.details["worktreeRolledBack"], true);
        assert_eq!(
            error.details["worktreeIncludePath"],
            json!(context.repo_root.join(WORKTREE_INCLUDE_PATH))
        );
        assert!(!target.exists());
        assert!(
            !local_branch_exists(&adapter, &context.repo_root, "feature/invalid-include").unwrap()
        );
    }

    #[test]
    fn successful_worktree_rollback_removes_stale_copy_recovery_details() {
        let (_temporary, context, adapter) = fixture();
        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        let plan = prepare_new(
            &adapter,
            &context.repo_root,
            &managed,
            &snapshot(&context.repo_root, Vec::new()).worktrees,
            "feature/stale-recovery-details",
            "main",
        )
        .expect("new plan");
        let applied = apply_new_git(&adapter, &plan).expect("Git phase");
        let original = CliError::new(ErrorCode::InternalError, "copy cleanup failed").with_details(
            BTreeMap::from([
                ("committed".to_owned(), json!(true)),
                (
                    "recoveryPath".to_owned(),
                    json!(applied.path.join("recovery")),
                ),
                ("recoveryRequired".to_owned(), json!(true)),
                ("rollbackFailed".to_owned(), json!(true)),
                ("transactionCleanupFailed".to_owned(), json!(true)),
            ]),
        );
        let error = rollback_created_worktree(
            &adapter,
            &applied.repo_root,
            &applied.path,
            applied
                .managed_worktree_root
                .as_deref()
                .expect("managed root in applied plan"),
            &applied.branch,
            applied.created_branch_oid.as_deref(),
            original,
        );

        assert_eq!(error.details["worktreeRolledBack"], true);
        for key in [
            "committed",
            "recoveryPath",
            "recoveryRequired",
            "rollbackFailed",
            "transactionCleanupFailed",
        ] {
            assert!(!error.details.contains_key(key), "stale detail: {key}");
        }
        assert!(!applied.path.exists());
        assert!(
            !local_branch_exists(
                &adapter,
                &context.repo_root,
                "feature/stale-recovery-details"
            )
            .unwrap()
        );
    }

    #[test]
    fn state_failure_compensation_preserves_a_branch_changed_after_git_apply() {
        let (_temporary, context, adapter) = fixture();
        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        fs::create_dir_all(context.repo_root.join(".vde/worktree/state")).expect("state root");
        fs::write(
            context.repo_root.join(".vde/worktree/state/branches"),
            "blocks lifecycle directory",
        )
        .expect("blocking state file");
        let plan = prepare_new(
            &adapter,
            &context.repo_root,
            &managed,
            &snapshot(&context.repo_root, Vec::new()).worktrees,
            "feature/state-failure",
            "main",
        )
        .expect("new plan");
        let applied = apply_new_git(&adapter, &plan).expect("Git phase");

        fs::write(applied.path.join("changed.txt"), "changed\n").unwrap();
        git(&applied.path, &["add", "changed.txt"]);
        git(&applied.path, &["commit", "-m", "raw branch change"]);

        assert!(finalize_worktree_state(&adapter, applied).is_err());
        assert!(
            local_branch_exists(&adapter, &context.repo_root, "feature/state-failure").unwrap(),
            "OID-conditional compensation must preserve the changed branch"
        );
    }

    #[test]
    fn switch_attaches_a_local_branch_then_reuses_the_same_worktree() {
        let (_temporary, context, adapter) = fixture();
        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        fs::create_dir_all(context.repo_root.join(".vde/worktree/state/branches"))
            .expect("state root");
        git(&context.repo_root, &["branch", "feature/local", "main"]);
        let empty = snapshot(&context.repo_root, Vec::new());
        let plan = prepare_switch(
            &adapter,
            &context.repo_root,
            &managed,
            &empty.worktrees,
            "feature/local",
            "main",
        )
        .expect("switch plan");
        let created = finalize_worktree_state(
            &adapter,
            apply_switch_git(&adapter, &plan).expect("switch Git apply"),
        )
        .expect("switch state apply");
        assert_eq!(created.disposition, Some(WorktreeDisposition::Created));
        assert!(created.path.is_dir());

        let existing_snapshot = snapshot(
            &context.repo_root,
            vec![status(Some("feature/local"), created.path.clone(), false)],
        );
        let repeated = prepare_switch(
            &adapter,
            &context.repo_root,
            &managed,
            &existing_snapshot.worktrees,
            "feature/local",
            "main",
        )
        .expect("existing plan");
        let reused = finalize_worktree_state(
            &adapter,
            apply_switch_git(&adapter, &repeated).expect("existing Git apply"),
        )
        .expect("existing state apply");
        assert_eq!(reused.disposition, Some(WorktreeDisposition::Existing));
        assert_eq!(reused.path, created.path);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn get_validates_remote_inputs_creates_tracking_branch_and_reuses_attachment() {
        let (_temporary, context, adapter) = fixture();
        let remote = tempfile::tempdir().expect("remote tempdir");
        git(remote.path(), &["init", "--bare"]);
        git(
            &context.repo_root,
            &[
                "remote",
                "add",
                "origin",
                remote.path().to_str().expect("remote utf8"),
            ],
        );
        git(&context.repo_root, &["push", "origin", "main"]);
        git(&context.repo_root, &["checkout", "-b", "feature/get"]);
        fs::write(context.repo_root.join("feature"), "remote feature\n").expect("feature file");
        git(&context.repo_root, &["add", "feature"]);
        git(&context.repo_root, &["commit", "-m", "feature"]);
        git(&context.repo_root, &["push", "origin", "feature/get"]);
        git(&context.repo_root, &["checkout", "main"]);
        git(&context.repo_root, &["branch", "-D", "feature/get"]);
        fs::write(context.repo_root.join(".gitignore"), ".env.get\n").expect("get gitignore");
        fs::write(context.repo_root.join(WORKTREE_INCLUDE_PATH), ".env.get\n")
            .expect("get worktree include");
        fs::write(context.repo_root.join(".env.get"), "get-local\n").expect("get ignored file");

        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        let empty = snapshot(&context.repo_root, Vec::new());
        assert_eq!(
            prepare_get(
                &adapter,
                &context.repo_root,
                &managed,
                &empty.worktrees,
                "upstream/feature/get",
                "main",
            )
            .expect_err("missing remote")
            .code,
            ErrorCode::RemoteNotFound
        );
        assert_eq!(
            prepare_get(
                &adapter,
                &context.repo_root,
                &managed,
                &empty.worktrees,
                "origin/feature/missing",
                "main",
            )
            .expect_err("missing remote branch")
            .code,
            ErrorCode::RemoteBranchNotFound
        );

        let plan = prepare_get(
            &adapter,
            &context.repo_root,
            &managed,
            &empty.worktrees,
            "origin/feature/get",
            "main",
        )
        .expect("get plan");
        let injected = FailWorktreeAdd {
            inner: GitCli::new(StdProcessRunner),
        };
        assert_eq!(
            apply_get_git(&injected, &plan)
                .expect_err("injected add failure")
                .code,
            ErrorCode::GitCommandFailed
        );
        let branch_after_rollback = adapter
            .execute(
                &context.repo_root,
                ["show-ref", "--verify", "--quiet", "refs/heads/feature/get"],
            )
            .expect("branch probe");
        assert_eq!(branch_after_rollback.exit_code, Some(1));
        assert!(!managed.join("feature/get").exists());

        let created = finalize_worktree_state(
            &adapter,
            apply_get_git(&adapter, &plan).expect("get Git apply"),
        )
        .expect("get state apply");
        assert_eq!(created.disposition, Some(WorktreeDisposition::Created));
        assert_eq!(
            fs::read_to_string(created.path.join(".env.get")).unwrap(),
            "get-local\n"
        );
        let upstream = adapter
            .execute_checked(&created.path, ["rev-parse", "--abbrev-ref", "@{upstream}"])
            .expect("upstream");
        assert_eq!(
            String::from_utf8_lossy(&upstream.stdout).trim(),
            "origin/feature/get"
        );

        let existing = snapshot(
            &context.repo_root,
            vec![status(Some("feature/get"), created.path.clone(), false)],
        );
        let repeated = prepare_get(
            &adapter,
            &context.repo_root,
            &managed,
            &existing.worktrees,
            "origin/feature/get",
            "main",
        )
        .expect("existing get plan");
        let reused = finalize_worktree_state(
            &adapter,
            apply_get_git(&adapter, &repeated).expect("existing get Git apply"),
        )
        .expect("existing get state apply");
        assert_eq!(reused.disposition, Some(WorktreeDisposition::Existing));
        assert_eq!(reused.path, created.path);
    }

    #[test]
    fn adopt_defaults_to_dry_run_and_apply_reports_target_created_after_preflight() {
        let (temporary, context, adapter) = fixture();
        let managed = context.repo_root.join(".worktree");
        fs::create_dir_all(&managed).expect("managed root");
        let external = temporary.path().with_extension("external");
        git(&context.repo_root, &["branch", "feature/adopt", "main"]);
        git(
            &context.repo_root,
            &[
                "worktree",
                "add",
                external.to_str().expect("utf8"),
                "feature/adopt",
            ],
        );
        let current = snapshot(
            &context.repo_root,
            vec![
                status(Some("main"), context.repo_root.clone(), false),
                status(Some("feature/adopt"), external.clone(), false),
            ],
        );
        let dry = prepare_adopt(&context.repo_root, &managed, &current, true).expect("dry plan");
        assert_eq!(dry.candidates.len(), 1);
        assert!(
            apply_adopt(&adapter, &dry)
                .expect("dry apply")
                .moved
                .is_empty()
        );

        let apply =
            prepare_adopt(&context.repo_root, &managed, &current, false).expect("apply plan");
        let target = apply.candidates[0].to_path.clone();
        fs::create_dir_all(target.parent().expect("target parent")).expect("parent");
        fs::write(&target, "hook collision").expect("collision");
        let result = apply_adopt(&adapter, &apply).expect("partial result");
        assert!(result.has_failures());
        assert_eq!(result.failed[0].code, "TARGET_PATH_NOT_EMPTY");
        assert_eq!(result.failed[0].details["path"], json!(target));
        assert_eq!(result.failed[0].execution.phase, ExecutionPhase::Preflight);
        assert_eq!(result.failed[0].execution.state, ExecutionState::NotStarted);
        assert!(external.is_dir());
    }

    #[test]
    fn remote_branch_parser_splits_only_the_remote_prefix() {
        assert_eq!(
            parse_remote_branch("origin/feature/deep").expect("valid"),
            ("origin".to_owned(), "feature/deep".to_owned())
        );
        for invalid in ["origin", "/feature", "origin/"] {
            assert_eq!(
                parse_remote_branch(invalid).expect_err("invalid").code,
                ErrorCode::InvalidRemoteBranchFormat
            );
        }
    }
}
