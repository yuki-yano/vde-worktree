use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use crate::domain::worktree::{
    GitWorktree, PrState, PrStatus, SnapshotWarning, SnapshotWarningCode, WorktreeLockState,
    WorktreeMergedState, WorktreeSnapshot, WorktreeStatus, WorktreeUpstreamState,
};
use crate::ports::process::ProcessOutput;
use crate::ports::snapshot::{GitSnapshotPort, PrStateLookup};
use crate::state::json_store::JsonRecordState;
use crate::state::lifecycle::{
    LifecycleObservationGuard, WorktreeLifecycleRecord, lifecycle_file_path,
    merge_lifecycle_observation_locked, read_worktree_lifecycle,
};
use crate::state::worktree_lock::read_worktree_lock;

const BRANCH_PREFIX: &[u8] = b"refs/heads/";
const SNAPSHOT_WORKER_LIMIT: usize = 4;
const WORK_REFLOG_PREFIXES: &[&str] = &[
    "commit:",
    "commit (",
    "cherry-pick:",
    "revert:",
    "rebase (pick):",
    "merge:",
];

#[derive(Debug)]
pub enum SnapshotError {
    Git(Box<SnapshotGitFailure>),
    InvalidPorcelain(WorktreePorcelainError),
    BaseBranchUnavailable { remote: String },
}

#[derive(Debug)]
pub struct SnapshotGitFailure {
    pub cwd: PathBuf,
    pub argv: Vec<OsString>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub cause: Option<String>,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Git(failure) => write!(
                formatter,
                "Git status probe failed in {} for {:?} (exit code: {:?}, timed out: {})",
                failure.cwd.display(),
                failure.argv,
                failure.exit_code,
                failure.timed_out
            ),
            Self::InvalidPorcelain(error) => {
                write!(formatter, "invalid Git worktree output: {error}")
            }
            Self::BaseBranchUnavailable { remote } => write!(
                formatter,
                "unable to resolve base branch from {remote}/HEAD, main, or master"
            ),
        }
    }
}

impl std::error::Error for SnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidPorcelain(error) => Some(error),
            Self::Git(_) | Self::BaseBranchUnavailable { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreePorcelainError {
    pub field_index: usize,
    pub reason: String,
}

impl fmt::Display for WorktreePorcelainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "field {}: {}", self.field_index, self.reason)
    }
}

impl std::error::Error for WorktreePorcelainError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotCollectionOptions {
    persist_lifecycle_observations: bool,
    include_upstream: bool,
}

impl SnapshotCollectionOptions {
    pub const fn monitor() -> Self {
        Self {
            persist_lifecycle_observations: false,
            include_upstream: false,
        }
    }
}

impl Default for SnapshotCollectionOptions {
    fn default() -> Self {
        Self {
            persist_lifecycle_observations: true,
            include_upstream: true,
        }
    }
}

pub struct SnapshotCollector<'a, G, P> {
    git: &'a G,
    pr_lookup: &'a P,
    options: SnapshotCollectionOptions,
}

impl<'a, G, P> SnapshotCollector<'a, G, P>
where
    G: GitSnapshotPort,
    P: PrStateLookup,
{
    pub const fn new(git: &'a G, pr_lookup: &'a P) -> Self {
        Self {
            git,
            pr_lookup,
            options: SnapshotCollectionOptions {
                persist_lifecycle_observations: true,
                include_upstream: true,
            },
        }
    }

    #[must_use]
    pub const fn with_options(mut self, options: SnapshotCollectionOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub const fn without_lifecycle_observations(mut self) -> Self {
        self.options.persist_lifecycle_observations = false;
        self
    }

    pub fn collect(
        &self,
        repo_root: &Path,
        base_branch: &str,
        gh_enabled: bool,
    ) -> Result<WorktreeSnapshot, SnapshotError> {
        let output = run_git(
            self.git,
            repo_root,
            ["worktree", "list", "--porcelain", "-z"],
        )?;
        require_success(
            &output,
            repo_root,
            ["worktree", "list", "--porcelain", "-z"],
        )?;
        let mut worktrees =
            parse_worktree_porcelain(&output.stdout).map_err(SnapshotError::InvalidPorcelain)?;
        if let Some(primary) = worktrees.first_mut() {
            primary.path = repo_root.to_path_buf();
        }
        let branches = worktrees
            .iter()
            .map(|worktree| worktree.branch.clone())
            .collect::<Vec<_>>();
        let pr_states =
            self.pr_lookup
                .resolve_pr_states(repo_root, Some(base_branch), &branches, gh_enabled);
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel();
        thread::scope(|scope| {
            for _ in 0..worktrees.len().min(SNAPSHOT_WORKER_LIMIT) {
                let sender = sender.clone();
                let next = &next;
                let worktrees = &worktrees;
                let pr_states = &pr_states;
                scope.spawn(move || {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(worktree) = worktrees.get(index) else {
                            break;
                        };
                        let mut warnings = Vec::new();
                        let result = self
                            .enrich_worktree(
                                repo_root,
                                base_branch,
                                worktree,
                                pr_states,
                                &mut warnings,
                            )
                            .map(|status| (status, warnings));
                        if sender.send((index, result)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
        });
        let mut indexed = (0..worktrees.len()).map(|_| None).collect::<Vec<_>>();
        for (index, result) in receiver {
            indexed[index] = Some(result);
        }
        let mut statuses = Vec::with_capacity(worktrees.len());
        let mut warnings = Vec::new();
        for result in indexed {
            let (status, mut worktree_warnings) =
                result.expect("every snapshot worker returns one result")?;
            statuses.push(status);
            warnings.append(&mut worktree_warnings);
        }
        Ok(WorktreeSnapshot {
            repo_root: repo_root.to_path_buf(),
            base_branch: Some(base_branch.to_owned()),
            worktrees: statuses,
            warnings,
        })
    }

    fn enrich_worktree(
        &self,
        repo_root: &Path,
        base_branch: &str,
        worktree: &GitWorktree,
        pr_states: &HashMap<String, PrState>,
        warnings: &mut Vec<SnapshotWarning>,
    ) -> Result<WorktreeStatus, SnapshotError> {
        if worktree.bare {
            return Ok(WorktreeStatus {
                branch: None,
                path: worktree.path.clone(),
                head: worktree.head.clone(),
                dirty: false,
                locked: unlocked(),
                merged: unknown_merged(),
                pr: PrState {
                    status: None,
                    url: None,
                },
                upstream: unknown_upstream(),
            });
        }
        let dirty = self.resolve_dirty(&worktree.path)?;
        let locked = resolve_lock(
            repo_root,
            worktree.branch.as_deref(),
            worktree.locked,
            warnings,
        );
        let pr = resolve_pr(worktree.branch.as_deref(), base_branch, pr_states);
        let merged = self.resolve_merged(repo_root, base_branch, worktree, &pr, warnings)?;
        let upstream = if self.options.include_upstream {
            self.resolve_upstream(&worktree.path)?
        } else {
            unknown_upstream()
        };
        Ok(WorktreeStatus {
            branch: worktree.branch.clone(),
            path: worktree.path.clone(),
            head: worktree.head.clone(),
            dirty,
            locked,
            merged,
            pr,
            upstream,
        })
    }

    fn resolve_dirty(&self, worktree_path: &Path) -> Result<bool, SnapshotError> {
        let output = run_git(self.git, worktree_path, ["status", "--porcelain"])?;
        require_success(&output, worktree_path, ["status", "--porcelain"])?;
        Ok(!trim_ascii_whitespace(&output.stdout).is_empty())
    }

    fn resolve_upstream(
        &self,
        worktree_path: &Path,
    ) -> Result<WorktreeUpstreamState, SnapshotError> {
        let upstream = run_git(
            self.git,
            worktree_path,
            [
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
        )?;
        if upstream.timed_out || upstream.exit_code != Some(0) {
            return Ok(unknown_upstream());
        }
        let remote = String::from_utf8_lossy(trim_ascii_whitespace(&upstream.stdout)).into_owned();
        if remote.is_empty() {
            return Ok(unknown_upstream());
        }
        let distance = run_git(
            self.git,
            worktree_path,
            ["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
        )?;
        if distance.timed_out || distance.exit_code != Some(0) {
            return Ok(WorktreeUpstreamState {
                ahead: None,
                behind: None,
                remote: Some(remote),
            });
        }
        let distance = String::from_utf8_lossy(trim_ascii_whitespace(&distance.stdout));
        let mut fields = distance.split_ascii_whitespace();
        let behind = fields.next().and_then(|value| value.parse().ok());
        let ahead = fields.next().and_then(|value| value.parse().ok());
        Ok(WorktreeUpstreamState {
            ahead,
            behind,
            remote: Some(remote),
        })
    }

    fn resolve_merged(
        &self,
        repo_root: &Path,
        base_branch: &str,
        worktree: &GitWorktree,
        pr: &PrState,
        warnings: &mut Vec<SnapshotWarning>,
    ) -> Result<WorktreeMergedState, SnapshotError> {
        let Some(branch) = worktree.branch.as_deref() else {
            return Ok(unknown_merged());
        };
        let lifecycle_target = LifecycleTarget {
            branch,
            head: &worktree.head,
        };
        let by_ancestry = self.probe_ancestry(repo_root, branch, base_branch)?;
        let by_pr = match pr.status {
            Some(PrStatus::Merged) => Some(true),
            Some(PrStatus::None | PrStatus::Open | PrStatus::ClosedUnmerged) => Some(false),
            Some(PrStatus::Unknown) | None => None,
        };
        let evidence = MergeEvidence { by_ancestry, by_pr };

        let read = read_worktree_lifecycle(repo_root, branch);
        let by_lifecycle = match read.state {
            JsonRecordState::Invalid { reason } => {
                warnings.push(SnapshotWarning {
                    code: SnapshotWarningCode::InvalidLifecycle,
                    message: format!("invalid lifecycle metadata was preserved: {reason}"),
                    branch: Some(branch.to_owned()),
                    path: Some(read.path),
                });
                None
            }
            state => {
                let current = match state {
                    JsonRecordState::Missing => None,
                    JsonRecordState::Valid(record) => Some(record),
                    JsonRecordState::Invalid { .. } => unreachable!("handled above"),
                };
                let observed_diverged_head =
                    (by_ancestry == Some(false)).then_some(worktree.head.as_str());
                let lifecycle = if self.options.persist_lifecycle_observations {
                    self.persist_lifecycle_observation(
                        repo_root,
                        lifecycle_target,
                        base_branch,
                        observed_diverged_head,
                        current,
                        warnings,
                    )?
                } else {
                    current
                };
                self.resolve_valid_lifecycle(
                    repo_root,
                    lifecycle_target,
                    base_branch,
                    evidence,
                    lifecycle,
                    warnings,
                )?
            }
        };
        Ok(WorktreeMergedState {
            by_ancestry,
            by_pr,
            overall: resolve_merged_overall(by_ancestry, by_pr, by_lifecycle),
        })
    }

    fn resolve_valid_lifecycle(
        &self,
        repo_root: &Path,
        target: LifecycleTarget<'_>,
        base_branch: &str,
        evidence: MergeEvidence,
        lifecycle: Option<WorktreeLifecycleRecord>,
        warnings: &mut Vec<SnapshotWarning>,
    ) -> Result<Option<bool>, SnapshotError> {
        if evidence.by_ancestry == Some(false) {
            return Ok(Some(false));
        }
        if evidence.by_ancestry != Some(true) {
            return Ok(None);
        }
        if let Some(record) = &lifecycle
            && record.ever_diverged
            && let Some(diverged_head) = &record.last_diverged_head
        {
            return self.probe_ancestry(repo_root, diverged_head, base_branch);
        }
        if evidence.by_pr == Some(true) {
            return Ok(None);
        }
        let reflog = self.probe_lifecycle_from_reflog(repo_root, target.branch, base_branch)?;
        if self.options.persist_lifecycle_observations
            && let Some(diverged_head) = reflog.diverged_head.as_deref()
        {
            let _ = self.persist_lifecycle_observation(
                repo_root,
                target,
                base_branch,
                Some(diverged_head),
                lifecycle,
                warnings,
            )?;
        }
        Ok(reflog.merged)
    }

    fn persist_lifecycle_observation(
        &self,
        repo_root: &Path,
        target: LifecycleTarget<'_>,
        base_branch: &str,
        observed_diverged_head: Option<&str>,
        current: Option<WorktreeLifecycleRecord>,
        warnings: &mut Vec<SnapshotWarning>,
    ) -> Result<Option<WorktreeLifecycleRecord>, SnapshotError> {
        if !repo_root.join(".vde/worktree/state").is_dir() {
            return Ok(current);
        }
        let guard = match LifecycleObservationGuard::acquire(repo_root) {
            Ok(guard) => guard,
            Err(error) => {
                push_lifecycle_observation_warning(repo_root, target.branch, error, warnings);
                return Ok(current);
            }
        };
        let reference = format!("refs/heads/{}", target.branch);
        let output = run_git(
            self.git,
            repo_root,
            ["rev-parse", "--verify", reference.as_str()],
        )?;
        if output.timed_out
            || output.exit_code != Some(0)
            || trim_ascii_whitespace(&output.stdout) != target.head.as_bytes()
        {
            return Ok(current);
        }
        match merge_lifecycle_observation_locked(
            &guard,
            repo_root,
            target.branch,
            base_branch,
            observed_diverged_head,
        ) {
            Ok(record) => Ok(Some(record)),
            Err(error) => {
                push_lifecycle_observation_warning(repo_root, target.branch, error, warnings);
                Ok(current)
            }
        }
    }

    fn probe_ancestry(
        &self,
        repo_root: &Path,
        ancestor: &str,
        descendant: &str,
    ) -> Result<Option<bool>, SnapshotError> {
        let output = run_git(
            self.git,
            repo_root,
            ["merge-base", "--is-ancestor", ancestor, descendant],
        )?;
        Ok(match (output.timed_out, output.exit_code) {
            (false, Some(0)) => Some(true),
            (false, Some(1)) => Some(false),
            _ => None,
        })
    }

    fn probe_lifecycle_from_reflog(
        &self,
        repo_root: &Path,
        branch: &str,
        base_branch: &str,
    ) -> Result<ReflogProbe, SnapshotError> {
        let output = run_git(
            self.git,
            repo_root,
            ["reflog", "show", "--format=%H%x09%gs", branch],
        )?;
        if output.timed_out || output.exit_code != Some(0) {
            return Ok(ReflogProbe::default());
        }
        let heads = parse_work_reflog_heads(&String::from_utf8_lossy(&output.stdout));
        if heads.is_empty() {
            return Ok(ReflogProbe::default());
        }
        let latest_head = heads.first().cloned();
        for head in &heads {
            match self.probe_ancestry(repo_root, head, base_branch)? {
                Some(true) => {
                    return Ok(ReflogProbe {
                        merged: Some(true),
                        diverged_head: Some(head.clone()),
                    });
                }
                None => {
                    return Ok(ReflogProbe {
                        merged: None,
                        diverged_head: latest_head,
                    });
                }
                Some(false) => {}
            }
        }
        Ok(ReflogProbe {
            merged: Some(false),
            diverged_head: latest_head,
        })
    }
}

pub fn resolve_base_branch<G: GitSnapshotPort>(
    git: &G,
    repo_root: &Path,
    configured: Option<&str>,
    base_remote: &str,
) -> Result<String, SnapshotError> {
    if let Some(configured) = configured.filter(|branch| !branch.is_empty()) {
        return Ok(configured.to_owned());
    }
    let remote_head = format!("refs/remotes/{base_remote}/HEAD");
    let output = run_git(
        git,
        repo_root,
        ["symbolic-ref", "--quiet", "--short", remote_head.as_str()],
    )?;
    if !output.timed_out && output.exit_code == Some(0) {
        let resolved = String::from_utf8_lossy(trim_ascii_whitespace(&output.stdout));
        if let Some(branch) = resolved.strip_prefix(&format!("{base_remote}/"))
            && !branch.is_empty()
        {
            return Ok(branch.to_owned());
        }
    }
    for candidate in ["main", "master"] {
        let reference = format!("refs/heads/{candidate}");
        let output = run_git(
            git,
            repo_root,
            ["show-ref", "--verify", "--quiet", reference.as_str()],
        )?;
        if !output.timed_out && output.exit_code == Some(0) {
            return Ok(candidate.to_owned());
        }
    }
    Err(SnapshotError::BaseBranchUnavailable {
        remote: base_remote.to_owned(),
    })
}

pub fn resolve_distance_from_base<G: GitSnapshotPort>(
    git: &G,
    repo_root: &Path,
    base_branch: &str,
    target_ref: &str,
) -> Result<(Option<i64>, Option<i64>), SnapshotError> {
    let range = format!("{base_branch}...{target_ref}");
    let output = run_git(
        git,
        repo_root,
        ["rev-list", "--left-right", "--count", range.as_str()],
    )?;
    if output.timed_out || output.exit_code != Some(0) {
        return Ok((None, None));
    }
    let text = String::from_utf8_lossy(trim_ascii_whitespace(&output.stdout));
    let mut fields = text.split_ascii_whitespace();
    let behind = fields.next().and_then(|value| value.parse::<i64>().ok());
    let ahead = fields.next().and_then(|value| value.parse::<i64>().ok());
    Ok((ahead, behind))
}

pub fn parse_worktree_porcelain(raw: &[u8]) -> Result<Vec<GitWorktree>, WorktreePorcelainError> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    if !raw.ends_with(&[0]) {
        return Err(parse_error(0, "output is not NUL terminated"));
    }
    let mut worktrees = Vec::new();
    let mut current: Option<PorcelainRecord> = None;
    for (index, field) in raw.split(|byte| *byte == 0).enumerate() {
        if field.is_empty() {
            if let Some(record) = current.take() {
                worktrees.push(record.finish(index)?);
            }
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            if path.is_empty() {
                return Err(parse_error(index, "worktree path is empty"));
            }
            if current.is_some() {
                return Err(parse_error(index, "record is missing a NUL separator"));
            }
            current = Some(PorcelainRecord::new(path_from_bytes(path)));
            continue;
        }
        let Some(record) = current.as_mut() else {
            return Err(parse_error(index, "field appears before worktree path"));
        };
        if let Some(head) = field.strip_prefix(b"HEAD ") {
            if head.is_empty() || record.head.replace(bytes_to_string(head)).is_some() {
                return Err(parse_error(index, "HEAD is empty or duplicated"));
            }
        } else if let Some(branch) = field.strip_prefix(b"branch ") {
            let Some(branch) = branch.strip_prefix(BRANCH_PREFIX) else {
                return Err(parse_error(index, "branch is not under refs/heads"));
            };
            if branch.is_empty() || record.branch.replace(bytes_to_string(branch)).is_some() {
                return Err(parse_error(index, "branch is empty or duplicated"));
            }
        } else if field == b"detached" {
            if record.detached || record.branch.is_some() {
                return Err(parse_error(index, "detached conflicts with branch state"));
            }
            record.detached = true;
        } else if field == b"bare" {
            record.bare = true;
        } else if field == b"locked" || field.starts_with(b"locked ") {
            record.locked = true;
        } else if field == b"prunable" || field.starts_with(b"prunable ") {
            record.prunable = true;
        } else {
            return Err(parse_error(index, "unknown porcelain field"));
        }
    }
    if current.is_some() {
        return Err(parse_error(0, "record was not terminated"));
    }
    Ok(worktrees)
}

pub const fn resolve_merged_overall(
    by_ancestry: Option<bool>,
    by_pr: Option<bool>,
    by_lifecycle: Option<bool>,
) -> Option<bool> {
    if matches!(by_pr, Some(true)) || matches!(by_lifecycle, Some(true)) {
        return Some(true);
    }
    if matches!(by_ancestry, Some(false))
        || matches!(by_pr, Some(false))
        || matches!(by_lifecycle, Some(false))
    {
        return Some(false);
    }
    None
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct PorcelainRecord {
    path: PathBuf,
    head: Option<String>,
    branch: Option<String>,
    detached: bool,
    bare: bool,
    locked: bool,
    prunable: bool,
}

impl PorcelainRecord {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }

    fn finish(self, index: usize) -> Result<GitWorktree, WorktreePorcelainError> {
        if self.branch.is_some() && self.detached {
            return Err(parse_error(index, "record is both attached and detached"));
        }
        if self.bare && (self.branch.is_some() || self.detached) {
            return Err(parse_error(index, "bare record has branch state"));
        }
        let head = match (self.head, self.bare) {
            (Some(head), _) => head,
            (None, true) => String::new(),
            (None, false) => return Err(parse_error(index, "record has no HEAD")),
        };
        Ok(GitWorktree {
            path: self.path,
            head,
            branch: self.branch,
            bare: self.bare,
            locked: self.locked,
            prunable: self.prunable,
        })
    }
}

#[derive(Default)]
struct ReflogProbe {
    merged: Option<bool>,
    diverged_head: Option<String>,
}

#[derive(Clone, Copy)]
struct MergeEvidence {
    by_ancestry: Option<bool>,
    by_pr: Option<bool>,
}

#[derive(Clone, Copy)]
struct LifecycleTarget<'a> {
    branch: &'a str,
    head: &'a str,
}

fn push_lifecycle_observation_warning(
    repo_root: &Path,
    branch: &str,
    error: impl fmt::Display,
    warnings: &mut Vec<SnapshotWarning>,
) {
    if !warnings.iter().any(|warning| {
        warning.code == SnapshotWarningCode::LifecycleObservationFailed
            && warning.branch.as_deref() == Some(branch)
    }) {
        warnings.push(SnapshotWarning {
            code: SnapshotWarningCode::LifecycleObservationFailed,
            message: format!("failed to persist lifecycle observation: {error}"),
            branch: Some(branch.to_owned()),
            path: Some(lifecycle_file_path(repo_root, branch)),
        });
    }
}

fn parse_work_reflog_heads(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| {
            let (head, message) = line.trim().split_once('\t')?;
            (!head.is_empty()
                && WORK_REFLOG_PREFIXES
                    .iter()
                    .any(|prefix| message.trim().starts_with(prefix)))
            .then(|| head.to_owned())
        })
        .collect()
}

fn resolve_lock(
    repo_root: &Path,
    branch: Option<&str>,
    git_native_locked: bool,
    warnings: &mut Vec<SnapshotWarning>,
) -> WorktreeLockState {
    let Some(branch) = branch else {
        return if git_native_locked {
            git_native_lock()
        } else {
            unlocked()
        };
    };
    let read = read_worktree_lock(repo_root, branch);
    match read.state {
        JsonRecordState::Missing if git_native_locked => git_native_lock(),
        JsonRecordState::Missing => unlocked(),
        JsonRecordState::Valid(record) => WorktreeLockState {
            value: true,
            reason: Some(record.reason),
            owner: Some(record.owner),
        },
        JsonRecordState::Invalid { reason } => {
            warnings.push(SnapshotWarning {
                code: SnapshotWarningCode::InvalidLock,
                message: format!("invalid lock metadata was treated as locked: {reason}"),
                branch: Some(branch.to_owned()),
                path: Some(read.path),
            });
            WorktreeLockState {
                value: true,
                reason: Some("invalid lock metadata".to_owned()),
                owner: None,
            }
        }
    }
}

fn git_native_lock() -> WorktreeLockState {
    WorktreeLockState {
        value: true,
        reason: Some("git worktree lock".to_owned()),
        owner: None,
    }
}

fn resolve_pr(
    branch: Option<&str>,
    base_branch: &str,
    states: &HashMap<String, PrState>,
) -> PrState {
    match branch {
        None => PrState {
            status: None,
            url: None,
        },
        Some(branch) if branch == base_branch => PrState {
            status: None,
            url: None,
        },
        Some(branch) => states.get(branch).cloned().unwrap_or_else(PrState::unknown),
    }
}

const fn unlocked() -> WorktreeLockState {
    WorktreeLockState {
        value: false,
        reason: None,
        owner: None,
    }
}

const fn unknown_upstream() -> WorktreeUpstreamState {
    WorktreeUpstreamState {
        ahead: None,
        behind: None,
        remote: None,
    }
}

const fn unknown_merged() -> WorktreeMergedState {
    WorktreeMergedState {
        by_ancestry: None,
        by_pr: None,
        overall: None,
    }
}

fn run_git<G, I, S>(git: &G, cwd: &Path, args: I) -> Result<ProcessOutput, SnapshotError>
where
    G: GitSnapshotPort,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let command_argv = args
        .into_iter()
        .map(|argument| argument.as_ref().to_os_string())
        .collect::<Vec<_>>();
    git.run_git(cwd, &command_argv).map_err(|error| {
        SnapshotError::Git(Box::new(SnapshotGitFailure {
            cwd: cwd.to_path_buf(),
            argv: command_argv,
            exit_code: None,
            timed_out: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
            cause: Some(error.to_string()),
        }))
    })
}

fn require_success<I, S>(output: &ProcessOutput, cwd: &Path, args: I) -> Result<(), SnapshotError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    if !output.timed_out && output.exit_code == Some(0) {
        Ok(())
    } else {
        Err(SnapshotError::Git(Box::new(SnapshotGitFailure {
            cwd: cwd.to_path_buf(),
            argv: args
                .into_iter()
                .map(|argument| argument.as_ref().to_os_string())
                .collect(),
            exit_code: output.exit_code,
            timed_out: output.timed_out,
            stdout: output.stdout.clone(),
            stderr: output.stderr.clone(),
            cause: None,
        })))
    }
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

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;

    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse_error(field_index: usize, reason: &str) -> WorktreePorcelainError {
    WorktreePorcelainError {
        field_index,
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};

    use super::*;
    use crate::adapters::git_cli::GitCli;
    use crate::adapters::process::StdProcessRunner;
    use crate::state::lifecycle::lifecycle_file_path;

    struct NoPrLookup;

    impl PrStateLookup for NoPrLookup {
        fn resolve_pr_states(
            &self,
            _repo_root: &Path,
            base_branch: Option<&str>,
            branches: &[Option<String>],
            _enabled: bool,
        ) -> HashMap<String, PrState> {
            let Some(base) = base_branch else {
                return HashMap::new();
            };
            branches
                .iter()
                .filter_map(Option::as_ref)
                .filter(|branch| branch.as_str() != base)
                .map(|branch| (branch.clone(), PrState::unknown()))
                .collect()
        }
    }

    struct ParallelGit {
        worktree_output: Vec<u8>,
        barrier: Option<Arc<Barrier>>,
        active: AtomicUsize,
        maximum: AtomicUsize,
        failure_index: Option<usize>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct RecordedGitCommand {
        cwd: PathBuf,
        args: Vec<String>,
    }

    struct RecordingGit {
        worktree_output: Vec<u8>,
        commands: Mutex<Vec<RecordedGitCommand>>,
    }

    impl RecordingGit {
        fn with_two_worktrees(repo_root: &Path) -> Self {
            let feature_path = repo_root.join("feature");
            Self {
                worktree_output: format!(
                    "worktree {}\0HEAD {:040}\0branch refs/heads/main\0\0\
                     worktree {}\0HEAD {:040}\0branch refs/heads/feature/monitor\0\0",
                    repo_root.display(),
                    1,
                    feature_path.display(),
                    2
                )
                .into_bytes(),
                commands: Mutex::new(Vec::new()),
            }
        }

        fn commands(&self) -> Vec<RecordedGitCommand> {
            self.commands.lock().unwrap().clone()
        }
    }

    #[derive(Debug)]
    struct FakeSnapshotError;

    impl fmt::Display for FakeSnapshotError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("injected snapshot failure")
        }
    }

    impl std::error::Error for FakeSnapshotError {}

    impl GitSnapshotPort for ParallelGit {
        type Error = FakeSnapshotError;

        fn run_git<I, S>(&self, cwd: &Path, args: I) -> Result<ProcessOutput, Self::Error>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let args = args
                .into_iter()
                .map(|arg| arg.as_ref().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            if args.first().is_some_and(|arg| arg == "worktree") {
                return Ok(success(self.worktree_output.clone()));
            }
            if args.first().is_some_and(|arg| arg == "status") {
                if self
                    .failure_index
                    .is_some_and(|index| cwd.ends_with(format!("wt-{index}")))
                {
                    return Err(FakeSnapshotError);
                }
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.maximum.fetch_max(active, Ordering::SeqCst);
                if let Some(barrier) = &self.barrier {
                    barrier.wait();
                }
                self.active.fetch_sub(1, Ordering::SeqCst);
                return Ok(success(Vec::new()));
            }
            if args.first().is_some_and(|arg| arg == "merge-base") {
                return Ok(ProcessOutput {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    exit_code: Some(1),
                    timed_out: false,
                });
            }
            if args.first().is_some_and(|arg| arg == "rev-parse") {
                let _ = cwd;
                return Ok(ProcessOutput {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    exit_code: Some(1),
                    timed_out: false,
                });
            }
            unreachable!("unexpected Git probe: {args:?}")
        }
    }

    impl GitSnapshotPort for RecordingGit {
        type Error = FakeSnapshotError;

        fn run_git<I, S>(&self, cwd: &Path, args: I) -> Result<ProcessOutput, Self::Error>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let args = args
                .into_iter()
                .map(|arg| arg.as_ref().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            self.commands.lock().unwrap().push(RecordedGitCommand {
                cwd: cwd.to_path_buf(),
                args: args.clone(),
            });
            match args.first().map(String::as_str) {
                Some("worktree") => Ok(success(self.worktree_output.clone())),
                Some("status") => Ok(success(Vec::new())),
                Some("merge-base") => Ok(failure(1)),
                Some("rev-parse") if args.last().is_some_and(|arg| arg == "@{upstream}") => {
                    Ok(success(b"origin/main\n".to_vec()))
                }
                Some("rev-list") if args.last().is_some_and(|arg| arg == "@{upstream}...HEAD") => {
                    Ok(success(b"0\t0\n".to_vec()))
                }
                _ => unreachable!("unexpected Git probe: {args:?}"),
            }
        }
    }

    fn success(stdout: Vec<u8>) -> ProcessOutput {
        ProcessOutput {
            stdout,
            stderr: Vec::new(),
            exit_code: Some(0),
            timed_out: false,
        }
    }

    fn failure(exit_code: i32) -> ProcessOutput {
        ProcessOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: Some(exit_code),
            timed_out: false,
        }
    }

    #[test]
    fn monitor_profile_skips_exactly_two_upstream_probes_per_worktree() {
        let directory = tempfile::tempdir().unwrap();
        let repo_root = directory.path();
        let normal_git = RecordingGit::with_two_worktrees(repo_root);
        let normal = SnapshotCollector::new(&normal_git, &NoPrLookup)
            .without_lifecycle_observations()
            .collect(repo_root, "main", false)
            .unwrap();
        let monitor_git = RecordingGit::with_two_worktrees(repo_root);
        let monitor = SnapshotCollector::new(&monitor_git, &NoPrLookup)
            .with_options(SnapshotCollectionOptions::monitor())
            .collect(repo_root, "main", false)
            .unwrap();

        let normal_commands = normal_git.commands();
        let mut monitor_commands = monitor_git.commands();
        assert_eq!(normal_commands.len(), monitor_commands.len() + 4);
        assert_eq!(
            normal_commands
                .iter()
                .filter(|command| {
                    command.args.first().is_some_and(|arg| arg == "rev-parse")
                        && command.args.last().is_some_and(|arg| arg == "@{upstream}")
                })
                .count(),
            2
        );
        assert_eq!(
            normal_commands
                .iter()
                .filter(|command| {
                    command.args.first().is_some_and(|arg| arg == "rev-list")
                        && command
                            .args
                            .last()
                            .is_some_and(|arg| arg == "@{upstream}...HEAD")
                })
                .count(),
            2
        );
        assert!(
            monitor_commands
                .iter()
                .all(|command| { !command.args.iter().any(|arg| arg.contains("@{upstream}")) })
        );
        assert!(
            normal
                .worktrees
                .iter()
                .all(|worktree| worktree.upstream.remote.as_deref() == Some("origin/main"))
        );
        assert!(
            monitor
                .worktrees
                .iter()
                .all(|worktree| worktree.upstream == unknown_upstream())
        );

        monitor_commands.sort();
        let feature_path = repo_root.join("feature");
        let mut expected = vec![
            RecordedGitCommand {
                cwd: repo_root.to_path_buf(),
                args: vec![
                    "worktree".into(),
                    "list".into(),
                    "--porcelain".into(),
                    "-z".into(),
                ],
            },
            RecordedGitCommand {
                cwd: repo_root.to_path_buf(),
                args: vec!["status".into(), "--porcelain".into()],
            },
            RecordedGitCommand {
                cwd: repo_root.to_path_buf(),
                args: vec![
                    "merge-base".into(),
                    "--is-ancestor".into(),
                    "main".into(),
                    "main".into(),
                ],
            },
            RecordedGitCommand {
                cwd: feature_path.clone(),
                args: vec!["status".into(), "--porcelain".into()],
            },
            RecordedGitCommand {
                cwd: repo_root.to_path_buf(),
                args: vec![
                    "merge-base".into(),
                    "--is-ancestor".into(),
                    "feature/monitor".into(),
                    "main".into(),
                ],
            },
        ];
        expected.sort();
        assert_eq!(monitor_commands, expected);
    }

    #[test]
    fn monitor_profile_does_not_write_lifecycle_observations_when_state_exists() {
        let directory = tempfile::tempdir().unwrap();
        let repo_root = directory.path();
        let state_root = repo_root.join(".vde/worktree/state");
        fs::create_dir_all(&state_root).unwrap();
        let git = RecordingGit::with_two_worktrees(repo_root);

        SnapshotCollector::new(&git, &NoPrLookup)
            .with_options(SnapshotCollectionOptions::monitor())
            .collect(repo_root, "main", false)
            .unwrap();

        assert_eq!(fs::read_dir(&state_root).unwrap().count(), 0);
        assert!(git.commands().iter().all(|command| {
            !(command.args.first().is_some_and(|arg| arg == "rev-parse")
                && command.args.iter().any(|arg| arg == "--verify"))
        }));
    }

    #[test]
    fn snapshot_probes_use_four_workers_and_preserve_worktree_order() {
        let directory = tempfile::tempdir().unwrap();
        let mut raw = Vec::new();
        for index in 0..8 {
            raw.extend_from_slice(
                format!(
                    "worktree {}/wt-{index}\0HEAD {index:040}\0branch refs/heads/feature/{index}\0\0",
                    directory.path().display()
                )
                .as_bytes(),
            );
            let lifecycle = lifecycle_file_path(directory.path(), &format!("feature/{index}"));
            fs::create_dir_all(lifecycle.parent().unwrap()).unwrap();
            fs::write(lifecycle, "{invalid}\n").unwrap();
        }
        let git = ParallelGit {
            worktree_output: raw,
            barrier: Some(Arc::new(Barrier::new(SNAPSHOT_WORKER_LIMIT))),
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            failure_index: None,
        };

        let snapshot = SnapshotCollector::new(&git, &NoPrLookup)
            .collect(directory.path(), "main", false)
            .unwrap();

        assert_eq!(git.maximum.load(Ordering::SeqCst), SNAPSHOT_WORKER_LIMIT);
        assert_eq!(
            snapshot
                .worktrees
                .iter()
                .filter_map(|worktree| worktree.branch.as_deref())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|index| format!("feature/{index}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            snapshot
                .warnings
                .iter()
                .filter_map(|warning| warning.branch.as_deref())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|index| format!("feature/{index}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parallel_snapshot_propagates_the_indexed_worker_error() {
        let directory = tempfile::tempdir().unwrap();
        let mut raw = Vec::new();
        for index in 0..3 {
            raw.extend_from_slice(
                format!(
                    "worktree {}/wt-{index}\0HEAD {index:040}\0branch refs/heads/feature/{index}\0\0",
                    directory.path().display()
                )
                .as_bytes(),
            );
        }
        let git = ParallelGit {
            worktree_output: raw,
            barrier: None,
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            failure_index: Some(1),
        };

        let error = SnapshotCollector::new(&git, &NoPrLookup)
            .collect(directory.path(), "main", false)
            .unwrap_err();
        let SnapshotError::Git(failure) = error else {
            panic!("worker error was not propagated as a Git failure");
        };
        assert!(failure.cwd.ends_with("wt-1"));
    }

    #[test]
    fn vanished_branch_is_not_recreated_from_a_stale_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".vde/worktree/state")).unwrap();
        let raw = format!(
            "worktree {}/wt\0HEAD {:040}\0branch refs/heads/feature/gone\0\0",
            directory.path().display(),
            1
        )
        .into_bytes();
        let git = ParallelGit {
            worktree_output: raw,
            barrier: None,
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            failure_index: None,
        };

        SnapshotCollector::new(&git, &NoPrLookup)
            .collect(directory.path(), "main", false)
            .unwrap();

        assert!(!lifecycle_file_path(directory.path(), "feature/gone").exists());
    }

    #[test]
    fn parses_attached_detached_locked_prunable_bare_and_newline_paths() {
        let raw = b"worktree /repo/main\0HEAD abc\0branch refs/heads/main\0\0worktree /repo/line\npath\0HEAD def\0detached\0locked reason\0prunable stale\0\0worktree /repo/bare\0bare\0\0";
        let parsed = parse_worktree_porcelain(raw).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].branch.as_deref(), Some("main"));
        assert_eq!(parsed[1].path, PathBuf::from("/repo/line\npath"));
        assert!(parsed[1].locked);
        assert!(parsed[1].prunable);
        assert!(parsed[2].bare);
        assert!(parsed[2].head.is_empty());
    }

    #[test]
    fn rejects_all_structurally_malformed_porcelain_forms() {
        for raw in [
            b"HEAD abc\0\0".as_slice(),
            b"worktree \0HEAD abc\0\0",
            b"worktree /repo\0\0",
            b"worktree /repo\0HEAD abc\0branch main\0\0",
            b"worktree /repo\0HEAD abc\0HEAD def\0\0",
            b"worktree /repo\0HEAD abc\0branch refs/heads/main\0detached\0\0",
            b"worktree /repo\0HEAD abc\0unknown value\0\0",
            b"worktree /repo\0HEAD abc",
        ] {
            assert!(parse_worktree_porcelain(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn merge_truth_table_exhaustively_covers_all_twenty_seven_combinations() {
        let values = [None, Some(false), Some(true)];
        let mut combinations = 0;
        for ancestry in values {
            for pr in values {
                for lifecycle in values {
                    combinations += 1;
                    let expected = if pr == Some(true) || lifecycle == Some(true) {
                        Some(true)
                    } else if ancestry == Some(false)
                        || pr == Some(false)
                        || lifecycle == Some(false)
                    {
                        Some(false)
                    } else {
                        None
                    };
                    assert_eq!(
                        resolve_merged_overall(ancestry, pr, lifecycle),
                        expected,
                        "ancestry={ancestry:?}, pr={pr:?}, lifecycle={lifecycle:?}"
                    );
                }
            }
        }
        assert_eq!(combinations, 27);
    }

    #[test]
    fn real_git_fixture_collects_dirty_and_preserves_invalid_lifecycle() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        run(
            &repo,
            directory.path(),
            ["init", "-b", "main", repo.to_str().unwrap()],
        );
        run(&repo, &repo, ["config", "user.email", "test@example.com"]);
        run(&repo, &repo, ["config", "user.name", "Test"]);
        fs::write(repo.join("README.md"), "initial\n").unwrap();
        run(&repo, &repo, ["add", "README.md"]);
        run(&repo, &repo, ["commit", "-m", "initial"]);
        let feature_path = directory.path().join("feature\npath");
        run(
            &repo,
            &repo,
            [
                "worktree",
                "add",
                "-b",
                "feature/a",
                feature_path.to_str().unwrap(),
            ],
        );
        fs::write(feature_path.join("dirty.txt"), "dirty\n").unwrap();
        let lifecycle_path = lifecycle_file_path(&repo, "feature/a");
        fs::create_dir_all(lifecycle_path.parent().unwrap()).unwrap();
        fs::write(&lifecycle_path, b"{not-json}\n").unwrap();
        let before = fs::read(&lifecycle_path).unwrap();

        let git = GitCli::new(StdProcessRunner);
        let snapshot = SnapshotCollector::new(&git, &NoPrLookup)
            .collect(&repo, "main", false)
            .unwrap();
        let feature = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some("feature/a"))
            .unwrap();
        assert!(feature.dirty);
        assert_eq!(feature.pr.status, Some(PrStatus::Unknown));
        assert_eq!(feature.upstream, unknown_upstream());
        assert!(snapshot.warnings.iter().any(|warning| {
            warning.code == SnapshotWarningCode::InvalidLifecycle
                && warning.branch.as_deref() == Some("feature/a")
        }));
        assert_eq!(fs::read(lifecycle_path).unwrap(), before);
    }

    #[test]
    fn real_git_native_and_application_locks_are_merged_conservatively() {
        use crate::state::worktree_lock::{WorktreeLockUpdate, upsert_worktree_lock};

        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        run(
            &repo,
            directory.path(),
            ["init", "-b", "main", repo.to_str().unwrap()],
        );
        run(&repo, &repo, ["config", "user.email", "test@example.com"]);
        run(&repo, &repo, ["config", "user.name", "Test"]);
        fs::write(repo.join("README.md"), "initial\n").unwrap();
        run(&repo, &repo, ["add", "README.md"]);
        run(&repo, &repo, ["commit", "-m", "initial"]);
        let native_path = directory.path().join("native");
        let app_path = directory.path().join("application");
        run(
            &repo,
            &repo,
            [
                "worktree",
                "add",
                "-b",
                "feature/native-lock",
                native_path.to_str().unwrap(),
            ],
        );
        run(
            &repo,
            &repo,
            [
                "worktree",
                "add",
                "-b",
                "feature/app-lock",
                app_path.to_str().unwrap(),
            ],
        );
        run(
            &repo,
            &repo,
            [
                "worktree",
                "lock",
                "--reason",
                "native",
                native_path.to_str().unwrap(),
            ],
        );
        upsert_worktree_lock(
            &repo,
            "feature/app-lock",
            WorktreeLockUpdate {
                reason: "application",
                owner: "tester",
                host: "localhost",
                pid: 1,
            },
        )
        .unwrap();

        let git = GitCli::new(StdProcessRunner);
        let snapshot = SnapshotCollector::new(&git, &NoPrLookup)
            .collect(&repo, "main", false)
            .unwrap();
        let native = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some("feature/native-lock"))
            .unwrap();
        assert!(native.locked.value);
        assert_eq!(native.locked.reason.as_deref(), Some("git worktree lock"));
        let application = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some("feature/app-lock"))
            .unwrap();
        assert!(application.locked.value);
        assert_eq!(application.locked.reason.as_deref(), Some("application"));
        assert_eq!(application.locked.owner.as_deref(), Some("tester"));
    }

    #[test]
    fn collection_persists_divergence_when_state_directory_exists() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        run(
            &repo,
            directory.path(),
            ["init", "-b", "main", repo.to_str().unwrap()],
        );
        run(&repo, &repo, ["config", "user.email", "test@example.com"]);
        run(&repo, &repo, ["config", "user.name", "Test"]);
        fs::write(repo.join("README.md"), "initial\n").unwrap();
        run(&repo, &repo, ["add", "README.md"]);
        run(&repo, &repo, ["commit", "-m", "initial"]);
        let feature_path = directory.path().join("feature");
        run(
            &repo,
            &repo,
            [
                "worktree",
                "add",
                "-b",
                "feature/read-only",
                feature_path.to_str().unwrap(),
            ],
        );
        fs::write(feature_path.join("feature.txt"), "feature\n").unwrap();
        run(&repo, &feature_path, ["add", "feature.txt"]);
        run(&repo, &feature_path, ["commit", "-m", "feature"]);

        let state_root = repo.join(".vde/worktree/state");
        fs::create_dir_all(&state_root).unwrap();
        let lifecycle_path = lifecycle_file_path(&repo, "feature/read-only");
        assert!(!lifecycle_path.exists());
        let record_head = String::from_utf8(
            Command::new("git")
                .current_dir(&repo)
                .args(["rev-parse", "feature/read-only"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let record_head = record_head.trim().to_owned();

        let git = GitCli::new(StdProcessRunner);
        SnapshotCollector::new(&git, &NoPrLookup)
            .collect(&repo, "main", false)
            .unwrap();

        assert!(state_root.is_dir());
        let JsonRecordState::Valid(record) =
            read_worktree_lifecycle(&repo, "feature/read-only").state
        else {
            panic!("divergence observation was not persisted");
        };
        assert!(record.ever_diverged);
        assert_eq!(
            record.last_diverged_head.as_deref(),
            Some(record_head.as_str())
        );
        assert!(lifecycle_path.exists());
        assert!(state_root.join("lifecycle-observation.lock").is_file());
    }

    #[test]
    fn resolves_remote_head_then_local_main_or_master() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        run(
            &repo,
            directory.path(),
            ["init", "-b", "main", repo.to_str().unwrap()],
        );
        run(&repo, &repo, ["config", "user.email", "test@example.com"]);
        run(&repo, &repo, ["config", "user.name", "Test"]);
        fs::write(repo.join("README.md"), "initial\n").unwrap();
        run(&repo, &repo, ["add", "README.md"]);
        run(&repo, &repo, ["commit", "-m", "initial"]);
        let git = GitCli::new(StdProcessRunner);
        assert_eq!(
            resolve_base_branch(&git, &repo, Some("develop"), "origin").unwrap(),
            "develop"
        );
        assert_eq!(
            resolve_base_branch(&git, &repo, None, "origin").unwrap(),
            "main"
        );
    }

    fn run<const N: usize>(repo: &Path, cwd: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git failed for {}: {}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
