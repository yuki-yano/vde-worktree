#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;
use vde_worktree::state::repo_lock::acquire_repo_lock;

fn create_repository() -> (TempDir, PathBuf) {
    let fixture = tempfile::tempdir().expect("create fixture directory");
    let repository = fixture.path().join("repository");
    fs::create_dir(&repository).expect("create repository directory");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["init", "--quiet", "-b", "main"])
        .status()
        .expect("initialize Git repository");
    assert!(status.success());
    (fixture, repository)
}

fn git_ok(repository: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repository)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_vw(repository: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vw"))
        .current_dir(repository)
        .args(args)
        .env("HOME", repository.join("isolated-home"))
        .env("XDG_CONFIG_HOME", repository.join("isolated-config"))
        .env("GIT_CONFIG_GLOBAL", repository.join("isolated-gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run vw")
}

fn parse_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is one JSON object")
}

#[test]
#[allow(clippy::too_many_lines)]
fn submodule_nested_cwd_keeps_init_list_and_switch_scoped_to_the_submodule() {
    let (fixture, parent) = create_repository();
    git_ok(&parent, &["config", "user.name", "Parent Repository"]);
    git_ok(&parent, &["config", "user.email", "parent@example.com"]);
    fs::write(parent.join("README.md"), "parent\n").expect("write parent fixture");
    git_ok(&parent, &["add", "README.md"]);
    git_ok(&parent, &["commit", "--quiet", "-m", "initial parent"]);

    let child_source = fixture.path().join("child-source");
    fs::create_dir(&child_source).expect("create child source directory");
    git_ok(&child_source, &["init", "--quiet", "-b", "main"]);
    git_ok(&child_source, &["config", "user.name", "Child Repository"]);
    git_ok(
        &child_source,
        &["config", "user.email", "child@example.com"],
    );
    fs::write(child_source.join("README.md"), "child\n").expect("write child fixture");
    git_ok(&child_source, &["add", "README.md"]);
    git_ok(&child_source, &["commit", "--quiet", "-m", "initial child"]);

    git_ok(
        &parent,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            child_source.to_str().expect("child source path is UTF-8"),
            "vendor/child",
        ],
    );
    git_ok(&parent, &["add", ".gitmodules", "vendor/child"]);
    git_ok(&parent, &["commit", "--quiet", "-m", "add child submodule"]);

    let parent = fs::canonicalize(parent).expect("canonical parent repository");
    let submodule =
        fs::canonicalize(parent.join("vendor/child")).expect("canonical submodule repository");
    let git_pointer = fs::read(submodule.join(".git")).expect("read submodule git pointer");
    assert!(submodule.join(".git").is_file());

    let nested = submodule.join("nested/directory");
    fs::create_dir_all(&nested).expect("create nested submodule directory");

    let first_init = run_vw(&nested, &["init", "--json"]);
    assert!(
        first_init.status.success(),
        "first init failed: stdout={} stderr={}",
        String::from_utf8_lossy(&first_init.stdout),
        String::from_utf8_lossy(&first_init.stderr)
    );
    assert_eq!(parse_json(&first_init)["data"]["alreadyInitialized"], false);

    let metadata_root = submodule.join(".vde/worktree");
    let managed_root = submodule.join(".worktree");
    assert!(metadata_root.is_dir());
    assert!(managed_root.is_dir());
    assert!(!parent.join(".vde").exists());
    assert!(!parent.join(".worktree").exists());
    assert!(submodule.join(".git").is_file());
    assert_eq!(fs::read(submodule.join(".git")).unwrap(), git_pointer);

    let common_git_dir = parent.join(".git/modules/vendor/child");
    let exclude_path = common_git_dir.join("info/exclude");
    let first_exclude = fs::read_to_string(&exclude_path).expect("read submodule exclude file");
    assert!(first_exclude.contains("# vde-worktree (managed)\n.worktree/\n.vde/worktree/\n"));

    let second_init = run_vw(&nested, &["init", "--json"]);
    assert!(
        second_init.status.success(),
        "second init stderr: {}",
        String::from_utf8_lossy(&second_init.stderr)
    );
    assert_eq!(parse_json(&second_init)["data"]["alreadyInitialized"], true);
    let second_exclude = fs::read_to_string(&exclude_path).expect("reread submodule exclude file");
    assert_eq!(second_exclude, first_exclude);
    assert_eq!(
        second_exclude.matches("# vde-worktree (managed)").count(),
        1
    );

    let initial_list = run_vw(&nested, &["list", "--json", "--no-gh"]);
    assert!(
        initial_list.status.success(),
        "initial list stderr: {}",
        String::from_utf8_lossy(&initial_list.stderr)
    );
    let initial_list = parse_json(&initial_list);
    assert_eq!(
        initial_list["data"]["managedWorktreeRoot"],
        managed_root.to_string_lossy().as_ref()
    );
    let initial_worktrees = initial_list["data"]["worktrees"]
        .as_array()
        .expect("initial worktrees array");
    assert_eq!(initial_worktrees.len(), 1);
    assert_eq!(initial_worktrees[0]["branch"], "main");
    assert_eq!(
        initial_worktrees[0]["path"],
        submodule.to_string_lossy().as_ref()
    );

    let target = managed_root.join("feature/submodule");
    let created = run_vw(
        &nested,
        &["switch", "feature/submodule", "--json", "--no-gh"],
    );
    assert!(
        created.status.success(),
        "created switch stderr: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let created = parse_json(&created);
    assert_eq!(created["data"]["branch"], "feature/submodule");
    assert_eq!(created["data"]["path"], target.to_string_lossy().as_ref());
    assert_eq!(created["data"]["disposition"], "created");

    let existing = run_vw(
        &nested,
        &["switch", "feature/submodule", "--json", "--no-gh"],
    );
    assert!(
        existing.status.success(),
        "existing switch stderr: {}",
        String::from_utf8_lossy(&existing.stderr)
    );
    let existing = parse_json(&existing);
    assert_eq!(existing["data"]["path"], target.to_string_lossy().as_ref());
    assert_eq!(existing["data"]["disposition"], "existing");

    let final_list = run_vw(&nested, &["list", "--json", "--no-gh"]);
    assert!(
        final_list.status.success(),
        "final list stderr: {}",
        String::from_utf8_lossy(&final_list.stderr)
    );
    let final_list = parse_json(&final_list);
    let final_worktrees = final_list["data"]["worktrees"]
        .as_array()
        .expect("final worktrees array");
    assert_eq!(final_worktrees.len(), 2);
    assert!(final_worktrees.iter().any(|worktree| {
        worktree["branch"] == "feature/submodule"
            && worktree["path"] == target.to_string_lossy().as_ref()
    }));

    let deleted = run_vw(
        &nested,
        &[
            "del",
            "feature/submodule",
            "--force",
            "--allow-unsafe",
            "--json",
            "--no-gh",
        ],
    );
    assert!(
        deleted.status.success(),
        "delete stderr: {}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    let deleted = parse_json(&deleted);
    assert_eq!(deleted["data"]["branch"], "feature/submodule");
    assert_eq!(deleted["data"]["path"], target.to_string_lossy().as_ref());
    assert!(!target.exists());

    let after_delete = run_vw(&nested, &["list", "--json", "--no-gh"]);
    assert!(after_delete.status.success());
    let after_delete = parse_json(&after_delete);
    let remaining = after_delete["data"]["worktrees"]
        .as_array()
        .expect("remaining worktrees array");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["path"], submodule.to_string_lossy().as_ref());
    assert!(submodule.join(".git").is_file());
    assert_eq!(fs::read(submodule.join(".git")).unwrap(), git_pointer);
    assert!(!parent.join(".vde").exists());
}

fn relative_entries(root: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, current: &Path, entries: &mut Vec<PathBuf>) {
        let mut children = fs::read_dir(current)
            .expect("read fixture directory")
            .map(|entry| entry.expect("read fixture entry").path())
            .collect::<Vec<_>>();
        children.sort();
        for path in children {
            entries.push(
                path.strip_prefix(root)
                    .expect("entry is under root")
                    .to_path_buf(),
            );
            if path.is_dir() {
                visit(root, &path, entries);
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    entries
}

#[test]
fn completion_runs_outside_a_repository_for_both_supported_shells() {
    let directory = tempfile::tempdir().expect("create non-repository directory");

    for shell in ["zsh", "fish"] {
        let output = run_vw(directory.path(), &["completion", shell]);

        assert!(
            output.status.success(),
            "{shell} completion failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.is_empty(), "{shell} completion is empty");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn all_thirteen_uninitialized_write_commands_have_no_filesystem_side_effect() {
    let cases: [&[&str]; 13] = [
        &["new", "feature/a", "--json"],
        &["switch", "main", "--json"],
        &["mv", "renamed", "--json"],
        &["del", "--json"],
        &["gone", "--json"],
        &["adopt", "--json"],
        &["get", "origin/main", "--json"],
        &["extract", "--current", "--json"],
        &["absorb", "main", "--json"],
        &["unabsorb", "main", "--json"],
        &["use", "main", "--json"],
        &["lock", "main", "--json"],
        &["unlock", "main", "--json"],
    ];

    for args in cases {
        let (_fixture, repository) = create_repository();
        let before = relative_entries(&repository);
        let output = run_vw(&repository, args);
        let value = parse_json(&output);

        assert_eq!(output.status.code(), Some(4), "{args:?}");
        assert_eq!(value["error"]["code"], "NOT_INITIALIZED", "{args:?}");
        assert_eq!(relative_entries(&repository), before, "{args:?}");
    }
}

#[test]
fn competing_process_observes_cli_lock_timeout_contract() {
    let (_fixture, repository) = create_repository();
    fs::create_dir_all(repository.join(".vde/worktree")).expect("mark repository initialized");
    let common_dir = repository.join(".git");
    let _owner = acquire_repo_lock(&common_dir, Duration::from_secs(1), "test-owner")
        .expect("acquire owner lock");

    let output = run_vw(
        &repository,
        &["new", "feature/a", "--json", "--lock-timeout-ms", "40"],
    );
    let value = parse_json(&output);

    assert_eq!(output.status.code(), Some(6));
    assert_eq!(value["error"]["code"], "REPO_LOCK_TIMEOUT");
    assert_eq!(value["error"]["details"]["timeoutMs"], 40);
}

#[test]
fn repository_config_controls_lock_timeout() {
    let (_fixture, repository) = create_repository();
    fs::create_dir_all(repository.join(".vde/worktree")).expect("mark repository initialized");
    fs::write(
        repository.join(".vde/worktree/config.yml"),
        "locks:\n  timeoutMs: 35\n",
    )
    .expect("write repository config");
    let _owner = acquire_repo_lock(
        &repository.join(".git"),
        Duration::from_secs(1),
        "test-owner",
    )
    .expect("acquire owner lock");

    let output = run_vw(&repository, &["new", "feature/a", "--json"]);
    let value = parse_json(&output);

    assert_eq!(output.status.code(), Some(6));
    assert_eq!(value["error"]["code"], "REPO_LOCK_TIMEOUT");
    assert_eq!(value["error"]["details"]["timeoutMs"], 35);
}

#[test]
fn twenty_processes_repeat_repo_lock_contention_one_hundred_times_without_overlap() {
    const PROCESS_COUNT: usize = 20;
    let common_dir = tempfile::tempdir().expect("create common directory");
    let barrier = common_dir.path().join("start");
    let executable = std::env::current_exe().expect("resolve test executable");
    let mut children: Vec<Child> = (0..PROCESS_COUNT)
        .map(|_| {
            Command::new(&executable)
                .args([
                    "--exact",
                    "repo_lock_stress_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("VW_REPO_LOCK_STRESS_DIR", common_dir.path())
                .env("VW_REPO_LOCK_STRESS_BARRIER", &barrier)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn lock contender")
        })
        .collect();

    fs::write(&barrier, b"start\n").expect("release lock contenders");

    for (index, child) in children.drain(..).enumerate() {
        let output = child.wait_with_output().expect("wait for lock contender");
        assert!(
            output.status.success(),
            "contender {index} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn repo_lock_stress_child() {
    const REPETITIONS: usize = 100;
    let Some(common_dir) = std::env::var_os("VW_REPO_LOCK_STRESS_DIR").map(PathBuf::from) else {
        return;
    };
    let barrier = PathBuf::from(
        std::env::var_os("VW_REPO_LOCK_STRESS_BARRIER").expect("stress barrier path"),
    );
    while !barrier.exists() {
        thread::sleep(Duration::from_millis(1));
    }

    let critical_marker = common_dir.join("critical-section");
    for iteration in 0..REPETITIONS {
        let lock = acquire_repo_lock(&common_dir, Duration::from_secs(10), "stress-child")
            .expect("acquire stress lock");
        let marker = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&critical_marker)
            .unwrap_or_else(|error| {
                panic!("overlapping critical section at iteration {iteration}: {error}")
            });
        thread::sleep(Duration::from_micros(200));
        drop(marker);
        fs::remove_file(&critical_marker).expect("remove critical-section marker");
        drop(lock);
    }
}

#[test]
fn new_copies_gitignored_files_matched_by_worktree_include() {
    let (_fixture, repository) = create_repository();
    git_ok(&repository, &["config", "user.email", "test@example.com"]);
    git_ok(&repository, &["config", "user.name", "Test"]);
    fs::write(repository.join("README.md"), "fixture\n").expect("write README");
    fs::write(repository.join(".gitignore"), ".env.local\n").expect("write gitignore");
    fs::write(repository.join(".worktreeinclude"), ".env.local\n").expect("write worktree include");
    git_ok(
        &repository,
        &["add", "README.md", ".gitignore", ".worktreeinclude"],
    );
    git_ok(&repository, &["commit", "--quiet", "-m", "initial"]);
    fs::write(repository.join(".env.local"), "local secret\n").expect("write ignored file");

    let initialized = run_vw(&repository, &["init", "--json"]);
    assert!(
        initialized.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let created = run_vw(
        &repository,
        &["new", "feature/include", "--json", "--no-gh"],
    );
    assert!(
        created.status.success(),
        "new failed: stdout={} stderr={}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    let created = parse_json(&created);
    let worktree = PathBuf::from(created["data"]["path"].as_str().expect("worktree path"));
    assert_eq!(
        fs::read_to_string(worktree.join(".env.local")).unwrap(),
        "local secret\n"
    );
}

#[test]
fn cli_hook_timeout_stops_the_command_before_dispatch() {
    let (_fixture, repository) = create_repository();
    for args in [
        &["config", "user.email", "test@example.com"][..],
        &["config", "user.name", "Test"][..],
        &["commit", "--quiet", "--allow-empty", "-m", "initial"][..],
    ] {
        assert!(
            Command::new("git")
                .current_dir(&repository)
                .args(args)
                .status()
                .expect("prepare initialized repository")
                .success()
        );
    }
    let initialized = run_vw(&repository, &["init", "--no-hooks", "--allow-unsafe"]);
    assert!(initialized.status.success());
    let hooks = repository.join(".vde/worktree/hooks");
    fs::create_dir_all(&hooks).expect("create hook directory");
    let hook = hooks.join("pre-new");
    fs::write(&hook, "#!/bin/sh\nsleep 30\n").expect("write timeout hook");
    let mut permissions = fs::metadata(&hook)
        .expect("read hook metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook, permissions).expect("make hook executable");

    let output = run_vw(
        &repository,
        &["new", "feature/a", "--json", "--hook-timeout-ms", "40"],
    );
    let value = parse_json(&output);

    assert_eq!(output.status.code(), Some(10));
    assert_eq!(value["error"]["code"], "HOOK_TIMEOUT");
}

#[test]
fn repository_config_disables_mutation_hooks_without_unsafe_consent() {
    let (_fixture, repository) = create_repository();
    for args in [
        &["config", "user.email", "test@example.com"][..],
        &["config", "user.name", "Test"][..],
        &["commit", "--quiet", "--allow-empty", "-m", "initial"][..],
    ] {
        assert!(
            Command::new("git")
                .current_dir(&repository)
                .args(args)
                .status()
                .expect("prepare initialized repository")
                .success()
        );
    }
    let initialized = run_vw(&repository, &["init"]);
    assert!(initialized.status.success());
    fs::write(
        repository.join(".vde/worktree/config.yml"),
        "hooks:\n  enabled: false\n",
    )
    .expect("write repository config");
    let hook = repository.join(".vde/worktree/hooks/pre-new");
    fs::write(&hook, "#!/bin/sh\nexit 73\n").expect("write rejecting hook");
    let mut permissions = fs::metadata(&hook)
        .expect("read hook metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook, permissions).expect("make hook executable");

    let output = run_vw(&repository, &["new", "feature/config-disabled", "--json"]);
    let value = parse_json(&output);

    assert!(output.status.success());
    assert_eq!(value["status"], "ok");
    assert_eq!(value["data"]["branch"], "feature/config-disabled");
}
