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

fn files(root: &Path) -> BTreeMap<PathBuf, (Option<Vec<u8>>, std::time::SystemTime)> {
    let mut result = BTreeMap::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries {
            let path = entry.unwrap().path();
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            if path.is_dir() {
                result.insert(path.clone(), (None, modified));
                result.extend(files(&path));
            } else {
                result.insert(path.clone(), (Some(fs::read(path).unwrap()), modified));
            }
        }
    }
    result
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
