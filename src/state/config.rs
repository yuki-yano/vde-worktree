use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_saphyr::options::{DuplicateKeyPolicy, MergeKeyPolicy};
use thiserror::Error;

use crate::domain::fzf::validate_fzf_extra_args;

const CONFIG_RELATIVE_PATH: &[&str] = &[".vde", "worktree", "config.yml"];
const GLOBAL_CONFIG_RELATIVE_PATH: &[&str] = &["vde", "worktree", "config.yml"];

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to access config path {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid config {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },
    #[error("HOME is not set and XDG_CONFIG_HOME was not provided")]
    HomeUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub paths: PathsConfig,
    pub git: GitConfig,
    pub github: GithubConfig,
    pub hooks: HooksConfig,
    pub locks: LocksConfig,
    pub list: ListConfig,
    pub selector: SelectorConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathsConfig {
    pub worktree_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitConfig {
    pub base_branch: Option<String>,
    pub base_remote: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubConfig {
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HooksConfig {
    pub enabled: bool,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocksConfig {
    pub timeout_ms: u64,
    pub stale_lock_ttl_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListConfig {
    pub table: ListTableConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListTableConfig {
    pub columns: Vec<ListTableColumn>,
    pub path: ListPathConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListPathConfig {
    pub truncate: ListPathTruncate,
    pub min_width: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorConfig {
    pub cd: SelectorCdConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorCdConfig {
    pub prompt: String,
    pub surface: SelectorCdSurface,
    pub tmux_popup_opts: String,
    pub fzf: SelectorFzfConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorFzfConfig {
    pub extra_args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ListTableColumn {
    Branch,
    Dirty,
    Merged,
    Pr,
    Locked,
    Ahead,
    Behind,
    Path,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ListPathTruncate {
    Auto,
    Never,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SelectorCdSurface {
    Auto,
    Inline,
    TmuxPopup,
}

impl Default for ResolvedConfig {
    fn default() -> Self {
        Self {
            paths: PathsConfig {
                worktree_root: ".worktree".to_owned(),
            },
            git: GitConfig {
                base_branch: None,
                base_remote: "origin".to_owned(),
            },
            github: GithubConfig { enabled: true },
            hooks: HooksConfig {
                enabled: true,
                timeout_ms: 30_000,
            },
            locks: LocksConfig {
                timeout_ms: 15_000,
                stale_lock_ttl_seconds: 1_800,
            },
            list: ListConfig {
                table: ListTableConfig {
                    columns: vec![
                        ListTableColumn::Branch,
                        ListTableColumn::Dirty,
                        ListTableColumn::Merged,
                        ListTableColumn::Pr,
                        ListTableColumn::Locked,
                        ListTableColumn::Ahead,
                        ListTableColumn::Behind,
                        ListTableColumn::Path,
                    ],
                    path: ListPathConfig {
                        truncate: ListPathTruncate::Auto,
                        min_width: 12,
                    },
                },
            },
            selector: SelectorConfig {
                cd: SelectorCdConfig {
                    prompt: "worktree> ".to_owned(),
                    surface: SelectorCdSurface::Auto,
                    tmux_popup_opts: "80%,70%".to_owned(),
                    fzf: SelectorFzfConfig { extra_args: vec![] },
                },
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PartialConfig {
    pub paths: Option<PartialPathsConfig>,
    pub git: Option<PartialGitConfig>,
    pub github: Option<PartialGithubConfig>,
    pub hooks: Option<PartialHooksConfig>,
    pub locks: Option<PartialLocksConfig>,
    pub list: Option<PartialListConfig>,
    pub selector: Option<PartialSelectorConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PartialPathsConfig {
    pub worktree_root: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PartialGitConfig {
    #[serde(deserialize_with = "deserialize_base_branch_patch")]
    pub base_branch: BaseBranchPatch,
    pub base_remote: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BaseBranchPatch {
    #[default]
    Absent,
    Null,
    Value(String),
}

fn deserialize_base_branch_patch<'de, D>(deserializer: D) -> Result<BaseBranchPatch, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(|value| match value {
        Some(value) => BaseBranchPatch::Value(value),
        None => BaseBranchPatch::Null,
    })
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PartialGithubConfig {
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PartialHooksConfig {
    pub enabled: Option<bool>,
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PartialLocksConfig {
    pub timeout_ms: Option<u64>,
    pub stale_lock_ttl_seconds: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PartialListConfig {
    pub table: Option<PartialListTableConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PartialListTableConfig {
    pub columns: Option<Vec<ListTableColumn>>,
    pub path: Option<PartialListPathConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PartialListPathConfig {
    pub truncate: Option<ListPathTruncate>,
    pub min_width: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PartialSelectorConfig {
    pub cd: Option<PartialSelectorCdConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PartialSelectorCdConfig {
    pub prompt: Option<String>,
    pub surface: Option<SelectorCdSurface>,
    pub tmux_popup_opts: Option<String>,
    pub fzf: Option<PartialSelectorFzfConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PartialSelectorFzfConfig {
    pub extra_args: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedConfig {
    pub config: ResolvedConfig,
    pub loaded_files: Vec<PathBuf>,
}

pub fn parse_partial_config(path: &Path, source: &str) -> Result<PartialConfig, ConfigError> {
    if source.trim().is_empty() {
        return Ok(PartialConfig::default());
    }
    let options = serde_saphyr::options! {
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        strict_booleans: true,
    };
    let raw: serde_json::Value =
        serde_saphyr::from_str_with_options(source, options).map_err(|error| {
            ConfigError::Invalid {
                path: path.to_path_buf(),
                reason: error.to_string(),
            }
        })?;
    if raw.is_null() {
        return Ok(PartialConfig::default());
    }
    reject_disallowed_nulls(path, &raw, &mut Vec::new())?;
    let partial = serde_json::from_value(raw).map_err(|error| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    validate_partial(path, &partial)?;
    Ok(partial)
}

fn reject_disallowed_nulls(
    file: &Path,
    value: &serde_json::Value,
    key_path: &mut Vec<String>,
) -> Result<(), ConfigError> {
    match value {
        serde_json::Value::Null
            if !(key_path.len() == 2 && key_path[0] == "git" && key_path[1] == "baseBranch") =>
        {
            Err(ConfigError::Invalid {
                path: file.to_path_buf(),
                reason: format!("{} must not be null", key_path.join(".")),
            })
        }
        serde_json::Value::Object(record) => {
            for (key, item) in record {
                key_path.push(key.clone());
                reject_disallowed_nulls(file, item, key_path)?;
                key_path.pop();
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                key_path.push(index.to_string());
                reject_disallowed_nulls(file, item, key_path)?;
                key_path.pop();
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn merge_config(base: &mut ResolvedConfig, partial: PartialConfig) {
    if let Some(paths) = partial.paths
        && let Some(value) = paths.worktree_root
    {
        base.paths.worktree_root = value;
    }
    if let Some(git) = partial.git {
        match git.base_branch {
            BaseBranchPatch::Absent => {}
            BaseBranchPatch::Null => base.git.base_branch = None,
            BaseBranchPatch::Value(value) => base.git.base_branch = Some(value),
        }
        if let Some(value) = git.base_remote {
            base.git.base_remote = value;
        }
    }
    if let Some(github) = partial.github
        && let Some(value) = github.enabled
    {
        base.github.enabled = value;
    }
    if let Some(hooks) = partial.hooks {
        if let Some(value) = hooks.enabled {
            base.hooks.enabled = value;
        }
        if let Some(value) = hooks.timeout_ms {
            base.hooks.timeout_ms = value;
        }
    }
    if let Some(locks) = partial.locks {
        if let Some(value) = locks.timeout_ms {
            base.locks.timeout_ms = value;
        }
        if let Some(value) = locks.stale_lock_ttl_seconds {
            base.locks.stale_lock_ttl_seconds = value;
        }
    }
    if let Some(list) = partial.list
        && let Some(table) = list.table
    {
        if let Some(value) = table.columns {
            base.list.table.columns = value;
        }
        if let Some(path) = table.path {
            if let Some(value) = path.truncate {
                base.list.table.path.truncate = value;
            }
            if let Some(value) = path.min_width {
                base.list.table.path.min_width = value;
            }
        }
    }
    if let Some(selector) = partial.selector
        && let Some(cd) = selector.cd
    {
        if let Some(value) = cd.prompt {
            base.selector.cd.prompt = value;
        }
        if let Some(value) = cd.surface {
            base.selector.cd.surface = value;
        }
        if let Some(value) = cd.tmux_popup_opts {
            base.selector.cd.tmux_popup_opts = value;
        }
        if let Some(fzf) = cd.fzf
            && let Some(value) = fzf.extra_args
        {
            base.selector.cd.fzf.extra_args = value;
        }
    }
}

pub fn find_git_boundary_directory(cwd: &Path) -> Option<PathBuf> {
    let mut current = absolute_path(cwd);
    loop {
        let marker = current.join(".git");
        if fs::symlink_metadata(marker)
            .is_ok_and(|metadata| metadata.file_type().is_dir() || metadata.file_type().is_file())
        {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

pub fn collect_config_search_directories(cwd: &Path) -> Vec<PathBuf> {
    let absolute_cwd = absolute_path(cwd);
    let Some(boundary) = find_git_boundary_directory(&absolute_cwd) else {
        return vec![absolute_cwd];
    };
    let mut directories = Vec::new();
    let mut current = absolute_cwd;
    loop {
        directories.push(current.clone());
        if current == boundary || !current.pop() {
            break;
        }
    }
    directories.reverse();
    directories
}

pub fn load_resolved_config(cwd: &Path, repo_root: &Path) -> Result<LoadedConfig, ConfigError> {
    let global = resolve_global_config_path()?;
    load_resolved_config_with_global(cwd, repo_root, &global)
}

pub fn load_resolved_config_with_global(
    cwd: &Path,
    repo_root: &Path,
    global_config: &Path,
) -> Result<LoadedConfig, ConfigError> {
    let mut candidates = vec![global_config.to_path_buf(), local_config_path(repo_root)];
    candidates.extend(
        collect_config_search_directories(cwd)
            .into_iter()
            .map(|directory| local_config_path(&directory)),
    );

    let mut deduplicated = HashMap::new();
    for (order, candidate) in candidates.into_iter().enumerate() {
        if !candidate.exists() {
            continue;
        }
        let canonical = fs::canonicalize(&candidate).unwrap_or_else(|_| absolute_path(&candidate));
        deduplicated.insert(canonical, (order, candidate));
    }
    let mut ordered: Vec<_> = deduplicated.into_values().collect();
    ordered.sort_by_key(|(order, _)| *order);
    let files: Vec<_> = ordered.into_iter().map(|(_, path)| path).collect();

    let mut config = ResolvedConfig::default();
    for file in &files {
        let source = fs::read_to_string(file).map_err(|source| ConfigError::Io {
            path: file.clone(),
            source,
        })?;
        merge_config(&mut config, parse_partial_config(file, &source)?);
    }
    validate_worktree_root(repo_root, &config)?;
    Ok(LoadedConfig {
        config,
        loaded_files: files,
    })
}

fn validate_partial(path: &Path, partial: &PartialConfig) -> Result<(), ConfigError> {
    let invalid = |reason: &str| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    if let Some(paths) = &partial.paths {
        validate_non_empty(
            paths.worktree_root.as_deref(),
            "paths.worktreeRoot",
            &invalid,
        )?;
    }
    if let Some(git) = &partial.git {
        if let BaseBranchPatch::Value(value) = &git.base_branch {
            validate_non_empty(Some(value), "git.baseBranch", &invalid)?;
        }
        validate_non_empty(git.base_remote.as_deref(), "git.baseRemote", &invalid)?;
    }
    if let Some(hooks) = &partial.hooks {
        validate_positive(hooks.timeout_ms, "hooks.timeoutMs", &invalid)?;
    }
    if let Some(locks) = &partial.locks {
        validate_positive(locks.timeout_ms, "locks.timeoutMs", &invalid)?;
        validate_positive(
            locks.stale_lock_ttl_seconds,
            "locks.staleLockTTLSeconds",
            &invalid,
        )?;
    }
    if let Some(table) = partial.list.as_ref().and_then(|list| list.table.as_ref()) {
        if let Some(columns) = &table.columns {
            if columns.is_empty() {
                return Err(invalid("list.table.columns must not be empty"));
            }
            let unique: HashSet<_> = columns.iter().collect();
            if unique.len() != columns.len() {
                return Err(invalid("list.table.columns must not contain duplicates"));
            }
        }
        if let Some(width) = table.path.as_ref().and_then(|value| value.min_width)
            && !(8..=200).contains(&width)
        {
            return Err(invalid("list.table.path.minWidth must be in range 8..200"));
        }
    }
    if let Some(cd) = partial
        .selector
        .as_ref()
        .and_then(|selector| selector.cd.as_ref())
    {
        validate_non_empty(cd.prompt.as_deref(), "selector.cd.prompt", &invalid)?;
        validate_non_empty(
            cd.tmux_popup_opts.as_deref(),
            "selector.cd.tmuxPopupOpts",
            &invalid,
        )?;
        if let Some(args) = cd.fzf.as_ref().and_then(|fzf| fzf.extra_args.as_ref()) {
            validate_fzf_extra_args(args)
                .map_err(|error| invalid(&format!("selector.cd.fzf.extraArgs: {error}")))?;
        }
    }
    Ok(())
}

fn validate_non_empty<F>(value: Option<&str>, key: &str, invalid: &F) -> Result<(), ConfigError>
where
    F: Fn(&str) -> ConfigError,
{
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(invalid(&format!("{key} must be a non-empty string")));
    }
    Ok(())
}

fn validate_positive<F>(value: Option<u64>, key: &str, invalid: &F) -> Result<(), ConfigError>
where
    F: Fn(&str) -> ConfigError,
{
    if value == Some(0) {
        return Err(invalid(&format!("{key} must be a positive integer")));
    }
    Ok(())
}

fn validate_worktree_root(repo_root: &Path, config: &ResolvedConfig) -> Result<(), ConfigError> {
    let configured = Path::new(&config.paths.worktree_root);
    let resolved = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        repo_root.join(configured)
    };
    match fs::symlink_metadata(&resolved) {
        Ok(metadata) if !metadata.is_dir() => Err(ConfigError::Invalid {
            path: PathBuf::from("<resolved>"),
            reason: "paths.worktreeRoot must not point to an existing file".to_owned(),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ConfigError::Io {
            path: resolved,
            source,
        }),
    }
}

fn resolve_global_config_path() -> Result<PathBuf, ConfigError> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(absolute_path(Path::new(&xdg)).join_all(GLOBAL_CONFIG_RELATIVE_PATH));
    }
    let home = env::var_os("HOME").ok_or(ConfigError::HomeUnavailable)?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join_all(GLOBAL_CONFIG_RELATIVE_PATH))
}

fn local_config_path(directory: &Path) -> PathBuf {
    directory.join_all(CONFIG_RELATIVE_PATH)
}

fn absolute_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

trait JoinAll {
    fn join_all(&self, components: &[&str]) -> PathBuf;
}

impl JoinAll for Path {
    fn join_all(&self, components: &[&str]) -> PathBuf {
        components
            .iter()
            .fold(self.to_path_buf(), |path, component| path.join(component))
    }
}

impl JoinAll for PathBuf {
    fn join_all(&self, components: &[&str]) -> PathBuf {
        self.as_path().join_all(components)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_merge_unknown_range_and_empty_values() {
        let path = Path::new("config.yml");
        for source in [
            "hooks:\n  enabled: true\n  enabled: false\n",
            "base: &base\n  enabled: true\nhooks:\n  <<: *base\n",
            "unknown: true\n",
            "list:\n  table:\n    path:\n      minWidth: 7\n",
            "git:\n  baseRemote: '   '\n",
            "selector:\n  cd:\n    surface: popup\n",
            "hooks: null\n",
            "git:\n  baseRemote: null\n",
        ] {
            assert!(
                parse_partial_config(path, source).is_err(),
                "config should be rejected: {source}"
            );
        }
        assert!(
            parse_partial_config(path, "list:\n  table:\n    path:\n      minWidth: 200\n").is_ok()
        );
    }

    #[test]
    fn rejects_reserved_fzf_options_from_config() {
        for argument in ["--tmux=90%,90%", "--no-height", "--no-border"] {
            let source =
                format!("selector:\n  cd:\n    fzf:\n      extraArgs:\n        - {argument}\n");
            let error = parse_partial_config(Path::new("config.yml"), &source)
                .expect_err("reserved option must be rejected");

            assert!(error.to_string().contains("reserved fzf option"));
        }
    }

    #[test]
    fn distinguishes_absent_and_explicitly_null_base_branch() {
        let path = Path::new("config.yml");
        let mut config = ResolvedConfig::default();
        merge_config(
            &mut config,
            parse_partial_config(path, "git:\n  baseBranch: develop\n").unwrap(),
        );
        assert_eq!(config.git.base_branch.as_deref(), Some("develop"));
        merge_config(
            &mut config,
            parse_partial_config(path, "git:\n  baseRemote: fork\n").unwrap(),
        );
        assert_eq!(config.git.base_branch.as_deref(), Some("develop"));
        merge_config(
            &mut config,
            parse_partial_config(path, "git:\n  baseBranch: null\n").unwrap(),
        );
        assert_eq!(config.git.base_branch, None);
    }

    #[test]
    fn merges_global_repo_and_near_cwd_with_array_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let cwd = repo.join("packages/app");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir(repo.join(".git")).unwrap();
        let global = directory.path().join("global.yml");
        fs::write(
            &global,
            "git:\n  baseRemote: upstream\nlist:\n  table:\n    columns: [branch, path]\n",
        )
        .unwrap();
        let repo_config = local_config_path(&repo);
        fs::create_dir_all(repo_config.parent().unwrap()).unwrap();
        fs::write(&repo_config, "github:\n  enabled: false\n").unwrap();
        let near_config = local_config_path(&cwd);
        fs::create_dir_all(near_config.parent().unwrap()).unwrap();
        fs::write(
            &near_config,
            "git:\n  baseRemote: fork\nlist:\n  table:\n    columns: [dirty]\n",
        )
        .unwrap();

        let loaded = load_resolved_config_with_global(&cwd, &repo, &global).unwrap();
        assert_eq!(loaded.config.git.base_remote, "fork");
        assert!(!loaded.config.github.enabled);
        assert_eq!(
            loaded.config.list.table.columns,
            vec![ListTableColumn::Dirty]
        );
        assert_eq!(loaded.loaded_files, vec![global, repo_config, near_config]);
    }

    #[test]
    fn search_stops_at_git_boundary_and_worktree_root_rejects_files() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let cwd = repo.join("nested");
        fs::create_dir_all(&cwd).unwrap();
        fs::write(repo.join(".git"), "gitdir: elsewhere\n").unwrap();
        assert_eq!(
            collect_config_search_directories(&cwd),
            vec![repo.clone(), cwd]
        );

        fs::write(repo.join("worktrees"), "not a directory").unwrap();
        let global = directory.path().join("missing-global.yml");
        let config = local_config_path(&repo);
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "paths:\n  worktreeRoot: worktrees\n").unwrap();
        assert!(load_resolved_config_with_global(&repo, &repo, &global).is_err());
    }
}
