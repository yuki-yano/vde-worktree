#![cfg(unix)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;
use vde_worktree::state::lifecycle::lifecycle_file_path;
use vde_worktree::state::metadata_transaction::{
    MetadataRenameRequest, MetadataTransactionFaultInjector, MetadataTransactionStep,
    commit_metadata_rename_with_injector, mark_metadata_rename_branch_renamed,
    mark_metadata_rename_worktree_moved, prepare_metadata_rename, stage_metadata_rename,
};
use vde_worktree::state::repo_lock::acquire_repo_lock;
use vde_worktree::state::worktree_lock::worktree_lock_file_path;

struct Fixture {
    root: TempDir,
    repo: PathBuf,
    home: PathBuf,
    xdg: PathBuf,
    git_config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let repo = root.path().join("repo");
        fs::create_dir(&repo).expect("repository directory");
        git_ok(&repo, &["init", "--quiet", "-b", "main"]);
        git_ok(&repo, &["config", "user.name", "Phase Three"]);
        git_ok(&repo, &["config", "user.email", "phase3@example.com"]);
        fs::write(repo.join("README.md"), "initial\n").expect("initial file");
        git_ok(&repo, &["add", "README.md"]);
        git_ok(&repo, &["commit", "--quiet", "-m", "initial"]);
        let repo = fs::canonicalize(repo).expect("canonical repository");
        let fixture = Self {
            home: root.path().join("home"),
            xdg: root.path().join("xdg"),
            git_config: root.path().join("global-gitconfig"),
            root,
            repo,
        };
        let initialized = fixture.vw(&fixture.repo, &["init", "--json"]);
        assert_success(&initialized);
        assert_eq!(json(&initialized)["data"]["alreadyInitialized"], false);
        fixture
    }

    fn vw(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_vw"))
            .current_dir(cwd)
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.xdg)
            .env("GIT_CONFIG_GLOBAL", &self.git_config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("USER", "phase3-user")
            .env_remove("NO_COLOR")
            .output()
            .expect("run vw")
    }

    fn vw_with_git_wrapper(&self, cwd: &Path, args: &[&str], wrapper_dir: &Path) -> Output {
        let original_path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(wrapper_dir.to_path_buf()).chain(std::env::split_paths(&original_path)),
        )
        .expect("wrapper PATH");
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .expect("resolve git executable");
        assert_success(&real_git);
        let real_git = String::from_utf8(real_git.stdout).expect("git path utf8");

        Command::new(env!("CARGO_BIN_EXE_vw"))
            .current_dir(cwd)
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.xdg)
            .env("GIT_CONFIG_GLOBAL", &self.git_config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("USER", "phase3-user")
            .env("PATH", path)
            .env("REAL_GIT", real_git.trim())
            .env_remove("NO_COLOR")
            .output()
            .expect("run vw with git wrapper")
    }

    fn stabilize_snapshot(&self) {
        assert_success(&self.vw(&self.repo, &["list", "--no-gh"]));
    }

    fn managed(&self, branch: &str) -> PathBuf {
        self.repo.join(".worktree").join(branch)
    }
}

fn git_ok(cwd: &Path, args: &[&str]) {
    let output = git_output(cwd, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(cwd: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git")
}

fn git_text(cwd: &Path, args: &[&str]) -> String {
    let output = git_output(cwd, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf8 git output")
}

fn json(output: &Output) -> Value {
    support::parse_cli_json(&output.stdout)
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_error(output: &Output, code: &str) -> Value {
    let value = json(output);
    assert!(
        !output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(value["status"], "error");
    assert_eq!(value["error"]["code"], code);
    value
}

#[derive(Debug, PartialEq, Eq)]
struct ObservableState {
    refs: String,
    worktrees: String,
    managed: Vec<(PathBuf, Option<Vec<u8>>)>,
    locks: Vec<(PathBuf, Option<Vec<u8>>)>,
    lifecycle: Vec<(PathBuf, Option<Vec<u8>>)>,
}

fn observable_state(repo: &Path) -> ObservableState {
    ObservableState {
        refs: git_text(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
            ],
        ),
        worktrees: git_text(repo, &["worktree", "list", "--porcelain"]),
        managed: file_tree(&repo.join(".worktree")),
        locks: file_tree(&repo.join(".vde/worktree/locks")),
        lifecycle: file_tree(&repo.join(".vde/worktree/state/branches")),
    }
}

fn file_tree(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    fn visit(root: &Path, current: &Path, entries: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
        let mut children = match fs::read_dir(current) {
            Ok(children) => children
                .map(|entry| entry.expect("directory entry").path())
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("read {}: {error}", current.display()),
        };
        children.sort();
        for path in children {
            let relative = path
                .strip_prefix(root)
                .expect("path below root")
                .to_path_buf();
            if path.is_dir() {
                entries.push((relative, None));
                visit(root, &path, entries);
            } else {
                entries.push((relative, Some(fs::read(&path).expect("read file"))));
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    entries
}

fn write_hook(repo: &Path, name: &str, script: &str) {
    let path = repo.join(".vde/worktree/hooks").join(name);
    fs::write(&path, script).expect("write hook");
    let mut permissions = fs::metadata(&path).expect("hook metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("executable hook");
}

fn write_executable(path: &Path, script: &str) {
    fs::write(path, script).expect("write executable");
    let mut permissions = fs::metadata(path)
        .expect("executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("set executable bit");
}

struct CrashAt {
    step: MetadataTransactionStep,
}

impl MetadataTransactionFaultInjector for CrashAt {
    fn after_step(&mut self, step: MetadataTransactionStep) -> Result<(), String> {
        if step == self.step {
            Err("phase 3 acceptance crash".to_owned())
        } else {
            Ok(())
        }
    }
}

#[test]
fn a009_new_rejects_existing_attached_and_target_collisions_without_mutation() {
    let fixture = Fixture::new();

    git_ok(&fixture.repo, &["branch", "feature/existing", "main"]);
    fixture.stabilize_snapshot();
    let before = observable_state(&fixture.repo);
    let output = fixture.vw(&fixture.repo, &["new", "feature/existing", "--json"]);
    assert_error(&output, "BRANCH_ALREADY_EXISTS");
    assert_eq!(observable_state(&fixture.repo), before);

    let attached_path = fixture.root.path().join("attached");
    git_ok(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/attached",
            attached_path.to_str().expect("utf8 attached path"),
            "main",
        ],
    );
    fixture.stabilize_snapshot();
    let before = observable_state(&fixture.repo);
    let output = fixture.vw(&fixture.repo, &["new", "feature/attached", "--json"]);
    assert_error(&output, "BRANCH_ALREADY_ATTACHED");
    assert_eq!(observable_state(&fixture.repo), before);

    let blocked = fixture.managed("feature/blocked");
    fs::create_dir_all(&blocked).expect("blocked target");
    fs::write(blocked.join("keep"), "do not overwrite\n").expect("block target");
    fixture.stabilize_snapshot();
    let before = observable_state(&fixture.repo);
    let output = fixture.vw(&fixture.repo, &["new", "feature/blocked", "--json"]);
    assert_error(&output, "TARGET_PATH_NOT_EMPTY");
    assert_eq!(observable_state(&fixture.repo), before);
}

#[test]
fn a010_switch_reports_created_then_existing_and_hooks_receive_phase_specific_cwd() {
    let fixture = Fixture::new();

    git_ok(&fixture.repo, &["branch", "feature/local", "main"]);
    let local = fixture.vw(&fixture.repo, &["switch", "feature/local", "--json"]);
    assert_success(&local);
    assert_eq!(json(&local)["data"]["disposition"], "created");
    assert_eq!(
        json(&local)["data"]["path"],
        fixture.managed("feature/local").to_string_lossy().as_ref()
    );

    git_ok(&fixture.repo, &["branch", "feature/switch-blocked", "main"]);
    let blocked = fixture.managed("feature/switch-blocked");
    fs::create_dir_all(&blocked).expect("blocked switch target");
    fs::write(blocked.join("keep"), "keep\n").expect("block switch target");
    fixture.stabilize_snapshot();
    let before = observable_state(&fixture.repo);
    let collision = fixture.vw(
        &fixture.repo,
        &["switch", "feature/switch-blocked", "--json"],
    );
    assert_error(&collision, "TARGET_PATH_NOT_EMPTY");
    assert_eq!(observable_state(&fixture.repo), before);

    let target = fixture.managed("feature/hooked");
    let trace = fixture.root.path().join("hook-trace");
    write_hook(
        &fixture.repo,
        "pre-switch",
        &format!(
            "#!/bin/sh\nset -eu\nprintf 'pre|%s|%s|%s|%s\\n' \"$PWD\" \"$WT_ACTION\" \"$WT_BRANCH\" \"$WT_WORKTREE_PATH\" >> '{}'\n",
            trace.display()
        ),
    );
    write_hook(
        &fixture.repo,
        "post-switch",
        &format!(
            "#!/bin/sh\nset -eu\nprintf 'post|%s|%s|%s|%s\\n' \"$PWD\" \"$WT_ACTION\" \"$WT_BRANCH\" \"$WT_WORKTREE_PATH\" >> '{}'\n",
            trace.display()
        ),
    );

    let created = fixture.vw(&fixture.repo, &["switch", "feature/hooked", "--json"]);
    assert_success(&created);
    let created = json(&created);
    assert_eq!(created["data"]["branch"], "feature/hooked");
    assert_eq!(created["data"]["path"], target.to_string_lossy().as_ref());
    assert_eq!(created["data"]["disposition"], "created");

    let existing = fixture.vw(&fixture.repo, &["switch", "feature/hooked", "--json"]);
    assert_success(&existing);
    let existing = json(&existing);
    assert_eq!(existing["data"]["path"], target.to_string_lossy().as_ref());
    assert_eq!(existing["data"]["disposition"], "existing");

    let lines = fs::read_to_string(trace).expect("hook trace");
    let expected_pre = format!(
        "pre|{}|switch|feature/hooked|{}",
        fixture.repo.display(),
        target.display()
    );
    let expected_post = format!(
        "post|{}|switch|feature/hooked|{}",
        target.display(),
        target.display()
    );
    assert_eq!(lines.matches(&expected_pre).count(), 2);
    assert_eq!(lines.matches(&expected_post).count(), 2);
}

#[test]
fn a011_mv_rejects_primary_detached_and_conflicts_then_moves_git_and_metadata() {
    let fixture = Fixture::new();
    let primary = fixture.vw(&fixture.repo, &["mv", "renamed", "--json"]);
    assert_error(&primary, "INVALID_ARGUMENT");

    let detached_path = fixture.managed("detached");
    git_ok(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "--detach",
            detached_path.to_str().expect("detached utf8"),
            "main",
        ],
    );
    let detached = fixture.vw(&detached_path, &["mv", "detached-renamed", "--json"]);
    assert_error(&detached, "DETACHED_HEAD");

    let created = fixture.vw(&fixture.repo, &["switch", "feature/source", "--json"]);
    assert_success(&created);
    let source_path = fixture.managed("feature/source");
    let before_same = observable_state(&fixture.repo);
    let same = fixture.vw(&source_path, &["mv", "feature/source", "--json"]);
    assert_success(&same);
    assert_eq!(
        json(&same)["data"]["path"],
        source_path.to_string_lossy().as_ref()
    );
    assert_eq!(observable_state(&fixture.repo), before_same);

    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/attached-mv", "--json"]));
    let before = observable_state(&fixture.repo);
    let collision = fixture.vw(&source_path, &["mv", "feature/attached-mv", "--json"]);
    assert_error(&collision, "BRANCH_ALREADY_ATTACHED");
    assert_eq!(observable_state(&fixture.repo), before);

    git_ok(&fixture.repo, &["branch", "feature/existing", "main"]);
    let before = observable_state(&fixture.repo);
    let collision = fixture.vw(&source_path, &["mv", "feature/existing", "--json"]);
    assert_error(&collision, "BRANCH_ALREADY_EXISTS");
    assert_eq!(observable_state(&fixture.repo), before);

    let blocked = fixture.managed("feature/blocked-mv");
    fs::create_dir_all(&blocked).expect("blocked mv target");
    fs::write(blocked.join("keep"), "keep\n").expect("block mv");
    let before = observable_state(&fixture.repo);
    let collision = fixture.vw(&source_path, &["mv", "feature/blocked-mv", "--json"]);
    assert_error(&collision, "TARGET_PATH_NOT_EMPTY");
    assert_eq!(observable_state(&fixture.repo), before);

    let lock = fixture.vw(
        &fixture.repo,
        &[
            "lock",
            "feature/source",
            "--owner",
            "alice",
            "--reason",
            "rename-test",
            "--json",
        ],
    );
    assert_success(&lock);
    let old_lock_path = worktree_lock_file_path(&fixture.repo, "feature/source");
    let old_lifecycle_path = lifecycle_file_path(&fixture.repo, "feature/source");
    let old_lock: Value = serde_json::from_slice(&fs::read(&old_lock_path).expect("old lock"))
        .expect("valid old lock");
    let old_lifecycle: Value =
        serde_json::from_slice(&fs::read(&old_lifecycle_path).expect("old lifecycle"))
            .expect("valid old lifecycle");

    let moved = fixture.vw(&source_path, &["mv", "feature/moved", "--json"]);
    assert_success(&moved);
    let moved_value = json(&moved);
    let target = fixture.managed("feature/moved");
    assert_eq!(moved_value["data"]["branch"], "feature/moved");
    assert_eq!(
        moved_value["data"]["path"],
        target.to_string_lossy().as_ref()
    );
    assert!(!source_path.exists());
    assert!(target.is_dir());
    assert!(git_text(&fixture.repo, &["branch", "--show-current"]).trim() == "main");
    assert!(git_text(&target, &["branch", "--show-current"]).trim() == "feature/moved");
    assert!(!old_lock_path.exists());
    assert!(!old_lifecycle_path.exists());

    let new_lock: Value = serde_json::from_slice(
        &fs::read(worktree_lock_file_path(&fixture.repo, "feature/moved")).expect("new lock"),
    )
    .expect("valid new lock");
    let new_lifecycle: Value = serde_json::from_slice(
        &fs::read(lifecycle_file_path(&fixture.repo, "feature/moved")).expect("new lifecycle"),
    )
    .expect("valid new lifecycle");
    assert_eq!(new_lock["branch"], "feature/moved");
    assert_eq!(new_lock["owner"], "alice");
    assert_eq!(new_lock["createdAt"], old_lock["createdAt"]);
    assert_eq!(new_lifecycle["branch"], "feature/moved");
    assert_eq!(new_lifecycle["createdAt"], old_lifecycle["createdAt"]);
}

#[test]
fn a011_mv_hooks_receive_old_and_new_branch_with_phase_specific_cwd() {
    let fixture = Fixture::new();
    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/hook-old", "--json"]));
    let source = fixture.managed("feature/hook-old");
    let target = fixture.managed("feature/hook-new");
    let trace = fixture.root.path().join("mv-hook-trace");
    for phase in ["pre", "post"] {
        write_hook(
            &fixture.repo,
            &format!("{phase}-mv"),
            &format!(
                "#!/bin/sh\nset -eu\nprintf '{phase}|%s|%s|%s|%s|%s|%s\\n' \"$PWD\" \"$WT_ACTION\" \"$WT_BRANCH\" \"$WT_WORKTREE_PATH\" \"$WT_OLD_BRANCH\" \"$WT_NEW_BRANCH\" >> '{}'\n",
                trace.display()
            ),
        );
    }

    assert_success(&fixture.vw(&source, &["mv", "feature/hook-new", "--json"]));
    assert_eq!(
        fs::read_to_string(trace).expect("mv hook trace"),
        format!(
            "pre|{}|mv|feature/hook-new|{}|feature/hook-old|feature/hook-new\npost|{}|mv|feature/hook-new|{}|feature/hook-old|feature/hook-new\n",
            source.display(),
            target.display(),
            target.display(),
            target.display(),
        )
    );
}

#[test]
fn a011_pending_metadata_transaction_finishes_forward_after_git_was_already_moved() {
    let fixture = Fixture::new();
    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/crash", "--json"]));
    assert_success(&fixture.vw(
        &fixture.repo,
        &["lock", "feature/crash", "--owner", "alice", "--json"],
    ));

    let source_lock = worktree_lock_file_path(&fixture.repo, "feature/crash");
    let source_lifecycle = lifecycle_file_path(&fixture.repo, "feature/crash");
    let source_lock_bytes = fs::read(&source_lock).expect("source lock before crash");
    let source_lifecycle_bytes =
        fs::read(&source_lifecycle).expect("source lifecycle before crash");
    let source = fixture.managed("feature/crash");
    let target = fixture.managed("feature/after-crash");
    let plan = prepare_metadata_rename(MetadataRenameRequest {
        repo_root: &fixture.repo,
        from_branch: "feature/crash",
        to_branch: "feature/after-crash",
        source_path: &source,
        target_path: &target,
        managed_root: &fixture.repo.join(".worktree"),
        target_relative_path: Path::new("feature/after-crash"),
        base_branch: "main",
        observed_diverged_head: None,
    })
    .expect("prepare metadata transaction");
    stage_metadata_rename(&plan).expect("stage metadata before Git mutation");
    git_ok(
        &source,
        &["branch", "-m", "feature/crash", "feature/after-crash"],
    );
    mark_metadata_rename_branch_renamed(&plan).expect("record branch rename");
    git_ok(
        &fixture.repo,
        &[
            "worktree",
            "move",
            source.to_str().expect("source utf8"),
            target.to_str().expect("target utf8"),
        ],
    );
    mark_metadata_rename_worktree_moved(&plan).expect("record completed worktree move");
    let error = commit_metadata_rename_with_injector(
        plan,
        &mut CrashAt {
            step: MetadataTransactionStep::TargetLifecycleInstalled,
        },
    )
    .expect_err("fault injector must interrupt metadata commit");
    assert!(error.to_string().contains("injected crash"));
    assert!(lifecycle_file_path(&fixture.repo, "feature/after-crash").is_file());

    let recovered = fixture.vw(&fixture.repo, &["switch", "feature/after-crash", "--json"]);
    assert_success(&recovered);
    assert_eq!(json(&recovered)["data"]["disposition"], "existing");
    assert!(!source_lock.exists());
    assert!(!source_lifecycle.exists());
    let target_lock: Value = serde_json::from_slice(
        &fs::read(worktree_lock_file_path(
            &fixture.repo,
            "feature/after-crash",
        ))
        .expect("recovered target lock"),
    )
    .expect("valid recovered target lock");
    let target_lifecycle: Value = serde_json::from_slice(
        &fs::read(lifecycle_file_path(&fixture.repo, "feature/after-crash"))
            .expect("recovered target lifecycle"),
    )
    .expect("valid recovered target lifecycle");
    let original_lock: Value =
        serde_json::from_slice(&source_lock_bytes).expect("valid original lock");
    let original_lifecycle: Value =
        serde_json::from_slice(&source_lifecycle_bytes).expect("valid original lifecycle");
    assert_eq!(target_lock["branch"], "feature/after-crash");
    assert_eq!(target_lock["owner"], original_lock["owner"]);
    assert_eq!(target_lock["createdAt"], original_lock["createdAt"]);
    assert_eq!(target_lifecycle["branch"], "feature/after-crash");
    assert_eq!(
        target_lifecycle["createdAt"],
        original_lifecycle["createdAt"]
    );
    let transaction_root = fixture
        .repo
        .join(".vde/worktree/state/metadata-transactions");
    assert!(
        !transaction_root.exists()
            || fs::read_dir(transaction_root)
                .expect("transaction root")
                .next()
                .is_none()
    );
}

#[test]
fn a011_mv_rolls_back_the_branch_when_git_worktree_move_fails_mid_operation() {
    let fixture = Fixture::new();
    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/git-failure", "--json"]));
    assert_success(&fixture.vw(
        &fixture.repo,
        &["lock", "feature/git-failure", "--owner", "alice", "--json"],
    ));
    fixture.stabilize_snapshot();
    let source = fixture.managed("feature/git-failure");
    let before = observable_state(&fixture.repo);

    let wrapper_dir = fixture.root.path().join("git-wrapper");
    fs::create_dir(&wrapper_dir).expect("git wrapper directory");
    write_executable(
        &wrapper_dir.join("git"),
        "#!/bin/sh\nset -eu\nif [ \"${1-}\" = worktree ] && [ \"${2-}\" = move ]; then\n  printf 'injected worktree move failure\\n' >&2\n  exit 97\nfi\nexec \"$REAL_GIT\" \"$@\"\n",
    );

    let rejected = fixture.vw_with_git_wrapper(
        &source,
        &["mv", "feature/after-git-failure", "--json"],
        &wrapper_dir,
    );
    assert_error(&rejected, "GIT_COMMAND_FAILED");
    assert_eq!(observable_state(&fixture.repo), before);
}

#[test]
fn a011_mv_invalid_ref_is_side_effect_free_and_never_runs_hooks() {
    let fixture = Fixture::new();
    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/invalid-ref", "--json"]));
    let source = fixture.managed("feature/invalid-ref");
    let trace = fixture.root.path().join("invalid-ref-hook");
    write_hook(
        &fixture.repo,
        "pre-mv",
        &format!("#!/bin/sh\nprintf ran > '{}'\n", trace.display()),
    );
    fixture.stabilize_snapshot();
    let before = observable_state(&fixture.repo);

    let rejected = fixture.vw(&source, &["mv", "feature/../invalid", "--json"]);
    assert_error(&rejected, "INVALID_ARGUMENT");
    assert_eq!(observable_state(&fixture.repo), before);
    assert!(!trace.exists());
    let transactions = fixture
        .repo
        .join(".vde/worktree/state/metadata-transactions");
    assert!(
        !transactions.exists()
            || fs::read_dir(transactions)
                .expect("transaction directory")
                .next()
                .is_none()
    );
}

#[test]
fn a011_mv_process_crash_after_branch_rename_recovers_branch_path_and_metadata() {
    let fixture = Fixture::new();
    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/crash-window", "--json"]));
    assert_success(&fixture.vw(
        &fixture.repo,
        &["lock", "feature/crash-window", "--owner", "alice", "--json"],
    ));
    let source = fixture.managed("feature/crash-window");
    let target = fixture.managed("feature/crash-recovered");
    let wrapper_dir = fixture.root.path().join("crash-window-wrapper");
    fs::create_dir(&wrapper_dir).expect("wrapper directory");
    write_executable(
        &wrapper_dir.join("git"),
        "#!/bin/sh\nset -eu\nif [ \"${1-}\" = branch ] && [ \"${2-}\" = -m ] && [ \"${3-}\" = feature/crash-window ]; then\n  \"$REAL_GIT\" \"$@\"\n  kill -KILL \"$PPID\"\n  exit 0\nfi\nexec \"$REAL_GIT\" \"$@\"\n",
    );

    let crashed = fixture.vw_with_git_wrapper(
        &source,
        &["mv", "feature/crash-recovered", "--json"],
        &wrapper_dir,
    );
    assert!(!crashed.status.success());
    assert!(source.is_dir());
    assert!(!target.exists());
    assert_eq!(
        git_text(&source, &["branch", "--show-current"]).trim(),
        "feature/crash-recovered"
    );

    let recovered = fixture.vw(
        &fixture.repo,
        &["switch", "feature/crash-recovered", "--json"],
    );
    assert_success(&recovered);
    assert!(!source.exists());
    assert!(target.is_dir());
    assert_eq!(
        git_text(&target, &["branch", "--show-current"]).trim(),
        "feature/crash-recovered"
    );
    assert!(!worktree_lock_file_path(&fixture.repo, "feature/crash-window").exists());
    assert!(worktree_lock_file_path(&fixture.repo, "feature/crash-recovered").is_file());
}

#[test]
fn a011_mv_process_crash_after_worktree_move_before_marker_recovers_forward() {
    let fixture = Fixture::new();
    assert_success(&fixture.vw(
        &fixture.repo,
        &["switch", "feature/move-crash-old", "--json"],
    ));
    assert_success(&fixture.vw(
        &fixture.repo,
        &[
            "lock",
            "feature/move-crash-old",
            "--owner",
            "alice",
            "--json",
        ],
    ));
    let source = fixture.managed("feature/move-crash-old");
    let target = fixture.managed("feature/move-crash-new");
    let wrapper_dir = fixture.root.path().join("move-crash-wrapper");
    fs::create_dir(&wrapper_dir).expect("wrapper directory");
    write_executable(
        &wrapper_dir.join("git"),
        "#!/bin/sh\nset -eu\nif [ \"${1-}\" = worktree ] && [ \"${2-}\" = move ]; then\n  \"$REAL_GIT\" \"$@\"\n  kill -KILL \"$PPID\"\n  exit 0\nfi\nexec \"$REAL_GIT\" \"$@\"\n",
    );

    let crashed = fixture.vw_with_git_wrapper(
        &source,
        &["mv", "feature/move-crash-new", "--json"],
        &wrapper_dir,
    );
    assert!(!crashed.status.success());
    assert!(!source.exists());
    assert!(target.is_dir());
    assert!(worktree_lock_file_path(&fixture.repo, "feature/move-crash-old").is_file());
    assert!(!worktree_lock_file_path(&fixture.repo, "feature/move-crash-new").exists());

    let recovered = fixture.vw(
        &fixture.repo,
        &["switch", "feature/move-crash-new", "--json"],
    );
    assert_success(&recovered);
    assert_eq!(
        git_text(&target, &["branch", "--show-current"]).trim(),
        "feature/move-crash-new"
    );
    assert!(!worktree_lock_file_path(&fixture.repo, "feature/move-crash-old").exists());
    assert!(worktree_lock_file_path(&fixture.repo, "feature/move-crash-new").is_file());
}

#[test]
fn a011_mv_double_fault_keeps_journal_until_next_command_finishes_forward() {
    let fixture = Fixture::new();
    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/double-old", "--json"]));
    let source = fixture.managed("feature/double-old");
    let target = fixture.managed("feature/double-new");
    let wrapper_dir = fixture.root.path().join("double-fault-wrapper");
    fs::create_dir(&wrapper_dir).expect("wrapper directory");
    write_executable(
        &wrapper_dir.join("git"),
        "#!/bin/sh\nset -eu\nif [ \"${1-}\" = worktree ] && [ \"${2-}\" = move ]; then\n  exit 97\nfi\nif [ \"${1-}\" = branch ] && [ \"${2-}\" = -m ] && [ \"${3-}\" = feature/double-new ]; then\n  exit 98\nfi\nexec \"$REAL_GIT\" \"$@\"\n",
    );

    let rejected = fixture.vw_with_git_wrapper(
        &source,
        &["mv", "feature/double-new", "--json"],
        &wrapper_dir,
    );
    let value = assert_error(&rejected, "GIT_COMMAND_FAILED");
    assert!(value["error"]["details"]["rollbackFailures"].is_array());
    let transactions = fixture
        .repo
        .join(".vde/worktree/state/metadata-transactions");
    assert!(fs::read_dir(&transactions).unwrap().next().is_some());
    assert!(source.is_dir());
    assert!(!target.exists());

    let recovered = fixture.vw(&fixture.repo, &["switch", "feature/double-new", "--json"]);
    assert_success(&recovered);
    assert!(!source.exists());
    assert!(target.is_dir());
    assert!(
        !transactions.exists()
            || fs::read_dir(transactions)
                .expect("transactions after recovery")
                .next()
                .is_none()
    );
}

#[test]
fn a011_mv_recovery_rejects_corrupt_out_of_root_journal_target() {
    let fixture = Fixture::new();
    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/corrupt-old", "--json"]));
    let source = fixture.managed("feature/corrupt-old");
    let target = fixture.managed("feature/corrupt-new");
    let plan = prepare_metadata_rename(MetadataRenameRequest {
        repo_root: &fixture.repo,
        from_branch: "feature/corrupt-old",
        to_branch: "feature/corrupt-new",
        source_path: &source,
        target_path: &target,
        managed_root: &fixture.repo.join(".worktree"),
        target_relative_path: Path::new("feature/corrupt-new"),
        base_branch: "main",
        observed_diverged_head: None,
    })
    .unwrap();
    stage_metadata_rename(&plan).unwrap();
    git_ok(
        &source,
        &["branch", "-m", "feature/corrupt-old", "feature/corrupt-new"],
    );
    mark_metadata_rename_branch_renamed(&plan).unwrap();

    let transaction_root = fixture
        .repo
        .join(".vde/worktree/state/metadata-transactions");
    let transaction = fs::read_dir(&transaction_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal_path = transaction.join("journal.json");
    let mut journal: Value = serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    let outside = fixture.root.path().join("outside-corrupt-target");
    journal["targetPath"] = Value::String(outside.to_string_lossy().into_owned());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

    let rejected = fixture.vw(&fixture.repo, &["switch", "feature/corrupt-new", "--json"]);
    assert_error(&rejected, "INTERNAL_ERROR");
    assert!(source.is_dir());
    assert!(!target.exists());
    assert!(!outside.exists());
    assert!(lifecycle_file_path(&fixture.repo, "feature/corrupt-old").is_file());
    assert!(!lifecycle_file_path(&fixture.repo, "feature/corrupt-new").exists());
}

#[test]
fn a011_mv_recovery_rejects_target_ancestor_swapped_to_symlink() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/symlink-old", "--json"]));
    let source = fixture.managed("feature/symlink-old");
    let target = fixture.managed("escape/symlink-new");
    let plan = prepare_metadata_rename(MetadataRenameRequest {
        repo_root: &fixture.repo,
        from_branch: "feature/symlink-old",
        to_branch: "escape/symlink-new",
        source_path: &source,
        target_path: &target,
        managed_root: &fixture.repo.join(".worktree"),
        target_relative_path: Path::new("escape/symlink-new"),
        base_branch: "main",
        observed_diverged_head: None,
    })
    .unwrap();
    stage_metadata_rename(&plan).unwrap();
    git_ok(
        &source,
        &["branch", "-m", "feature/symlink-old", "escape/symlink-new"],
    );
    mark_metadata_rename_branch_renamed(&plan).unwrap();

    let outside = fixture.root.path().join("outside-symlink-target");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, fixture.repo.join(".worktree/escape")).unwrap();
    let rejected = fixture.vw(&fixture.repo, &["switch", "escape/symlink-new", "--json"]);
    assert_error(&rejected, "INTERNAL_ERROR");
    assert!(source.is_dir());
    assert!(!outside.join("symlink-new").exists());
    assert!(lifecycle_file_path(&fixture.repo, "feature/symlink-old").is_file());
    assert!(!lifecycle_file_path(&fixture.repo, "escape/symlink-new").exists());
}

#[test]
fn a012_get_types_remote_failures_and_reuses_the_created_worktree() {
    let fixture = Fixture::new();
    let remote = tempfile::tempdir().expect("bare remote");
    git_ok(remote.path(), &["init", "--quiet", "--bare"]);
    git_ok(
        &fixture.repo,
        &[
            "remote",
            "add",
            "origin",
            remote.path().to_str().expect("remote utf8"),
        ],
    );
    git_ok(&fixture.repo, &["push", "origin", "main"]);
    git_ok(&fixture.repo, &["checkout", "-b", "feature/get"]);
    fs::write(fixture.repo.join("remote.txt"), "remote\n").expect("remote file");
    git_ok(&fixture.repo, &["add", "remote.txt"]);
    git_ok(&fixture.repo, &["commit", "--quiet", "-m", "remote branch"]);
    git_ok(&fixture.repo, &["push", "origin", "feature/get"]);
    git_ok(&fixture.repo, &["checkout", "main"]);
    git_ok(&fixture.repo, &["branch", "-D", "feature/get"]);

    let missing_remote = fixture.vw(&fixture.repo, &["get", "upstream/feature/get", "--json"]);
    assert_error(&missing_remote, "REMOTE_NOT_FOUND");
    let missing_branch = fixture.vw(&fixture.repo, &["get", "origin/feature/missing", "--json"]);
    assert_error(&missing_branch, "REMOTE_BRANCH_NOT_FOUND");

    let created = fixture.vw(&fixture.repo, &["get", "origin/feature/get", "--json"]);
    assert_success(&created);
    let target = fixture.managed("feature/get");
    assert_eq!(json(&created)["data"]["disposition"], "created");
    assert_eq!(
        git_text(&target, &["rev-parse", "--abbrev-ref", "@{upstream}"]).trim(),
        "origin/feature/get"
    );

    let existing = fixture.vw(&fixture.repo, &["get", "origin/feature/get", "--json"]);
    assert_success(&existing);
    assert_eq!(json(&existing)["data"]["disposition"], "existing");
    assert_eq!(
        json(&existing)["data"]["path"],
        target.to_string_lossy().as_ref()
    );
}

#[test]
fn a013_extract_rejects_dirty_and_base_then_stashes_into_the_new_worktree() {
    let fixture = Fixture::new();
    git_ok(&fixture.repo, &["checkout", "-b", "feature/extract"]);
    fs::write(fixture.repo.join("tracked.txt"), "tracked\n").expect("tracked file");
    git_ok(&fixture.repo, &["add", "tracked.txt"]);
    git_ok(
        &fixture.repo,
        &["commit", "--quiet", "-m", "extract branch"],
    );
    fs::write(fixture.repo.join("dirty.txt"), "dirty\n").expect("dirty file");

    let dirty = fixture.vw(&fixture.repo, &["extract", "--current", "--json"]);
    assert_error(&dirty, "DIRTY_WORKTREE");
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]).trim(),
        "feature/extract"
    );

    let extracted = fixture.vw(
        &fixture.repo,
        &["extract", "--current", "--stash", "--json"],
    );
    assert_success(&extracted);
    let target = fixture.managed("feature/extract");
    assert_eq!(json(&extracted)["data"]["branch"], "feature/extract");
    assert_eq!(
        json(&extracted)["data"]["path"],
        target.to_string_lossy().as_ref()
    );
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]).trim(),
        "main"
    );
    assert_eq!(
        git_text(&target, &["branch", "--show-current"]).trim(),
        "feature/extract"
    );
    assert_eq!(
        fs::read_to_string(target.join("dirty.txt")).expect("restored dirty file"),
        "dirty\n"
    );
    assert!(
        git_text(&fixture.repo, &["stash", "list"])
            .trim()
            .is_empty()
    );

    let base = fixture.vw(&fixture.repo, &["extract", "--current", "--json"]);
    assert_error(&base, "INVALID_ARGUMENT");
}

#[test]
fn a013_extract_pre_hook_failure_restores_the_exact_stash_before_returning() {
    let fixture = Fixture::new();
    git_ok(&fixture.repo, &["checkout", "-b", "feature/pre-failure"]);
    fs::write(fixture.repo.join("tracked.txt"), "base\n").expect("tracked file");
    git_ok(&fixture.repo, &["add", "tracked.txt"]);
    git_ok(
        &fixture.repo,
        &["commit", "--quiet", "-m", "tracked extract fixture"],
    );
    fs::write(fixture.repo.join("tracked.txt"), "dirty tracked\n").expect("dirty tracked");
    fs::write(fixture.repo.join("untracked.txt"), "dirty untracked\n").expect("dirty untracked");
    let trace = fixture.root.path().join("extract-pre-status");
    write_hook(
        &fixture.repo,
        "pre-extract",
        &format!(
            "#!/bin/sh\nset -eu\ngit status --porcelain > '{}'\nexit 42\n",
            trace.display()
        ),
    );

    let rejected = fixture.vw(
        &fixture.repo,
        &["extract", "--current", "--stash", "--json"],
    );
    assert_error(&rejected, "HOOK_FAILED");
    assert_eq!(fs::read_to_string(trace).expect("pre-hook status"), "");
    assert_eq!(
        fs::read_to_string(fixture.repo.join("tracked.txt")).expect("restored tracked"),
        "dirty tracked\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("untracked.txt")).expect("restored untracked"),
        "dirty untracked\n"
    );
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]).trim(),
        "feature/pre-failure"
    );
    assert!(
        git_text(&fixture.repo, &["stash", "list"])
            .trim()
            .is_empty()
    );
    assert!(!fixture.managed("feature/pre-failure").exists());
}

#[test]
fn a013_extract_stash_oid_resolution_failure_auto_restores_without_loss() {
    let fixture = Fixture::new();
    git_ok(&fixture.repo, &["checkout", "-b", "feature/oid-crash"]);
    fs::write(fixture.repo.join("tracked.txt"), "base\n").unwrap();
    git_ok(&fixture.repo, &["add", "tracked.txt"]);
    git_ok(&fixture.repo, &["commit", "--quiet", "-m", "tracked"]);
    fs::write(fixture.repo.join("tracked.txt"), "dirty\n").unwrap();
    fs::write(fixture.repo.join("untracked.txt"), "untracked\n").unwrap();
    let wrapper_dir = fixture.root.path().join("oid-failure-wrapper");
    fs::create_dir(&wrapper_dir).unwrap();
    write_executable(
        &wrapper_dir.join("git"),
        "#!/bin/sh\nset -eu\nif [ \"${1-}\" = rev-parse ] && [ \"${4-}\" = 'stash@{0}' ]; then\n  exit 91\nfi\nexec \"$REAL_GIT\" \"$@\"\n",
    );

    let rejected = fixture.vw_with_git_wrapper(
        &fixture.repo,
        &["extract", "--current", "--stash", "--json"],
        &wrapper_dir,
    );
    let value = assert_error(&rejected, "GIT_COMMAND_FAILED");
    assert_eq!(value["error"]["details"]["autoRestoreCompleted"], true);
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]).trim(),
        "feature/oid-crash"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("tracked.txt")).unwrap(),
        "dirty\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("untracked.txt")).unwrap(),
        "untracked\n"
    );
    assert!(
        git_text(&fixture.repo, &["stash", "list"])
            .trim()
            .is_empty()
    );
    assert!(!fixture.managed("feature/oid-crash").exists());
}

#[test]
fn a013_extract_worktree_add_failure_compensates_checkout_and_stash() {
    let fixture = Fixture::new();
    git_ok(&fixture.repo, &["checkout", "-b", "feature/add-failure"]);
    fs::write(fixture.repo.join("tracked.txt"), "base\n").unwrap();
    git_ok(&fixture.repo, &["add", "tracked.txt"]);
    git_ok(&fixture.repo, &["commit", "--quiet", "-m", "tracked"]);
    fs::write(fixture.repo.join("tracked.txt"), "dirty\n").unwrap();
    fs::write(fixture.repo.join("untracked.txt"), "untracked\n").unwrap();
    let wrapper_dir = fixture.root.path().join("extract-add-failure-wrapper");
    fs::create_dir(&wrapper_dir).unwrap();
    write_executable(
        &wrapper_dir.join("git"),
        "#!/bin/sh\nset -eu\nif [ \"${1-}\" = worktree ] && [ \"${2-}\" = add ]; then\n  exit 92\nfi\nexec \"$REAL_GIT\" \"$@\"\n",
    );

    let rejected = fixture.vw_with_git_wrapper(
        &fixture.repo,
        &["extract", "--current", "--stash", "--json"],
        &wrapper_dir,
    );
    let value = assert_error(&rejected, "GIT_COMMAND_FAILED");
    assert_eq!(value["error"]["details"]["autoRestoreCompleted"], true);
    assert_eq!(
        value["error"]["details"]["currentBranch"],
        "feature/add-failure"
    );
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]).trim(),
        "feature/add-failure"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("tracked.txt")).unwrap(),
        "dirty\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("untracked.txt")).unwrap(),
        "untracked\n"
    );
    assert!(
        git_text(&fixture.repo, &["stash", "list"])
            .trim()
            .is_empty()
    );
    assert!(!fixture.managed("feature/add-failure").exists());
}

#[test]
fn a013_extract_target_stash_apply_failure_is_typed_and_preserves_the_stash() {
    let fixture = Fixture::new();
    git_ok(&fixture.repo, &["checkout", "-b", "feature/apply-failure"]);
    fs::write(fixture.repo.join("conflict.txt"), "base\n").expect("conflict base");
    git_ok(&fixture.repo, &["add", "conflict.txt"]);
    git_ok(&fixture.repo, &["commit", "--quiet", "-m", "conflict base"]);
    fs::write(fixture.repo.join("conflict.txt"), "stashed version\n").expect("dirty conflict");
    write_hook(
        &fixture.repo,
        "pre-extract",
        "#!/bin/sh\nset -eu\nprintf 'hook version\\n' > conflict.txt\ngit add conflict.txt\ngit commit --quiet -m 'hook creates conflict'\n",
    );

    let rejected = fixture.vw(
        &fixture.repo,
        &["extract", "--current", "--stash", "--json"],
    );
    let value = assert_error(&rejected, "STASH_APPLY_FAILED");
    let stash_oid = value["error"]["details"]["stashOid"]
        .as_str()
        .expect("stash OID in error details");
    let target = fixture.managed("feature/apply-failure");
    assert!(!target.exists());
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]).trim(),
        "feature/apply-failure"
    );
    assert!(
        git_text(&fixture.repo, &["stash", "list", "--format=%H"])
            .lines()
            .any(|oid| oid == stash_oid)
    );
    assert!(
        fs::read_to_string(fixture.repo.join("conflict.txt"))
            .expect("conflicted primary file")
            .contains("<<<<<<<")
    );
    assert_eq!(value["error"]["details"]["autoRestoreFailed"], true);
    assert_eq!(
        value["error"]["details"]["targetPath"],
        target.to_string_lossy().as_ref()
    );
}

#[test]
fn a014_use_enforces_non_tty_dirty_and_shared_guards() {
    let fixture = Fixture::new();
    git_ok(&fixture.repo, &["branch", "feature/use", "main"]);
    let missing_agent = fixture.vw(&fixture.repo, &["use", "feature/use", "--json"]);
    assert_error(&missing_agent, "UNSAFE_FLAG_REQUIRED");
    let missing_unsafe = fixture.vw(
        &fixture.repo,
        &["use", "feature/use", "--allow-agent", "--json"],
    );
    assert_error(&missing_unsafe, "UNSAFE_FLAG_REQUIRED");

    fs::write(fixture.repo.join("dirty-use"), "dirty\n").expect("dirty primary");
    let dirty = fixture.vw(
        &fixture.repo,
        &[
            "use",
            "feature/use",
            "--allow-agent",
            "--allow-unsafe",
            "--json",
        ],
    );
    assert_error(&dirty, "DIRTY_WORKTREE");
    fs::remove_file(fixture.repo.join("dirty-use")).expect("clean primary");

    let trace = fixture.root.path().join("use-hook-trace");
    for phase in ["pre", "post"] {
        write_hook(
            &fixture.repo,
            &format!("{phase}-use"),
            &format!(
                "#!/bin/sh\nset -eu\nprintf '{phase}|%s|%s|%s|%s\\n' \"$PWD\" \"$WT_ACTION\" \"$WT_BRANCH\" \"$WT_WORKTREE_PATH\" >> '{}'\n",
                trace.display()
            ),
        );
    }
    let used = fixture.vw(
        &fixture.repo,
        &[
            "use",
            "feature/use",
            "--allow-agent",
            "--allow-unsafe",
            "--json",
        ],
    );
    assert_success(&used);
    assert_eq!(json(&used)["data"]["branch"], "feature/use");
    assert_eq!(
        fs::read_to_string(&trace).expect("use hook trace"),
        format!(
            "pre|{}|use|feature/use|{}\npost|{}|use|feature/use|{}\n",
            fixture.repo.display(),
            fixture.repo.display(),
            fixture.repo.display(),
            fixture.repo.display(),
        )
    );

    git_ok(&fixture.repo, &["checkout", "main"]);

    let linked = fixture.vw(&fixture.repo, &["switch", "feature/shared", "--json"]);
    assert_success(&linked);
    let shared_rejected = fixture.vw(
        &fixture.repo,
        &[
            "use",
            "feature/shared",
            "--allow-agent",
            "--allow-unsafe",
            "--json",
        ],
    );
    assert_error(&shared_rejected, "BRANCH_IN_USE");
    let shared_allowed = fixture.vw(
        &fixture.repo,
        &[
            "use",
            "feature/shared",
            "--allow-agent",
            "--allow-unsafe",
            "--allow-shared",
            "--json",
        ],
    );
    assert_success(&shared_allowed);
    assert_eq!(json(&shared_allowed)["data"]["branch"], "feature/shared");
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]).trim(),
        "feature/shared"
    );
}

#[test]
fn a015_lock_unlock_missing_target_and_force_invalid_metadata_are_typed() {
    let fixture = Fixture::new();
    let lock_root = fixture.repo.join(".vde/worktree/locks");
    let before = file_tree(&lock_root);
    let missing = fixture.vw(
        &fixture.repo,
        &["lock", "feature/missing", "--owner", "alice", "--json"],
    );
    assert_error(&missing, "WORKTREE_NOT_FOUND");
    assert_eq!(file_tree(&lock_root), before);

    assert_success(&fixture.vw(&fixture.repo, &["switch", "feature/lock", "--json"]));
    {
        let _guard = acquire_repo_lock(
            &fixture.repo.join(".git"),
            Duration::from_secs(1),
            "phase3-test-owner",
        )
        .expect("hold repository lock");
        let contended = fixture.vw(
            &fixture.repo,
            &[
                "lock",
                "feature/lock",
                "--owner",
                "alice",
                "--lock-timeout-ms",
                "30",
                "--json",
            ],
        );
        assert_error(&contended, "REPO_LOCK_TIMEOUT");
        assert!(!worktree_lock_file_path(&fixture.repo, "feature/lock").exists());
    }
    let locked = fixture.vw(
        &fixture.repo,
        &[
            "lock",
            "feature/lock",
            "--owner",
            "alice",
            "--reason",
            "protected",
            "--json",
        ],
    );
    assert_success(&locked);
    assert_eq!(json(&locked)["data"]["owner"], "alice");
    let lock_path = worktree_lock_file_path(&fixture.repo, "feature/lock");
    assert!(lock_path.is_file());

    let unlocked = fixture.vw(
        &fixture.repo,
        &["unlock", "feature/lock", "--owner", "alice", "--json"],
    );
    assert_success(&unlocked);
    assert!(!lock_path.exists());

    fs::write(&lock_path, b"{invalid json\n").expect("invalid lock metadata");
    let rejected = fixture.vw(
        &fixture.repo,
        &["unlock", "feature/lock", "--owner", "alice", "--json"],
    );
    assert_error(&rejected, "LOCK_CONFLICT");
    assert!(lock_path.is_file());
    let forced = fixture.vw(
        &fixture.repo,
        &[
            "unlock",
            "feature/lock",
            "--owner",
            "alice",
            "--force",
            "--json",
        ],
    );
    assert_success(&forced);
    assert!(!lock_path.exists());
}

#[test]
fn generated_new_branch_is_identical_in_pre_hook_and_final_result() {
    let fixture = Fixture::new();
    let trace = fixture.root.path().join("generated-branch");
    write_hook(
        &fixture.repo,
        "pre-new",
        &format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$WT_BRANCH\" > '{}'\n",
            trace.display()
        ),
    );

    let created = fixture.vw(&fixture.repo, &["new", "--json"]);
    assert_success(&created);
    let value = json(&created);
    let branch = value["data"]["branch"]
        .as_str()
        .expect("generated result branch");
    assert!(branch.starts_with("wip-"));
    assert_eq!(
        fs::read_to_string(trace).expect("generated hook branch"),
        format!("{branch}\n")
    );
    assert_eq!(
        value["data"]["path"],
        fixture.managed(branch).to_string_lossy().as_ref()
    );
}

#[test]
fn phase3_human_success_and_safety_rejection_use_stable_stream_and_exit_contracts() {
    let fixture = Fixture::new();
    let target = fixture.managed("feature/human");
    let created = fixture.vw(&fixture.repo, &["new", "feature/human"]);
    assert_success(&created);
    assert_eq!(
        String::from_utf8(created.stdout).expect("human success stdout"),
        format!("{}\n", target.display())
    );
    assert!(created.stderr.is_empty());

    let rejected = fixture.vw(&fixture.repo, &["new", "feature/human"]);
    assert_eq!(rejected.status.code(), Some(4));
    assert!(rejected.stdout.is_empty());
    let stderr = String::from_utf8(rejected.stderr).expect("human rejection stderr");
    assert!(stderr.starts_with("[BRANCH_ALREADY_ATTACHED] "));
    assert_eq!(stderr.lines().count(), 1);

    let primary_mv = fixture.vw(&fixture.repo, &["mv", "renamed-primary"]);
    assert_eq!(primary_mv.status.code(), Some(3));
    assert!(primary_mv.stdout.is_empty());
    assert!(
        String::from_utf8(primary_mv.stderr)
            .expect("human mv rejection")
            .starts_with("[INVALID_ARGUMENT] ")
    );

    let missing_lock = fixture.vw(
        &fixture.repo,
        &["lock", "feature/missing", "--owner", "alice"],
    );
    assert_eq!(missing_lock.status.code(), Some(4));
    assert!(missing_lock.stdout.is_empty());
    assert!(
        String::from_utf8(missing_lock.stderr)
            .expect("human lock rejection")
            .starts_with("[WORKTREE_NOT_FOUND] ")
    );
}
