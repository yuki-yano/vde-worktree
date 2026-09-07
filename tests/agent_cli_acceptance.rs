#![cfg(unix)]

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let repo = fs::canonicalize(repo).unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let fixture = Self { temp, repo, bin };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.git(&["config", "user.name", "Test"]);
        fixture.git(&["config", "user.email", "test@example.com"]);
        fixture.git(&["commit", "--allow-empty", "-q", "-m", "initial"]);
        fixture.script(
            "gh",
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$GH_LOG\"\nprintf '[]\\n'\n",
        );
        fixture.script("fzf", "#!/bin/sh\nprintf 'test-fzf\\n'\n");
        fixture.script("tmux", "#!/bin/sh\nprintf 'test-tmux\\n'\n");
        fixture
    }
    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn script(&self, name: &str, text: &str) {
        let path = self.bin.join(name);
        fs::write(&path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vw"));
        let path = std::env::join_paths(std::iter::once(self.bin.clone()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        command
            .current_dir(&self.repo)
            .env("HOME", self.temp.path().join("home"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", self.temp.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GH_LOG", self.temp.path().join("gh-log"))
            .env("PATH", path)
            .env_remove("WT_WORKTREE_PATH");
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        let value = support::parse_cli_json(&output.stdout);
        assert!(
            output.status.success(),
            "{args:?}: {value}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        value
    }
    fn config(&self, text: &str) -> PathBuf {
        let path = self.repo.join(".vde/worktree/config.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

#[derive(Debug, PartialEq, Eq)]
enum FileEvidence {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

fn files(root: &Path) -> BTreeMap<PathBuf, (FileEvidence, std::time::SystemTime)> {
    let mut result = BTreeMap::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return result,
        Err(error) => panic!("cannot inspect {}: {error}", root.display()),
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let metadata = fs::symlink_metadata(&path)
            .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", path.display()));
        let modified = metadata.modified().unwrap();
        let evidence = if metadata.file_type().is_symlink() {
            FileEvidence::Symlink(fs::read_link(&path).unwrap())
        } else if metadata.is_dir() {
            result.extend(files(&path));
            FileEvidence::Directory
        } else {
            FileEvidence::File(fs::read(&path).unwrap())
        };
        result.insert(path, (evidence, modified));
    }
    result
}

#[test]
fn filesystem_evidence_records_dangling_symlinks() {
    let directory = tempfile::tempdir().unwrap();
    let link = directory.path().join("dangling");
    std::os::unix::fs::symlink("missing-target", &link).unwrap();
    let cycle = directory.path().join("cycle");
    std::os::unix::fs::symlink(".", &cycle).unwrap();
    let evidence = files(directory.path());
    assert_eq!(evidence.len(), 2);
    assert_eq!(
        evidence[&link].0,
        FileEvidence::Symlink("missing-target".into())
    );
    assert_eq!(evidence[&cycle].0, FileEvidence::Symlink(".".into()));
}

#[test]
fn context_tracks_effective_cli_values_and_each_setting_source() {
    let fixture = Fixture::new();
    let global = fixture.temp.path().join("config/vde/worktree/config.yml");
    fs::create_dir_all(global.parent().unwrap()).unwrap();
    fs::write(&global, "hooks:\n  enabled: false\ngithub:\n  enabled: false\nlocks:\n  timeoutMs: 21\nselector:\n  cd:\n    fzf:\n      extraArgs: [--exact]\n").unwrap();
    let local = fixture.config("locks:\n  timeoutMs: 30\n");
    let near = fixture.repo.join("nested");
    let near_config = near.join(".vde/worktree/config.yml");
    fs::create_dir_all(near_config.parent().unwrap()).unwrap();
    fs::write(&near_config, "selector:\n  cd:\n    prompt: Local\n").unwrap();
    let value = fixture.ok(&[
        "-C",
        near.to_str().unwrap(),
        "context",
        "--json",
        "--hooks",
        "--gh",
        "--hook-timeout-ms",
        "88",
        "--fzf-arg=--cycle",
    ]);
    let report = &value["data"]["config"];
    assert_eq!(report["loadedFiles"], json!([global, local, near_config]));
    assert_eq!(report["effective"]["hooks"]["enabled"], true);
    assert_eq!(report["effective"]["github"]["enabled"], true);
    assert_eq!(report["effective"]["hooks"]["timeoutMs"], 88);
    assert_eq!(report["effective"]["locks"]["timeoutMs"], 30);
    assert_eq!(
        report["sources"]["hooks.enabled"],
        json!([{"kind":"commandLine","argument":"--hooks"}])
    );
    assert_eq!(
        report["sources"]["locks.timeoutMs"],
        json!([{"kind":"file","path":local}])
    );
    assert_eq!(
        report["sources"]["selector.cd.prompt"],
        json!([{"kind":"file","path":near_config}])
    );
    assert_eq!(
        report["sources"]["git.baseRemote"],
        json!([{"kind":"default"}])
    );
    assert_eq!(
        report["effective"]["selector"]["cd"]["fzf"]["extraArgs"],
        json!(["--exact", "--cycle"])
    );
    assert_eq!(
        report["sources"]["selector.cd.fzf.extraArgs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(value["data"]["initialized"], false);
    assert!(!fixture.temp.path().join("gh-log").exists());
}

#[test]
fn explicit_hooks_and_github_override_disabled_config_and_verbose_keeps_json_valid() {
    let fixture = Fixture::new();
    fixture.config("hooks:\n  enabled: false\ngithub:\n  enabled: false\n");
    fixture.ok(&["init", "--json"]);
    let hook = fixture.repo.join(".vde/worktree/hooks/post-new");
    fs::write(
        &hook,
        "#!/bin/sh\nprintf '%s\\n' \"$WT_BRANCH\" >> \"$WT_REPO_ROOT/hook-log\"\n",
    )
    .unwrap();
    fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.ok(&["new", "inactive", "--json"]);
    assert!(!fixture.repo.join("hook-log").exists());
    fixture.ok(&["new", "active", "--json", "--hooks"]);
    assert_eq!(
        fs::read_to_string(fixture.repo.join("hook-log")).unwrap(),
        "active\n"
    );
    let disabled = fixture.ok(&["status", "active", "--json"]);
    assert_eq!(
        disabled["data"]["worktree"]["pr"]["diagnostic"]["reason"],
        "disabled"
    );
    assert!(!fixture.temp.path().join("gh-log").exists());
    let active = fixture.run(&[
        "status",
        "active",
        "--json",
        "--gh",
        "--verbose",
        "--verbose",
    ]);
    assert!(active.status.success());
    let value = support::parse_cli_json(&active.stdout);
    assert_eq!(value["data"]["worktree"]["pr"]["status"], "none");
    assert!(
        fs::read_to_string(fixture.temp.path().join("gh-log"))
            .unwrap()
            .contains("pr list")
    );
    let stderr = String::from_utf8_lossy(&active.stderr);
    assert!(stderr.contains("[verbose] command=status"));
    assert!(stderr.contains("github=true"));
    assert!(stderr.contains("[verbose] config="));
}

#[test]
fn doctor_reports_invalid_config_uninitialized_state_and_pending_journals_without_writes() {
    let fixture = Fixture::new();
    fixture.config("locks:\n  staleLockTTLSeconds: 1800\n");
    let journal = fixture
        .repo
        .join(".vde/worktree/state/metadata-transactions/invalid/journal.json");
    fs::create_dir_all(journal.parent().unwrap()).unwrap();
    fs::write(&journal, "invalid json").unwrap();
    let orphan = fixture
        .repo
        .join(".vde/worktree/state/metadata-transactions/orphan");
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join("staged-lock.json"), "preserve").unwrap();
    let before = files(&fixture.repo.join(".vde"));
    let output = fixture.run(&["doctor", "--json"]);
    assert_eq!(output.status.code(), Some(4));
    let value = support::parse_cli_json(&output.stdout);
    assert_eq!(value["data"]["healthy"], false);
    let checks = value["data"]["checks"].as_array().unwrap();
    for name in ["configuration", "initialization", "pendingRecoveries"] {
        assert_eq!(
            checks.iter().find(|check| check["name"] == name).unwrap()["status"],
            "error"
        );
    }
    assert_eq!(
        value["data"]["pendingRecoveries"][0]["journalState"],
        "invalid"
    );
    assert_eq!(
        value["data"]["pendingRecoveries"][1]["journalState"],
        "missing"
    );
    assert_eq!(files(&fixture.repo.join(".vde")), before);
    assert!(orphan.is_dir());
    assert!(!fixture.temp.path().join("gh-log").exists());
    let context = fixture.run(&["context", "--json"]);
    assert_eq!(
        support::parse_cli_json(&context.stdout)["error"]["code"],
        "INVALID_CONFIG"
    );
}

#[test]
fn doctor_works_outside_git_and_optional_dependency_warnings_do_not_hide_repository_health() {
    let fixture = Fixture::new();
    let outside = fixture
        .command()
        .current_dir(fixture.temp.path())
        .args(["doctor", "--json", "--no-gh"])
        .output()
        .unwrap();
    let value = support::parse_cli_json(&outside.stdout);
    assert_eq!(value["data"]["healthy"], false);
    assert!(value["data"]["repository"].is_null());
    fixture.ok(&["init", "--json"]);
    fixture.script("fzf", "#!/bin/sh\nexit 1\n");
    let value = fixture.ok(&["doctor", "--json", "--no-gh"]);
    assert_eq!(value["data"]["healthy"], true);
    assert!(
        value["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "fzf" && check["status"] == "warning")
    );
    assert!(!fixture.temp.path().join("gh-log").exists());
}

#[test]
fn diagnostics_report_unsupported_paths_without_panicking() {
    let mut fixture = Fixture::new();
    let invalid = fixture.temp.path().join("repo-line\nbreak");
    fs::rename(&fixture.repo, &invalid).unwrap();
    fixture.repo = invalid;
    let context = fixture.run(&["context", "--json"]);
    assert_eq!(
        support::parse_cli_json(&context.stdout)["error"]["code"],
        "UNSUPPORTED_REPOSITORY_LAYOUT"
    );
    let doctor = fixture.run(&["doctor", "--json", "--no-gh"]);
    let value = support::parse_cli_json(&doctor.stdout);
    assert_eq!(value["data"]["healthy"], false);
    assert!(
        value["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "repositoryPaths" && check["status"] == "error")
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn check_and_dry_run_never_lock_recover_stash_run_hooks_or_write_git_state() {
    let fixture = Fixture::new();
    let before = files(&fixture.repo);
    let init = fixture.ok(&["init", "--dry-run", "--json"]);
    assert_eq!(init["data"]["allowed"], true);
    assert_eq!(before, files(&fixture.repo));
    fixture.ok(&["init", "--json"]);
    let new = fixture.ok(&["new", "topic", "--json"]);
    let topic = PathBuf::from(new["data"]["path"].as_str().unwrap());
    fs::write(topic.join("dirty"), "untracked changes").unwrap();
    fixture.ok(&["lock", "topic", "--owner", "test-session", "--json"]);
    for action in [
        "new", "switch", "get", "adopt", "mv", "del", "gone", "extract", "absorb", "unabsorb",
        "use", "init",
    ] {
        let path = fixture
            .repo
            .join(".vde/worktree/hooks")
            .join(format!("pre-{action}"));
        fs::write(
            &path,
            "#!/bin/sh\ntouch \"$WT_REPO_ROOT/hook-must-not-run\"\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let before = files(&fixture.repo);
    for args in [
        vec!["init"],
        vec!["new", "planned"],
        vec!["switch", "planned"],
        vec!["get", "missing/topic"],
        vec!["adopt", "--apply"],
        vec!["mv", "renamed"],
        vec!["del", "topic"],
        vec!["gone", "--apply"],
        vec!["extract", "--current", "--stash"],
        vec!["absorb", "topic", "--allow-agent", "--allow-unsafe"],
        vec!["unabsorb", "main", "--allow-agent", "--allow-unsafe"],
        vec!["use", "topic", "--allow-agent", "--allow-unsafe"],
        vec!["lock", "topic", "--owner", "test-session"],
        vec!["unlock", "topic", "--owner", "test-session"],
    ] {
        let mut full = vec!["check", "--json", "--no-gh", "--"];
        full.extend(args.clone());
        let output = fixture.run(&full);
        let value = support::parse_cli_json(&output.stdout);
        assert_eq!(value["command"], "check", "{args:?}: {value}");
        assert_eq!(value["data"]["command"], args[0], "{value}");
        assert_eq!(value["data"]["dryRun"], true, "{value}");
        assert_eq!(value["data"]["requiresRevalidation"], true);
        assert_eq!(value["data"]["allowed"], output.status.success());
        assert_eq!(
            before,
            files(&fixture.repo),
            "inspection changed repository: {args:?}"
        );
    }
    let output = fixture.run(&["del", "topic", "--dry-run", "--json", "--no-gh"]);
    let value = support::parse_cli_json(&output.stdout);
    assert_eq!(output.status.code(), Some(4));
    let rejected = &value["data"]["evidence"]["targets"][0]["rejections"];
    for code in [
        "DIRTY_WORKTREE",
        "LOCKED_WORKTREE",
        "UNMERGED_WORKTREE",
        "UNPUSHED_WORKTREE",
    ] {
        assert!(
            rejected
                .as_array()
                .unwrap()
                .iter()
                .any(|error| error["code"] == code),
            "{value}"
        );
    }
    assert_eq!(before, files(&fixture.repo));

    // Default batch previews also bypass the mutation lock and recovery path.
    fixture.ok(&["gone", "--json", "--no-gh"]);
    fixture.ok(&["adopt", "--json", "--no-gh"]);
    assert_eq!(before, files(&fixture.repo));
    let journals = fixture
        .repo
        .join(".vde/worktree/state/metadata-transactions");
    fs::create_dir_all(journals.join("orphan")).unwrap();
    let before = files(&fixture.repo);
    for args in [
        vec!["new", "planned", "--dry-run"],
        vec!["gone"],
        vec!["adopt"],
    ] {
        let mut full = vec!["--json", "--no-gh"];
        full.extend(args);
        let output = fixture.run(&full);
        let value = support::parse_cli_json(&output.stdout);
        assert!(!output.status.success(), "{value}");
        assert_eq!(value["data"]["allowed"], false);
        assert_eq!(
            value["data"]["pendingRecoveries"].as_array().unwrap().len(),
            1
        );
        assert_eq!(before, files(&fixture.repo));
    }
}

#[test]
fn deletion_requires_matching_pr_head_and_rechecks_after_the_pre_hook() {
    for change_in_hook in [false, true] {
        let fixture = Fixture::new();
        fixture.ok(&["init", "--json"]);
        let new = fixture.ok(&["new", "topic", "--json"]);
        let topic = new["data"]["path"].as_str().unwrap();
        fs::write(Path::new(topic).join("feature"), "feature contents").unwrap();
        fixture.git(&["-C", topic, "add", "feature"]);
        fixture.git(&["-C", topic, "commit", "-q", "-m", "feature"]);
        fixture.git(&["merge", "--squash", "topic"]);
        fixture.git(&["commit", "-q", "-m", "squashed feature"]);
        let oid = Command::new("git")
            .args(["rev-parse", "topic"])
            .current_dir(&fixture.repo)
            .output()
            .unwrap();
        let oid = String::from_utf8(oid.stdout).unwrap().trim().to_owned();
        for (head, reason) in [
            (Value::Null, "head_unavailable"),
            (
                json!("0000000000000000000000000000000000000000"),
                "head_mismatch",
            ),
        ] {
            let response = json!([{"headRefName": "topic", "headRefOid": head, "state": "MERGED", "mergedAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z", "url": "https://example.test/pr/1"}]);
            fixture.script("gh", &format!("#!/bin/sh\nprintf '%s\\n' '{response}'\n"));
            let status = fixture.ok(&["status", "topic", "--json"]);
            assert_eq!(
                status["data"]["worktree"]["pr"]["diagnostic"]["reason"],
                reason
            );
            assert_eq!(status["data"]["worktree"]["merged"]["byPR"], Value::Null);
            assert_eq!(
                fixture.ok(&["gone", "--json"])["data"]["candidates"],
                json!([])
            );
        }
        let response = json!([{"headRefName": "topic", "headRefOid": oid, "state": "MERGED", "mergedAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z", "url": "https://example.test/pr/1"}]);
        fixture.script("gh", &format!("#!/bin/sh\nprintf '%s\\n' '{response}'\n"));
        let status = fixture.ok(&["status", "topic", "--json"]);
        assert_eq!(status["data"]["worktree"]["merged"]["byAncestry"], false);
        assert_eq!(status["data"]["worktree"]["merged"]["byPR"], true);
        assert_eq!(status["data"]["worktree"]["pr"]["headOid"], oid);
        assert_eq!(
            fixture.ok(&["gone", "--json"])["data"]["candidates"],
            json!(["topic"])
        );
        let inspect = fixture.ok(&["check", "--json", "--", "gone", "--apply"]);
        assert_eq!(
            inspect["data"]["plannedResult"]["candidates"],
            json!(["topic"])
        );
        let manual =
            support::parse_cli_json(&fixture.run(&["del", "topic", "--dry-run", "--json"]).stdout);
        assert_eq!(manual["data"]["allowed"], false); // del also checks upstream state.
        assert_eq!(manual["error"]["code"], "UNPUSHED_WORKTREE");
        if change_in_hook {
            let hook = fixture.repo.join(".vde/worktree/hooks/pre-gone");
            fs::write(&hook, "#!/bin/sh\ngit -C \"$WT_REPO_ROOT/.worktree/topic\" commit --allow-empty -q -m 'new work after inspection'\n").unwrap();
            fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let output = fixture.run(&["gone", "--apply", "--json"]);
        let value = support::parse_cli_json(&output.stdout);
        if change_in_hook {
            assert!(!output.status.success(), "{value}");
            assert_eq!(value["data"]["failed"][0]["code"], "UNMERGED_WORKTREE");
            assert!(Path::new(topic).exists());
        } else {
            assert!(output.status.success(), "{value}");
            assert_eq!(value["data"]["deleted"], json!(["topic"]));
            assert!(!Path::new(topic).exists());
        }
    }
}

#[test]
fn recovery_results_survive_a_later_command_or_journal_failure() {
    let fixture = Fixture::new();
    fixture.ok(&["init", "--json"]);
    let journals = fixture
        .repo
        .join(".vde/worktree/state/metadata-transactions");
    fs::create_dir_all(journals.join("orphan")).unwrap();
    let output = fixture.run(&["new", "main", "--json"]);
    let value = support::parse_cli_json(&output.stdout);
    assert!(!output.status.success());
    assert_eq!(value["error"]["code"], "BRANCH_ALREADY_ATTACHED");
    assert_eq!(value["warnings"][0]["code"], "METADATA_RECOVERY_COMPLETED");
    assert_eq!(
        value["warnings"][0]["details"]["recoveryOutcome"]["transactionId"],
        "orphan"
    );
    assert!(!journals.join("orphan").exists());
    fs::create_dir_all(journals.join("a-orphan")).unwrap();
    fs::create_dir_all(journals.join("b-invalid")).unwrap();
    fs::write(journals.join("b-invalid/journal.json"), "invalid").unwrap();
    let output = fixture.run(&["new", "planned", "--json"]);
    let value = support::parse_cli_json(&output.stdout);
    assert!(!output.status.success(), "{value}");
    assert_eq!(value["error"]["execution"]["phase"], "recover");
    assert_eq!(value["error"]["execution"]["state"], "recoveryRequired");
    assert_eq!(
        value["error"]["details"]["completedRecoveries"][0]["transactionId"],
        "a-orphan"
    );
    assert!(
        value["error"]["details"]["path"]
            .as_str()
            .unwrap()
            .ends_with("b-invalid/journal.json")
    );
    assert!(!journals.join("a-orphan").exists());
    assert!(journals.join("b-invalid/journal.json").exists());
    assert!(!fixture.repo.join(".worktree/planned").exists());
}

#[test]
fn exec_exposes_stdin_limits_and_signal_termination() {
    let fixture = Fixture::new();
    let input = fixture.temp.path().join("stdin");
    fs::write(&input, "inherited input\n").unwrap();
    for (option, expected) in [(None, ""), (Some("inherit"), "inherited input\n")] {
        let mut command = fixture.command();
        command.args(["exec", "main", "--json", "--timeout-ms", "1000"]);
        if let Some(option) = option {
            command.args(["--stdin", option]);
        }
        let output = command
            .args(["--", "cat"])
            .stdin(fs::File::open(&input).unwrap())
            .output()
            .unwrap();
        let value = support::parse_cli_json(&output.stdout);
        assert!(output.status.success(), "{value}");
        assert_eq!(value["data"]["childStdout"], expected);
        assert_eq!(value["data"]["childSignal"], Value::Null);
        assert_eq!(value["data"]["timedOut"], false);
    }
    let human = fixture
        .command()
        .args(["exec", "main", "--stdin", "inherit", "--", "cat"])
        .stdin(fs::File::open(&input).unwrap())
        .output()
        .unwrap();
    assert!(human.status.success());
    assert_eq!(human.stdout, b"inherited input\n");
    for prefix in [
        vec!["exec", "main", "--json"],
        vec!["--json", "--worktree", ".", "exec"],
    ] {
        let args = [
            prefix,
            vec![
                "--max-output-bytes",
                "17",
                "--",
                "sh",
                "-c",
                "head -c 8192 /dev/zero; head -c 8192 /dev/zero >&2",
            ],
        ]
        .concat();
        let limited = fixture.ok(&args);
        assert_eq!(limited["data"]["childStdout"].as_str().unwrap().len(), 17);
        assert_eq!(limited["data"]["childStderr"].as_str().unwrap().len(), 17);
        assert_eq!(limited["data"]["stdoutTruncated"], true);
        assert_eq!(limited["data"]["stderrTruncated"], true);
        assert_eq!(limited["data"]["childExitCode"], 0);
    }
    let output = fixture.run(&[
        "exec",
        "main",
        "--json",
        "--",
        "sh",
        "-c",
        "printf before; printf detail >&2; kill -TERM $$",
    ]);
    let signal = support::parse_cli_json(&output.stdout);
    assert_eq!(output.status.code(), Some(21));
    assert_eq!(signal["data"]["childExitCode"], Value::Null);
    assert_eq!(signal["data"]["childSignal"], 15);
    assert_eq!(signal["data"]["timedOut"], false);
    assert_eq!(signal["data"]["childStdout"], "before");
    assert_eq!(signal["error"]["execution"]["phase"], "process");
    for args in [
        vec!["exec", "main", "--timeout-ms", "0", "--json", "--", "true"],
        vec![
            "exec",
            "main",
            "--max-output-bytes",
            "0",
            "--json",
            "--",
            "true",
        ],
        vec!["exec", "main", "--max-output-bytes", "17", "--", "true"],
    ] {
        assert_eq!(fixture.run(&args).status.code(), Some(3));
    }
}

#[test]
fn exec_timeout_preserves_output_and_terminates_descendants_even_after_leader_exit() {
    let fixture = Fixture::new();
    for child in [
        "sleep 30 & printf '%s:%s\\n' \"$$\" \"$!\"; printf prefix >&2; wait",
        "sleep 30 & printf '%s\\n' \"$!\"; printf prefix >&2",
    ] {
        let started = std::time::Instant::now();
        let output = fixture.run(&[
            "exec",
            "main",
            "--json",
            "--timeout-ms",
            "100",
            "--",
            "sh",
            "-c",
            child,
        ]);
        let value = support::parse_cli_json(&output.stdout);
        assert_eq!(output.status.code(), Some(21), "{value}");
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
        assert_eq!(value["data"]["timedOut"], true);
        assert_eq!(value["data"]["childStderr"], "prefix");
        assert_eq!(value["error"]["details"]["timedOut"], true);
        for pid in value["data"]["childStdout"]
            .as_str()
            .unwrap()
            .trim()
            .split(':')
        {
            let _: u32 = pid.parse().expect("child PID");
            let mut stopped = false;
            for _ in 0..20 {
                let output = Command::new("ps")
                    .args(["-o", "stat=", "-p", pid])
                    .output()
                    .unwrap();
                let state = String::from_utf8_lossy(&output.stdout);
                if state.trim().is_empty() || state.trim_start().starts_with('Z') {
                    stopped = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            assert!(stopped, "descendant {pid} is still running");
        }
    }
}
