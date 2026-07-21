use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoContext {
    pub repo_root: PathBuf,
    pub current_worktree_root: PathBuf,
    pub git_common_dir: PathBuf,
}
