use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;

use crate::domain::worktree::PrState;
use crate::ports::process::{ProcessOutput, ProcessRunner};

pub trait GitSnapshotPort: Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn run_git<I, S>(&self, cwd: &Path, args: I) -> Result<ProcessOutput, Self::Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>;
}

pub trait PrStateLookup: Sync {
    fn resolve_pr_states(
        &self,
        repo_root: &Path,
        base_branch: Option<&str>,
        branches: &[Option<String>],
        enabled: bool,
    ) -> HashMap<String, PrState>;
}

impl<R> GitSnapshotPort for crate::adapters::git_cli::GitCli<R>
where
    R: ProcessRunner + Sync,
{
    type Error = crate::adapters::git_cli::GitCliError;

    fn run_git<I, S>(&self, cwd: &Path, args: I) -> Result<ProcessOutput, Self::Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.execute(cwd, args)
    }
}
