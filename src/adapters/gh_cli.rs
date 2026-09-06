use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::domain::worktree::{PrState, PrStatus, PrUnavailableReason};
use crate::ports::process::{
    OutputPolicy, ProcessCommand, ProcessError, ProcessRunner, StdinPolicy,
};
use crate::ports::snapshot::PrStateLookup;

const DEFAULT_GH_TIMEOUT: Duration = Duration::from_secs(30);
pub const GH_BRANCH_BATCH_SIZE: usize = 25;

#[derive(Debug)]
pub struct GhCli<R> {
    runner: R,
    timeout: Duration,
}

impl<R> PrStateLookup for GhCli<R>
where
    R: ProcessRunner + Sync,
{
    fn resolve_pr_states(
        &self,
        repo_root: &Path,
        base_branch: Option<&str>,
        branches: &[Option<String>],
        enabled: bool,
    ) -> HashMap<String, PrState> {
        self.resolve_pr_states(repo_root, base_branch, branches, enabled)
    }
}

impl<R> GhCli<R>
where
    R: ProcessRunner,
{
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            timeout: DEFAULT_GH_TIMEOUT,
        }
    }

    pub fn with_timeout(runner: R, timeout: Duration) -> Self {
        Self { runner, timeout }
    }

    /// Resolves all non-base branches. A disabled or unavailable `gh`, a failed batch, and a
    /// malformed batch response are represented as `unknown`, never as a false "no PR" result.
    pub fn resolve_pr_states(
        &self,
        repo_root: &Path,
        base_branch: Option<&str>,
        branches: &[Option<String>],
        enabled: bool,
    ) -> HashMap<String, PrState> {
        let Some(base_branch) = base_branch else {
            return HashMap::new();
        };
        let targets = target_branches(branches, base_branch);
        if targets.is_empty() {
            return HashMap::new();
        }
        if !enabled {
            return unknown_states(&targets, PrUnavailableReason::Disabled, None, None);
        }

        let mut merged = HashMap::with_capacity(targets.len());
        for batch in targets.chunks(GH_BRANCH_BATCH_SIZE) {
            merged.extend(self.resolve_batch(repo_root, base_branch, batch));
        }
        merged
    }

    fn resolve_batch(
        &self,
        repo_root: &Path,
        base_branch: &str,
        branches: &[String],
    ) -> HashMap<String, PrState> {
        let search = branches
            .iter()
            .map(|branch| format!("head:{branch}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let args = vec![
            OsString::from("pr"),
            OsString::from("list"),
            OsString::from("--state"),
            OsString::from("all"),
            OsString::from("--base"),
            OsString::from(base_branch),
            OsString::from("--search"),
            OsString::from(search),
            OsString::from("--limit"),
            OsString::from("1000"),
            OsString::from("--json"),
            OsString::from("headRefName,headRefOid,state,mergedAt,updatedAt,url"),
        ];
        let output = match self.runner.run(&gh_command(repo_root, args, self.timeout)) {
            Ok(output) => output,
            Err(error) => {
                let reason = if matches!(&error, ProcessError::Spawn(error) if error.kind() == std::io::ErrorKind::NotFound)
                {
                    PrUnavailableReason::DependencyMissing
                } else {
                    PrUnavailableReason::CommandFailed
                };
                return unknown_states(branches, reason, Some(&error.to_string()), None);
            }
        };
        if output.timed_out || output.exit_code != Some(0) {
            let reason = if output.timed_out {
                PrUnavailableReason::TimedOut
            } else if output.exit_code == Some(4) {
                PrUnavailableReason::AuthenticationRequired
            } else {
                PrUnavailableReason::CommandFailed
            };
            return unknown_states(
                branches,
                reason,
                Some(String::from_utf8_lossy(&output.stderr).trim()),
                output.exit_code,
            );
        }
        parse_pr_states(&output.stdout, branches).unwrap_or_else(|| {
            unknown_states(
                branches,
                PrUnavailableReason::InvalidResponse,
                Some("gh returned an invalid PR response"),
                output.exit_code,
            )
        })
    }
}

fn gh_command(repo_root: &Path, args: Vec<OsString>, timeout: Duration) -> ProcessCommand {
    let mut command = ProcessCommand::new("gh");
    command.args = args;
    command.cwd = Some(repo_root.to_path_buf());
    command.stdin = StdinPolicy::Null;
    command.stdout = OutputPolicy::Capture;
    command.stderr = OutputPolicy::Capture;
    command.timeout = Some(timeout);
    command
}

fn target_branches(branches: &[Option<String>], base_branch: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    branches
        .iter()
        .filter_map(Option::as_deref)
        .filter(|branch| !branch.is_empty() && *branch != base_branch)
        .filter(|branch| seen.insert((*branch).to_owned()))
        .map(str::to_owned)
        .collect()
}

fn unknown_states(
    branches: &[String],
    reason: PrUnavailableReason,
    message: Option<&str>,
    exit_code: Option<i32>,
) -> HashMap<String, PrState> {
    branches
        .iter()
        .map(|branch| {
            (
                branch.clone(),
                PrState::unavailable(reason, message.map(str::to_owned), exit_code),
            )
        })
        .collect()
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PrSummary {
    head_ref_oid: Option<String>,
    head_ref_name: Option<String>,
    state: Option<String>,
    merged_at: Option<String>,
    updated_at: Option<String>,
    url: Option<String>,
}

fn parse_pr_states(raw: &[u8], branches: &[String]) -> Option<HashMap<String, PrState>> {
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let records = value.as_array()?;
    let targets = branches.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut latest = HashMap::<String, (i128, usize, PrState)>::new();

    for (index, value) in records.iter().enumerate() {
        let object = value.as_object()?;
        if ["headRefName", "state", "mergedAt", "updatedAt", "url"]
            .iter()
            .any(|field| !object.contains_key(*field))
        {
            return None;
        }
        let record = serde_json::from_value::<PrSummary>(value.clone()).ok()?;
        let branch = record
            .head_ref_name
            .as_deref()
            .filter(|branch| !branch.is_empty())?
            .to_owned();
        let updated_at = timestamp_key(record.updated_at.as_deref())?;
        let state_name = record.state.as_deref()?.to_ascii_uppercase();
        if !matches!(state_name.as_str(), "OPEN" | "CLOSED" | "MERGED") {
            return None;
        }
        if record.url.as_deref().is_none_or(str::is_empty) {
            return None;
        }
        if let Some(merged_at) = record.merged_at.as_deref()
            && !merged_at.is_empty()
            && timestamp_key(Some(merged_at)).is_none()
        {
            return None;
        }
        if !targets.contains(branch.as_str()) {
            continue;
        }
        let state = PrState {
            status: Some(pr_status(&record)),
            url: record.url.filter(|url| !url.is_empty()),
            head_oid: record.head_ref_oid.filter(|oid| !oid.is_empty()),
            diagnostic: None,
        };
        let replace = latest
            .get(&branch)
            .is_none_or(|(current, current_index, _)| {
                updated_at > *current || (updated_at == *current && index > *current_index)
            });
        if replace {
            latest.insert(branch, (updated_at, index, state));
        }
    }

    Some(
        branches
            .iter()
            .map(|branch| {
                let state = latest
                    .remove(branch)
                    .map_or_else(PrState::none, |(_, _, state)| state);
                (branch.clone(), state)
            })
            .collect(),
    )
}

fn pr_status(record: &PrSummary) -> PrStatus {
    if record
        .merged_at
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        return PrStatus::Merged;
    }
    match record
        .state
        .as_deref()
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        Some("MERGED") => PrStatus::Merged,
        Some("OPEN") => PrStatus::Open,
        Some("CLOSED") => PrStatus::ClosedUnmerged,
        _ => PrStatus::Unknown,
    }
}

fn timestamp_key(value: Option<&str>) -> Option<i128> {
    OffsetDateTime::parse(value?, &Rfc3339)
        .ok()
        .map(OffsetDateTime::unix_timestamp_nanos)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;
    use crate::ports::process::{ProcessError, ProcessOutput};

    #[derive(Debug, Default)]
    struct FakeRunner {
        commands: Mutex<Vec<ProcessCommand>>,
        outputs: Mutex<Vec<Result<ProcessOutput, ProcessError>>>,
    }

    impl FakeRunner {
        fn with_outputs(outputs: Vec<Result<ProcessOutput, ProcessError>>) -> Self {
            Self {
                commands: Mutex::new(Vec::new()),
                outputs: Mutex::new(outputs.into_iter().rev().collect()),
            }
        }
    }

    impl ProcessRunner for FakeRunner {
        fn run(&self, command: &ProcessCommand) -> Result<ProcessOutput, ProcessError> {
            self.commands.lock().unwrap().push(command.clone());
            self.outputs.lock().unwrap().pop().unwrap()
        }
    }

    fn success(stdout: &str) -> ProcessOutput {
        ProcessOutput {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: Some(0),
            timed_out: false,
        }
    }

    #[test]
    fn disabled_lookup_returns_unknown_without_starting_gh() {
        let client = GhCli::new(FakeRunner::default());
        let states = client.resolve_pr_states(
            Path::new("/repo"),
            Some("main"),
            &[Some("main".into()), Some("feature/a".into()), None],
            false,
        );
        assert_eq!(
            states["feature/a"].diagnostic.as_ref().unwrap().reason,
            PrUnavailableReason::Disabled
        );
        assert!(client.runner.commands.lock().unwrap().is_empty());
    }

    #[test]
    fn maps_all_pr_states_and_prefers_the_latest_record() {
        let output = serde_json::json!([
            {"headRefName":"feature/open","state":"MERGED","mergedAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","url":"old"},
            {"headRefName":"feature/open","state":"OPEN","mergedAt":null,"updatedAt":"2026-01-02T00:00:00Z","url":"new"},
            {"headRefName":"feature/merged","state":"CLOSED","mergedAt":"2026-01-02T00:00:00Z","updatedAt":"2026-01-02T00:00:00Z","url":"merged"},
            {"headRefName":"feature/closed","state":"CLOSED","mergedAt":null,"updatedAt":"2026-01-02T00:00:00Z","url":"closed"}
        ]);
        let client = GhCli::new(FakeRunner::with_outputs(vec![Ok(success(
            &output.to_string(),
        ))]));
        let states = client.resolve_pr_states(
            Path::new("/repo"),
            Some("main"),
            &[
                Some("feature/open".into()),
                Some("feature/merged".into()),
                Some("feature/closed".into()),
                Some("feature/none".into()),
            ],
            true,
        );
        assert_eq!(states["feature/open"].status, Some(PrStatus::Open));
        assert_eq!(states["feature/open"].url.as_deref(), Some("new"));
        assert_eq!(states["feature/merged"].status, Some(PrStatus::Merged));
        assert_eq!(
            states["feature/closed"].status,
            Some(PrStatus::ClosedUnmerged)
        );
        assert_eq!(states["feature/none"].status, Some(PrStatus::None));
    }

    #[test]
    fn batches_queries_and_merges_success_and_failure_as_unknown() {
        let first = serde_json::json!([
            {"headRefName":"feature/0","state":"OPEN","mergedAt":null,"updatedAt":"2026-01-01T00:00:00Z","url":"https://example.test/0"}
        ]);
        let failed = Ok(ProcessOutput {
            stdout: Vec::new(),
            stderr: b"not logged in".to_vec(),
            exit_code: Some(1),
            timed_out: false,
        });
        let runner = FakeRunner::with_outputs(vec![Ok(success(&first.to_string())), failed]);
        let client = GhCli::new(runner);
        let branches = (0..(GH_BRANCH_BATCH_SIZE + 3))
            .map(|index| Some(format!("feature/{index}")))
            .collect::<Vec<_>>();
        let states = client.resolve_pr_states(Path::new("/repo"), Some("main"), &branches, true);

        assert_eq!(client.runner.commands.lock().unwrap().len(), 2);
        assert_eq!(states["feature/0"].status, Some(PrStatus::Open));
        assert_eq!(states["feature/1"].status, Some(PrStatus::None));
        assert_eq!(states["feature/25"].status, Some(PrStatus::Unknown));
        assert_eq!(states.len(), GH_BRANCH_BATCH_SIZE + 3);
    }

    #[test]
    fn invalid_json_and_unavailable_process_are_unknown() {
        let client = GhCli::new(FakeRunner::with_outputs(vec![Ok(success("not-json"))]));
        let states = client.resolve_pr_states(
            Path::new("/repo"),
            Some("main"),
            &[Some("feature/a".into())],
            true,
        );
        assert_eq!(states["feature/a"].status, Some(PrStatus::Unknown));
        assert_eq!(
            states["feature/a"].diagnostic.as_ref().unwrap().reason,
            PrUnavailableReason::InvalidResponse
        );

        let unavailable = GhCli::new(FakeRunner::with_outputs(vec![Err(ProcessError::Spawn(
            std::io::Error::new(std::io::ErrorKind::NotFound, "gh not found"),
        ))]));
        let states = unavailable.resolve_pr_states(
            Path::new("/repo"),
            Some("main"),
            &[Some("feature/a".into())],
            true,
        );
        assert_eq!(states["feature/a"].status, Some(PrStatus::Unknown));
        assert_eq!(
            states["feature/a"].diagnostic.as_ref().unwrap().reason,
            PrUnavailableReason::DependencyMissing
        );
    }

    #[test]
    fn unknown_states_preserve_authentication_timeout_and_failure_evidence() {
        for (exit_code, timed_out, reason) in [
            (Some(4), false, PrUnavailableReason::AuthenticationRequired),
            (None, true, PrUnavailableReason::TimedOut),
            (Some(1), false, PrUnavailableReason::CommandFailed),
        ] {
            let client = GhCli::new(FakeRunner::with_outputs(vec![Ok(ProcessOutput {
                stdout: Vec::new(),
                stderr: b"probe detail".to_vec(),
                exit_code,
                timed_out,
            })]));
            let states = client.resolve_pr_states(
                Path::new("/repo"),
                Some("main"),
                &[Some("topic".to_owned())],
                true,
            );
            let diagnostic = states["topic"].diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.reason, reason);
            assert_eq!(diagnostic.exit_code, exit_code);
            assert_eq!(diagnostic.message.as_deref(), Some("probe detail"));
            assert_eq!(states["topic"].status, Some(PrStatus::Unknown));
        }
    }

    #[test]
    fn a_type_invalid_record_makes_the_whole_batch_unknown() {
        let client = GhCli::new(FakeRunner::with_outputs(vec![Ok(success(
            r#"[
                {"headRefName":"feature/a","state":"OPEN","mergedAt":null,"updatedAt":"2026-01-01T00:00:00Z","url":"https://example.test/a"},
                {"headRefName":"feature/b","state":42,"mergedAt":null,"updatedAt":"2026-01-01T00:00:00Z","url":"https://example.test/b"}
            ]"#,
        ))]));
        let states = client.resolve_pr_states(
            Path::new("/repo"),
            Some("main"),
            &[Some("feature/a".into()), Some("feature/b".into())],
            true,
        );

        assert_eq!(states["feature/a"].status, Some(PrStatus::Unknown));
        assert_eq!(states["feature/b"].status, Some(PrStatus::Unknown));
    }

    #[test]
    fn missing_required_fields_or_invalid_timestamps_make_the_batch_unknown() {
        for response in [
            r#"[{"state":"OPEN","mergedAt":null,"updatedAt":"2026-01-01T00:00:00Z","url":"https://example.test/a"}]"#,
            r#"[{"headRefName":"feature/a","state":"OPEN","mergedAt":null,"updatedAt":"not-a-time","url":"https://example.test/a"}]"#,
            r#"[{"headRefName":"feature/a","state":"OPEN","mergedAt":null,"updatedAt":"2026-01-01T00:00:00Z"}]"#,
        ] {
            let client = GhCli::new(FakeRunner::with_outputs(vec![Ok(success(response))]));
            let states = client.resolve_pr_states(
                Path::new("/repo"),
                Some("main"),
                &[Some("feature/a".into())],
                true,
            );
            assert_eq!(states["feature/a"].status, Some(PrStatus::Unknown));
        }
    }

    #[test]
    fn command_contract_uses_bounded_search_and_expected_process_policy() {
        let client = GhCli::with_timeout(
            FakeRunner::with_outputs(vec![Ok(success("[]"))]),
            Duration::from_secs(7),
        );
        let states = client.resolve_pr_states(
            Path::new("/repo"),
            Some("main"),
            &[Some("main".into()), Some("feature/a".into())],
            true,
        );
        assert_eq!(states["feature/a"].status, Some(PrStatus::None));
        let commands = client.runner.commands.lock().unwrap();
        let command = &commands[0];
        assert_eq!(command.program, OsString::from("gh"));
        assert_eq!(command.cwd, Some(PathBuf::from("/repo")));
        assert_eq!(command.timeout, Some(Duration::from_secs(7)));
        assert_eq!(command.stdin, StdinPolicy::Null);
        assert_eq!(command.stdout, OutputPolicy::Capture);
        assert_eq!(command.stderr, OutputPolicy::Capture);
        assert_eq!(command.args[7], OsString::from("head:feature/a"));
    }
}
