use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::repo::RepoContext;
use crate::ports::process::{
    OutputPolicy, ProcessCommand, ProcessError, ProcessOutput, ProcessRunner, StdinPolicy,
};

const DEFAULT_GIT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum GitCliError {
    NotGitRepository { cwd: PathBuf, stderr: Vec<u8> },
    UnsupportedRepositoryLayout { cwd: PathBuf, reason: String },
    GitCommandFailed(Box<GitCommandFailure>),
}

#[derive(Debug)]
pub struct GitCommandFailure {
    pub cwd: PathBuf,
    pub args: Vec<OsString>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub source: Option<ProcessError>,
}

impl GitCliError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotGitRepository { .. } => "NOT_GIT_REPOSITORY",
            Self::UnsupportedRepositoryLayout { .. } => "UNSUPPORTED_REPOSITORY_LAYOUT",
            Self::GitCommandFailed(_) => "GIT_COMMAND_FAILED",
        }
    }
}

impl fmt::Display for GitCliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotGitRepository { cwd, .. } => {
                write!(
                    formatter,
                    "{} is not inside a Git repository",
                    cwd.display()
                )
            }
            Self::UnsupportedRepositoryLayout { cwd, reason } => write!(
                formatter,
                "unsupported Git repository layout in {}: {reason}",
                cwd.display()
            ),
            Self::GitCommandFailed(failure) => write!(
                formatter,
                "git command failed in {} (args: {:?}, exit code: {:?}, timed out: {})",
                failure.cwd.display(),
                failure.args,
                failure.exit_code,
                failure.timed_out
            ),
        }
    }
}

impl std::error::Error for GitCliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::GitCommandFailed(failure) => failure
                .source
                .as_ref()
                .map(|source| source as &(dyn std::error::Error + 'static)),
            Self::NotGitRepository { .. } | Self::UnsupportedRepositoryLayout { .. } => None,
        }
    }
}

#[derive(Debug)]
pub struct GitCli<R> {
    runner: R,
    timeout: Duration,
}

impl<R> GitCli<R>
where
    R: ProcessRunner,
{
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            timeout: DEFAULT_GIT_TIMEOUT,
        }
    }

    pub fn with_timeout(runner: R, timeout: Duration) -> Self {
        Self { runner, timeout }
    }

    pub fn execute<I, S>(&self, cwd: &Path, args: I) -> Result<ProcessOutput, GitCliError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = args
            .into_iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect::<Vec<_>>();
        let mut command = ProcessCommand::new("git");
        command.args.clone_from(&args);
        command.cwd = Some(cwd.to_path_buf());
        command.stdin = StdinPolicy::Null;
        command.stdout = OutputPolicy::Capture;
        command.stderr = OutputPolicy::Capture;
        command.timeout = Some(self.timeout);

        self.runner.run(&command).map_err(|source| {
            GitCliError::GitCommandFailed(Box::new(GitCommandFailure {
                cwd: cwd.to_path_buf(),
                args,
                exit_code: None,
                timed_out: false,
                stdout: Vec::new(),
                stderr: Vec::new(),
                source: Some(source),
            }))
        })
    }

    pub fn execute_checked<I, S>(&self, cwd: &Path, args: I) -> Result<ProcessOutput, GitCliError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = args
            .into_iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect::<Vec<_>>();
        let output = self.execute(cwd, &args)?;
        if output.exit_code == Some(0) && !output.timed_out {
            return Ok(output);
        }
        Err(command_failure(cwd, args, output))
    }

    pub fn resolve_repo_context(&self, cwd: &Path) -> Result<RepoContext, GitCliError> {
        let toplevel_args = [
            OsString::from("rev-parse"),
            OsString::from("--show-toplevel"),
        ];
        let toplevel = self.execute(cwd, &toplevel_args)?;
        if toplevel.exit_code != Some(0) || toplevel.timed_out {
            return Err(GitCliError::NotGitRepository {
                cwd: cwd.to_path_buf(),
                stderr: toplevel.stderr,
            });
        }
        let current_worktree_root = path_from_stdout(cwd, toplevel_args.to_vec(), toplevel)?;

        let common_dir_args = [
            OsString::from("rev-parse"),
            OsString::from("--path-format=absolute"),
            OsString::from("--git-common-dir"),
        ];
        let common_dir_output = self.execute(cwd, &common_dir_args)?;
        if common_dir_output.exit_code != Some(0) || common_dir_output.timed_out {
            return Err(command_failure(
                cwd,
                common_dir_args.to_vec(),
                common_dir_output,
            ));
        }
        let git_common_dir = path_from_stdout(cwd, common_dir_args.to_vec(), common_dir_output)?;
        let repo_root = self.resolve_primary_worktree_root(cwd, &git_common_dir)?;

        Ok(RepoContext {
            repo_root,
            current_worktree_root,
            git_common_dir,
        })
    }

    fn resolve_primary_worktree_root(
        &self,
        cwd: &Path,
        git_common_dir: &Path,
    ) -> Result<PathBuf, GitCliError> {
        let list_args = [
            OsString::from("worktree"),
            OsString::from("list"),
            OsString::from("--porcelain"),
            OsString::from("-z"),
        ];
        let list_output = self.execute_checked(cwd, &list_args)?;
        let listed_worktrees = worktree_paths_from_porcelain(&list_output.stdout);
        let listed_primary = listed_worktrees
            .first()
            .cloned()
            .ok_or_else(|| command_failure(cwd, list_args.to_vec(), list_output))?;

        if !paths_refer_to_same_location(&listed_primary, git_common_dir) {
            return Ok(listed_primary);
        }

        let config_args = [
            OsString::from("config"),
            OsString::from("--path"),
            OsString::from("--get"),
            OsString::from("core.worktree"),
        ];
        let config_output = self.execute(cwd, &config_args)?;
        if config_output.timed_out || !matches!(config_output.exit_code, Some(0 | 1)) {
            return Err(command_failure(cwd, config_args.to_vec(), config_output));
        }
        if config_output.exit_code == Some(1) {
            return Err(unsupported_layout(
                cwd,
                "separate Git directory does not define core.worktree",
            ));
        }
        let configured_worktree = path_from_bytes(trim_ascii_whitespace(&config_output.stdout));
        if configured_worktree.as_os_str().is_empty() {
            return Err(unsupported_layout(cwd, "core.worktree must not be empty"));
        }
        let configured_worktree = if configured_worktree.is_absolute() {
            configured_worktree
        } else {
            git_common_dir.join(configured_worktree)
        };
        validate_separate_git_pointer(cwd, &configured_worktree, git_common_dir)
    }
}

fn path_from_stdout(
    cwd: &Path,
    args: Vec<OsString>,
    output: ProcessOutput,
) -> Result<PathBuf, GitCliError> {
    let trimmed = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if trimmed.is_empty() {
        return Err(command_failure(cwd, args, output));
    }
    Ok(PathBuf::from(trimmed))
}

fn worktree_paths_from_porcelain(output: &[u8]) -> Vec<PathBuf> {
    output
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .map(path_from_bytes)
        .collect()
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;

    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn trim_ascii_whitespace(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn paths_refer_to_same_location(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn validate_separate_git_pointer(
    cwd: &Path,
    configured_worktree: &Path,
    git_common_dir: &Path,
) -> Result<PathBuf, GitCliError> {
    let worktree = configured_worktree.canonicalize().map_err(|error| {
        unsupported_layout(
            cwd,
            format!(
                "core.worktree {} cannot be resolved: {error}",
                configured_worktree.display()
            ),
        )
    })?;
    let pointer_path = worktree.join(".git");
    let contents = std::fs::read_to_string(&pointer_path).map_err(|error| {
        unsupported_layout(
            cwd,
            format!("cannot read {}: {error}", pointer_path.display()),
        )
    })?;
    let target = contents.trim().strip_prefix("gitdir: ").ok_or_else(|| {
        unsupported_layout(
            cwd,
            format!("{} is not a Git directory pointer", pointer_path.display()),
        )
    })?;
    let target = PathBuf::from(target);
    let target = if target.is_absolute() {
        target
    } else {
        worktree.join(target)
    };
    if !paths_refer_to_same_location(&target, git_common_dir) {
        return Err(unsupported_layout(
            cwd,
            format!(
                "{} does not point to the common Git directory {}",
                pointer_path.display(),
                git_common_dir.display()
            ),
        ));
    }
    Ok(worktree)
}

fn unsupported_layout(cwd: &Path, reason: impl Into<String>) -> GitCliError {
    GitCliError::UnsupportedRepositoryLayout {
        cwd: cwd.to_path_buf(),
        reason: reason.into(),
    }
}

fn command_failure(cwd: &Path, args: Vec<OsString>, output: ProcessOutput) -> GitCliError {
    GitCliError::GitCommandFailed(Box::new(GitCommandFailure {
        cwd: cwd.to_path_buf(),
        args,
        exit_code: output.exit_code,
        timed_out: output.timed_out,
        stdout: output.stdout,
        stderr: output.stderr,
        source: None,
    }))
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::process::Command;

    use tempfile::tempdir;

    use super::{GitCli, GitCliError};
    use crate::adapters::process::StdProcessRunner;

    fn git(cwd: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .status()
            .expect("run git fixture command");
        assert!(status.success(), "git fixture command failed: {args:?}");
    }

    #[test]
    fn resolves_primary_and_linked_worktree_context() {
        let fixture = tempdir().expect("create temporary directory");
        let primary = fixture.path().join("primary");
        let linked = fixture.path().join("linked");
        fs::create_dir_all(&primary).expect("create primary repository directory");
        git(&primary, &["init", "--quiet"]);
        git(
            &primary,
            &[
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            &primary,
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                linked.to_str().expect("linked path is utf-8"),
            ],
        );

        let linked_context = GitCli::new(StdProcessRunner)
            .resolve_repo_context(&linked)
            .expect("resolve linked context");
        let primary_context = GitCli::new(StdProcessRunner)
            .resolve_repo_context(&primary)
            .expect("resolve primary context");

        let primary = primary.canonicalize().expect("canonicalize primary path");
        let linked = linked.canonicalize().expect("canonicalize linked path");

        assert_eq!(primary_context.repo_root, primary);
        assert_eq!(primary_context.current_worktree_root, primary);
        assert_eq!(primary_context.git_common_dir, primary.join(".git"));
        assert_eq!(linked_context.repo_root, primary);
        assert_eq!(linked_context.current_worktree_root, linked);
        assert_eq!(linked_context.git_common_dir, primary.join(".git"));
    }

    #[test]
    fn resolves_the_same_repo_root_with_a_separate_git_directory() {
        let fixture = tempdir().expect("create temporary directory");
        let primary = fixture.path().join("worktrees/primary/repository");
        let git_directory = fixture.path().join("control/metadata/repository.git");
        let linked = fixture.path().join("elsewhere/linked/repository");
        fs::create_dir_all(&primary).expect("create primary repository directory");
        fs::create_dir_all(git_directory.parent().expect("metadata parent"))
            .expect("create metadata parent");
        fs::create_dir_all(linked.parent().expect("linked parent")).expect("create linked parent");
        git(
            fixture.path(),
            &[
                "init",
                "--quiet",
                &format!(
                    "--separate-git-dir={}",
                    git_directory.to_str().expect("git directory path is utf-8")
                ),
                primary.to_str().expect("primary path is utf-8"),
            ],
        );
        git(
            &primary,
            &[
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            fixture.path(),
            &[
                &format!(
                    "--git-dir={}",
                    git_directory.to_str().expect("git directory is utf-8")
                ),
                "config",
                "core.worktree",
                primary.to_str().expect("primary path is utf-8"),
            ],
        );
        git(
            &primary,
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                linked.to_str().expect("linked path is utf-8"),
            ],
        );

        let linked_context = GitCli::new(StdProcessRunner)
            .resolve_repo_context(&linked)
            .expect("resolve linked context");
        let primary_context = GitCli::new(StdProcessRunner)
            .resolve_repo_context(&primary)
            .expect("resolve primary context");

        let primary = primary.canonicalize().expect("canonicalize primary path");
        let linked = linked.canonicalize().expect("canonicalize linked path");
        let git_directory = git_directory
            .canonicalize()
            .expect("canonicalize separate git directory");

        assert_eq!(primary_context.repo_root, primary);
        assert_eq!(linked_context.repo_root, primary);
        assert_eq!(primary_context.current_worktree_root, primary);
        assert_eq!(linked_context.current_worktree_root, linked);
        assert_eq!(primary_context.git_common_dir, git_directory);
        assert_eq!(linked_context.git_common_dir, git_directory);

        fs::write(primary.join(".git"), "gitdir: /invalid/common-directory\n")
            .expect("replace primary Git pointer");
        let error = GitCli::new(StdProcessRunner)
            .resolve_repo_context(&linked)
            .expect_err("mismatched primary Git pointer must reject the layout");
        assert!(matches!(
            error,
            GitCliError::UnsupportedRepositoryLayout { .. }
        ));
    }

    #[test]
    fn rejects_a_separate_git_directory_without_core_worktree() {
        let fixture = tempdir().expect("create temporary directory");
        let primary = fixture.path().join("work/primary");
        let git_directory = fixture.path().join("metadata/repository.git");
        let linked = fixture.path().join("linked/repository");
        fs::create_dir_all(&primary).expect("create primary repository directory");
        fs::create_dir_all(git_directory.parent().expect("metadata parent"))
            .expect("create metadata parent");
        fs::create_dir_all(linked.parent().expect("linked parent")).expect("create linked parent");
        git(
            fixture.path(),
            &[
                "init",
                "--quiet",
                &format!(
                    "--separate-git-dir={}",
                    git_directory.to_str().expect("git directory is utf-8")
                ),
                primary.to_str().expect("primary path is utf-8"),
            ],
        );
        git(
            &primary,
            &[
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            &primary,
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                linked.to_str().expect("linked path is utf-8"),
            ],
        );

        let error = GitCli::new(StdProcessRunner)
            .resolve_repo_context(&linked)
            .expect_err("missing core.worktree must reject the layout");

        assert!(matches!(
            error,
            GitCliError::UnsupportedRepositoryLayout { .. }
        ));
        assert_eq!(error.code(), "UNSUPPORTED_REPOSITORY_LAYOUT");
    }

    #[test]
    fn reports_not_git_repository_with_a_stable_code() {
        let fixture = tempdir().expect("create temporary directory");
        let cli = GitCli::new(StdProcessRunner);

        let error = cli
            .resolve_repo_context(fixture.path())
            .expect_err("outside a repository must fail");

        assert!(matches!(error, GitCliError::NotGitRepository { .. }));
        assert_eq!(error.code(), "NOT_GIT_REPOSITORY");
    }

    #[test]
    fn executes_arbitrary_git_arguments() {
        let fixture = tempdir().expect("create temporary directory");
        let cli = GitCli::new(StdProcessRunner);

        let output = cli
            .execute(fixture.path(), ["--version"])
            .expect("execute git");

        assert_eq!(output.exit_code, Some(0));
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("git version "));
    }
}
