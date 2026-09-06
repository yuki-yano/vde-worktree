#![cfg(unix)]

mod support;

use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;
use vde_worktree::state::lifecycle::lifecycle_file_path;
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
        git_ok(&repo, &["config", "user.name", "Phase Four"]);
        git_ok(&repo, &["config", "user.email", "phase4@example.com"]);
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
            .env("USER", "phase4-user")
            .env_remove("NO_COLOR")
            .output()
            .expect("run vw")
    }

    fn managed(&self, branch: &str) -> PathBuf {
        self.repo.join(".worktree").join(branch)
    }

    fn add_origin(&self) {
        let remote = self.root.path().join("origin.git");
        fs::create_dir(&remote).expect("remote directory");
        git_ok(&remote, &["init", "--quiet", "--bare"]);
        git_ok(&self.repo, &["remote", "add", "origin", utf8(&remote)]);
        git_ok(&self.repo, &["push", "--quiet", "-u", "origin", "main"]);
    }

    fn create_tracked_worktree(&self, branch: &str) -> PathBuf {
        let created = self.vw(&self.repo, &["switch", branch, "--no-gh", "--json"]);
        assert_success(&created);
        let target = self.managed(branch);
        git_ok(&target, &["push", "--quiet", "-u", "origin", branch]);
        target
    }

    fn create_merged_tracked_worktree(&self, branch: &str, marker: &str) -> PathBuf {
        let target = self.create_tracked_worktree(branch);
        fs::write(target.join(marker), format!("{branch}\n")).expect("merged marker");
        git_ok(&target, &["add", marker]);
        git_ok(&target, &["commit", "--quiet", "-m", "merged work"]);
        git_ok(&target, &["push", "--quiet", "origin", branch]);
        git_ok(&self.repo, &["merge", "--quiet", "--ff-only", branch]);
        target
    }
}

fn vw_on_pty(fixture: &Fixture, args: &[&str]) -> (std::process::ExitStatus, Vec<u8>) {
    use nix::pty::{Winsize, openpty};

    let pty = openpty(
        Some(&Winsize {
            ws_row: 40,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )
    .expect("open PTY");
    let slave = fs::File::from(pty.slave);
    let mut command = Command::new(env!("CARGO_BIN_EXE_vw"));
    command
        .current_dir(&fixture.repo)
        .args(args)
        .env("HOME", &fixture.home)
        .env("XDG_CONFIG_HOME", &fixture.xdg)
        .env("GIT_CONFIG_GLOBAL", &fixture.git_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("USER", "phase4-user")
        .env_remove("NO_COLOR");
    let mut child = command
        .stdin(Stdio::from(slave.try_clone().expect("clone PTY slave")))
        .stdout(Stdio::from(slave.try_clone().expect("clone PTY slave")))
        .stderr(Stdio::from(slave.try_clone().expect("clone PTY slave")))
        .spawn()
        .expect("spawn vw on PTY");
    drop(command);
    drop(slave);
    let master = fs::File::from(pty.master);
    let reader = std::thread::spawn(move || {
        let mut master = master;
        let mut output = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.raw_os_error() == Some(5) => break,
                Err(error) => panic!("read PTY output: {error}"),
            }
        }
        output
    });
    let status = child.wait().expect("wait for PTY child");
    (status, reader.join().expect("join PTY reader"))
}

fn utf8(path: &Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}

fn git_ok(cwd: &Path, args: &[&str]) {
    let output = git_output(cwd, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
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
    String::from_utf8(output.stdout).expect("UTF-8 git output")
}

fn write_hook(repo: &Path, name: &str, script: &str) {
    let path = repo.join(".vde/worktree/hooks").join(name);
    fs::write(&path, script).expect("write hook");
    let mut permissions = fs::metadata(&path).expect("hook metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make hook executable");
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
struct DeleteEvidence {
    refs: Vec<u8>,
    worktree_porcelain: Vec<u8>,
    head_tree: Vec<u8>,
    status: Vec<u8>,
    worktree_diff: Vec<u8>,
    index_diff: Vec<u8>,
    lock_metadata: Option<Vec<u8>>,
    lifecycle_metadata: Option<Vec<u8>>,
}

fn git_bytes(cwd: &Path, args: &[&str]) -> Vec<u8> {
    let output = git_output(cwd, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn capture_delete_evidence(fixture: &Fixture, branch: &str, target: &Path) -> DeleteEvidence {
    DeleteEvidence {
        refs: git_bytes(
            &fixture.repo,
            &[
                "for-each-ref",
                "--format=%(refname)%09%(objectname)",
                "refs/heads",
            ],
        ),
        worktree_porcelain: git_bytes(&fixture.repo, &["worktree", "list", "--porcelain", "-z"]),
        head_tree: git_bytes(target, &["rev-parse", "HEAD^{tree}"]),
        status: git_bytes(target, &["status", "--porcelain=v1", "-uall"]),
        worktree_diff: git_bytes(target, &["diff", "--binary"]),
        index_diff: git_bytes(target, &["diff", "--cached", "--binary"]),
        lock_metadata: fs::read(worktree_lock_file_path(&fixture.repo, branch)).ok(),
        lifecycle_metadata: fs::read(lifecycle_file_path(&fixture.repo, branch)).ok(),
    }
}

fn assert_delete_error_without_mutation(
    fixture: &Fixture,
    cwd: &Path,
    args: &[&str],
    code: &str,
    branch: &str,
    target: &Path,
) {
    let before = capture_delete_evidence(fixture, branch, target);
    let output = fixture.vw(cwd, args);
    assert_error(&output, code);
    let after = capture_delete_evidence(fixture, branch, target);
    assert_eq!(before, after, "rejected command mutated evidence: {args:?}");
}

#[test]
fn a016_del_enforces_structural_dirty_and_lock_guards() {
    let fixture = Fixture::new();
    fixture.add_origin();

    let primary = fixture.vw(&fixture.repo, &["del", "main", "--no-gh", "--json"]);
    assert_error(&primary, "INVALID_ARGUMENT");

    let unmanaged = fixture.root.path().join("unmanaged");
    git_ok(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/unmanaged",
            utf8(&unmanaged),
            "main",
        ],
    );
    let unmanaged_result = fixture.vw(
        &fixture.repo,
        &["del", "feature/unmanaged", "--no-gh", "--json"],
    );
    assert_error(&unmanaged_result, "WORKTREE_NOT_FOUND");
    assert!(unmanaged.is_dir());

    let dirty_path = fixture.create_tracked_worktree("feature/del-dirty");
    fs::write(dirty_path.join("dirty.txt"), "dirty\n").expect("dirty file");
    assert_delete_error_without_mutation(
        &fixture,
        &fixture.repo,
        &["del", "feature/del-dirty", "--no-gh", "--json"],
        "DIRTY_WORKTREE",
        "feature/del-dirty",
        &dirty_path,
    );

    let locked_path = fixture.create_tracked_worktree("feature/del-locked");
    assert_success(&fixture.vw(
        &fixture.repo,
        &[
            "lock",
            "feature/del-locked",
            "--owner",
            "phase4-user",
            "--json",
        ],
    ));
    assert_delete_error_without_mutation(
        &fixture,
        &fixture.repo,
        &["del", "feature/del-locked", "--no-gh", "--json"],
        "LOCKED_WORKTREE",
        "feature/del-locked",
        &locked_path,
    );

    let native_locked_path = fixture.create_tracked_worktree("feature/del-native-locked");
    git_ok(
        &fixture.repo,
        &[
            "worktree",
            "lock",
            "--reason",
            "native",
            utf8(&native_locked_path),
        ],
    );
    assert_delete_error_without_mutation(
        &fixture,
        &fixture.repo,
        &["del", "feature/del-native-locked", "--no-gh", "--json"],
        "LOCKED_WORKTREE",
        "feature/del-native-locked",
        &native_locked_path,
    );
}

#[test]
fn a016_branchless_del_from_linked_cwd_rejects_shared_branch_before_pre_hook() {
    let fixture = Fixture::new();
    let branch = "feature/del-shared-cwd";
    let first = fixture.managed(branch);
    assert_success(&fixture.vw(&fixture.repo, &["switch", branch, "--no-gh", "--json"]));
    let second = fixture.repo.join(".worktree/shared-copy");
    git_ok(
        &fixture.repo,
        &["worktree", "add", "--detach", utf8(&second), "main"],
    );
    git_ok(&second, &["checkout", "--ignore-other-worktrees", branch]);
    let hook_marker = fixture.root.path().join("shared-del-pre-hook");
    write_hook(
        &fixture.repo,
        "pre-del",
        &format!("#!/bin/sh\nprintf ran > '{}'\n", hook_marker.display()),
    );

    assert_delete_error_without_mutation(
        &fixture,
        &first,
        &["del", "--no-gh", "--json"],
        "INVALID_ARGUMENT",
        branch,
        &first,
    );
    assert!(!hook_marker.exists(), "pre-del hook must not run");
}

#[test]
fn a016_del_enforces_merge_push_success_and_human_hook_contracts() {
    let fixture = Fixture::new();
    fixture.add_origin();
    let unmerged_path = fixture.create_tracked_worktree("feature/del-unmerged");
    fs::write(unmerged_path.join("unmerged.txt"), "unmerged\n").expect("unmerged file");
    git_ok(&unmerged_path, &["add", "unmerged.txt"]);
    git_ok(
        &unmerged_path,
        &["commit", "--quiet", "-m", "unmerged commit"],
    );
    git_ok(
        &unmerged_path,
        &["push", "--quiet", "origin", "feature/del-unmerged"],
    );
    assert_delete_error_without_mutation(
        &fixture,
        &fixture.repo,
        &["del", "feature/del-unmerged", "--no-gh", "--json"],
        "UNMERGED_WORKTREE",
        "feature/del-unmerged",
        &unmerged_path,
    );

    let unpushed_path = fixture.create_tracked_worktree("feature/del-unpushed");
    fs::write(unpushed_path.join("unpushed.txt"), "unpushed\n").expect("unpushed file");
    git_ok(&unpushed_path, &["add", "unpushed.txt"]);
    git_ok(
        &unpushed_path,
        &["commit", "--quiet", "-m", "unpushed commit"],
    );
    git_ok(
        &fixture.repo,
        &["merge", "--quiet", "--ff-only", "feature/del-unpushed"],
    );
    assert_delete_error_without_mutation(
        &fixture,
        &fixture.repo,
        &["del", "feature/del-unpushed", "--no-gh", "--json"],
        "UNPUSHED_WORKTREE",
        "feature/del-unpushed",
        &unpushed_path,
    );

    let unknown_path = fixture.create_merged_tracked_worktree("feature/del-unknown", "unknown.txt");
    git_ok(&unknown_path, &["branch", "--unset-upstream"]);
    assert_delete_error_without_mutation(
        &fixture,
        &fixture.repo,
        &["del", "feature/del-unknown", "--no-gh", "--json"],
        "UNPUSHED_WORKTREE",
        "feature/del-unknown",
        &unknown_path,
    );

    let safe_path = fixture.create_merged_tracked_worktree("feature/del-safe", "del-safe.txt");
    let safe = fixture.vw(
        &fixture.repo,
        &["del", "feature/del-safe", "--no-gh", "--json"],
    );
    assert_success(&safe);
    let safe = json(&safe);
    assert_eq!(safe["data"]["branch"], "feature/del-safe");
    assert_eq!(safe["data"]["path"], utf8(&safe_path));
    assert!(safe["data"].get("progress").is_none());
    assert!(!safe_path.exists());
    assert!(
        !git_text(&fixture.repo, &["branch", "--list", "feature/del-safe"])
            .contains("feature/del-safe")
    );

    let human_path = fixture.create_merged_tracked_worktree("feature/del-human", "del-human.txt");
    let pre_pwd = fixture.root.path().join("del-pre-pwd");
    let post_pwd = fixture.root.path().join("del-post-pwd");
    write_hook(
        &fixture.repo,
        "pre-del",
        &format!("#!/bin/sh\npwd > '{}'\n", pre_pwd.display()),
    );
    write_hook(
        &fixture.repo,
        "post-del",
        &format!("#!/bin/sh\npwd > '{}'\n", post_pwd.display()),
    );
    let human = fixture.vw(&fixture.repo, &["del", "feature/del-human", "--no-gh"]);
    assert_success(&human);
    assert_eq!(
        String::from_utf8_lossy(&human.stdout),
        format!("{}\n", human_path.display())
    );
    assert_eq!(
        fs::read_to_string(pre_pwd).expect("del pre pwd").trim(),
        utf8(&human_path)
    );
    assert_eq!(
        fs::read_to_string(post_pwd).expect("del post pwd").trim(),
        utf8(&fixture.repo)
    );
}

#[test]
fn a017_del_force_in_non_tty_requires_explicit_allow_unsafe() {
    let fixture = Fixture::new();
    fixture.add_origin();
    let target = fixture.create_tracked_worktree("feature/del-force");
    fs::write(target.join("dirty.txt"), "dirty\n").expect("dirty file");

    for force_option in [
        "--force",
        "--force-dirty",
        "--allow-unpushed",
        "--force-unmerged",
        "--force-locked",
    ] {
        assert_delete_error_without_mutation(
            &fixture,
            &fixture.repo,
            &[
                "del",
                "feature/del-force",
                force_option,
                "--no-gh",
                "--json",
            ],
            "UNSAFE_FLAG_REQUIRED",
            "feature/del-force",
            &target,
        );
    }

    let allowed = fixture.vw(
        &fixture.repo,
        &[
            "del",
            "feature/del-force",
            "--force",
            "--allow-unsafe",
            "--no-gh",
            "--json",
        ],
    );
    assert_success(&allowed);
    assert!(!target.exists());
    let allowed = json(&allowed);
    assert_eq!(allowed["data"]["branch"], "feature/del-force");
    assert_eq!(allowed["data"]["path"], utf8(&target));
}

#[test]
fn a017_interactive_tty_allows_explicit_force_without_non_tty_consent() {
    let fixture = Fixture::new();
    fixture.add_origin();
    let target = fixture.create_tracked_worktree("feature/del-force-tty");
    fs::write(target.join("dirty.txt"), "dirty\n").expect("dirty file");

    let (status, output) = vw_on_pty(
        &fixture,
        &["del", "feature/del-force-tty", "--force", "--no-gh"],
    );
    assert!(
        status.success(),
        "PTY output: {}",
        String::from_utf8_lossy(&output)
    );
    assert!(!target.exists());
    assert!(String::from_utf8_lossy(&output).contains(utf8(&target)));
}

#[test]
fn a017_force_locked_removes_a_real_git_native_lock_with_double_force() {
    let fixture = Fixture::new();
    fixture.add_origin();
    let target = fixture.create_merged_tracked_worktree("feature/del-native-force", "native.txt");
    git_ok(
        &fixture.repo,
        &["worktree", "lock", "--reason", "native", utf8(&target)],
    );
    let deleted = fixture.vw(
        &fixture.repo,
        &[
            "del",
            "feature/del-native-force",
            "--force-locked",
            "--allow-unsafe",
            "--no-gh",
            "--json",
        ],
    );
    assert_success(&deleted);
    assert!(!target.exists());
    assert_eq!(json(&deleted)["data"]["branch"], "feature/del-native-force");
}

#[test]
fn invalid_delete_metadata_is_rejected_before_git_with_zero_ref_tree_difference() {
    for (branch, metadata_path) in [
        (
            "feature/del-invalid-lock",
            worktree_lock_file_path as fn(&Path, &str) -> PathBuf,
        ),
        (
            "feature/del-invalid-lifecycle",
            lifecycle_file_path as fn(&Path, &str) -> PathBuf,
        ),
    ] {
        let fixture = Fixture::new();
        fixture.add_origin();
        let target = fixture.create_merged_tracked_worktree(branch, "marker.txt");
        let state_path = metadata_path(&fixture.repo, branch);
        fs::create_dir_all(state_path.parent().expect("state parent")).expect("state directory");
        fs::write(&state_path, b"{not-json}\n").expect("invalid metadata");
        let head_before = git_text(&fixture.repo, &["rev-parse", branch]);
        let tree_before = git_text(&target, &["status", "--porcelain=v1", "-uall"]);

        let rejected = fixture.vw(
            &fixture.repo,
            &[
                "del",
                branch,
                "--force",
                "--allow-unsafe",
                "--no-gh",
                "--json",
            ],
        );
        assert_error(&rejected, "LOCK_CONFLICT");
        assert!(target.is_dir());
        assert_eq!(git_text(&fixture.repo, &["rev-parse", branch]), head_before);
        assert_eq!(
            git_text(&target, &["status", "--porcelain=v1", "-uall"]),
            tree_before
        );
        assert_eq!(
            fs::read(&state_path).expect("metadata retained"),
            b"{not-json}\n"
        );
    }
}

#[test]
fn a018_gone_dry_run_has_no_hooks_and_apply_reports_deleted_and_failed_candidates() {
    let fixture = Fixture::new();
    fixture.add_origin();
    let failed_path =
        fixture.create_merged_tracked_worktree("feature/gone-failed", "gone-failed.txt");
    let deleted_path = fixture.create_merged_tracked_worktree("feature/gone-ok", "gone-ok.txt");
    let hook_marker = fixture.root.path().join("gone-hook-ran");
    write_hook(
        &fixture.repo,
        "pre-gone",
        &format!(
            "#!/bin/sh\nset -eu\nprintf ran > '{}'\n",
            hook_marker.display()
        ),
    );

    let dry_run = fixture.vw(&fixture.repo, &["gone", "--dry-run", "--no-gh", "--json"]);
    assert_success(&dry_run);
    let dry_run = json(&dry_run);
    assert_eq!(dry_run["data"]["dryRun"], true);
    assert_eq!(
        dry_run["data"]["candidates"],
        serde_json::json!(["feature/gone-failed", "feature/gone-ok"])
    );
    assert!(!hook_marker.exists(), "gone --dry-run must not run hooks");
    assert!(failed_path.is_dir());
    assert!(deleted_path.is_dir());

    write_hook(
        &fixture.repo,
        "pre-gone",
        &format!(
            "#!/bin/sh\nset -eu\nprintf dirty > '{}'\n",
            failed_path.join("made-dirty-by-hook").display()
        ),
    );
    let applied = fixture.vw(&fixture.repo, &["gone", "--apply", "--no-gh", "--json"]);
    let applied = assert_error(&applied, "GIT_COMMAND_FAILED");
    assert_eq!(applied["data"]["dryRun"], false);
    assert_eq!(
        applied["data"]["candidates"],
        serde_json::json!(["feature/gone-failed", "feature/gone-ok"])
    );
    assert_eq!(
        applied["data"]["deleted"],
        serde_json::json!(["feature/gone-ok"])
    );
    assert_eq!(
        applied["data"]["failed"][0]["branch"],
        "feature/gone-failed"
    );
    assert_eq!(applied["data"]["failed"][0]["phase"], "revalidation");
    assert_eq!(applied["data"]["failed"][0]["code"], "DIRTY_WORKTREE");
    assert!(failed_path.is_dir());
    assert!(!deleted_path.exists());
}

#[test]
fn a019_absorb_restores_on_pre_hook_failure_and_uses_the_exact_stash_oid() {
    let fixture = Fixture::new();
    let source = fixture.managed("feature/absorb");
    assert_success(&fixture.vw(
        &fixture.repo,
        &["switch", "feature/absorb", "--no-gh", "--json"],
    ));
    fs::write(source.join("from-source.txt"), "source\n").expect("source change");

    let denied = fixture.vw(
        &fixture.repo,
        &["absorb", "feature/absorb", "--no-gh", "--json"],
    );
    assert_error(&denied, "UNSAFE_FLAG_REQUIRED");
    assert!(source.join("from-source.txt").is_file());

    write_hook(&fixture.repo, "pre-absorb", "#!/bin/sh\nexit 17\n");
    let hook_failed = fixture.vw(
        &fixture.repo,
        &[
            "absorb",
            "feature/absorb",
            "--allow-agent",
            "--allow-unsafe",
            "--no-gh",
            "--json",
        ],
    );
    assert_error(&hook_failed, "HOOK_FAILED");
    assert_eq!(
        fs::read_to_string(source.join("from-source.txt")).expect("restored source change"),
        "source\n"
    );
    assert!(!fixture.repo.join("from-source.txt").exists());
    assert!(
        git_text(&fixture.repo, &["stash", "list", "--format=%H"])
            .trim()
            .is_empty()
    );

    write_hook(
        &fixture.repo,
        "pre-absorb",
        "#!/bin/sh\nset -eu\nprintf hook > \"$WT_TARGET/hook-only.txt\"\ngit -C \"$WT_TARGET\" stash push -u -m hook-stash >/dev/null\n",
    );
    let absorbed = fixture.vw(
        &fixture.repo,
        &[
            "absorb",
            "feature/absorb",
            "--allow-agent",
            "--allow-unsafe",
            "--no-gh",
            "--json",
        ],
    );
    assert_success(&absorbed);
    let absorbed = json(&absorbed);
    assert_eq!(absorbed["data"]["direction"], "absorb");
    assert_eq!(absorbed["data"]["stashed"], true);
    assert_eq!(absorbed["data"]["stashRef"], Value::Null);
    assert_eq!(
        fs::read_to_string(fixture.repo.join("from-source.txt")).expect("absorbed change"),
        "source\n"
    );
    assert!(!source.join("from-source.txt").exists());
    assert!(!fixture.repo.join("hook-only.txt").exists());
    let stashes = git_text(&fixture.repo, &["stash", "list", "--format=%gs"]);
    assert_eq!(
        stashes.lines().collect::<Vec<_>>(),
        vec!["On main: hook-stash"]
    );
}

#[test]
fn a020_unabsorb_enforces_safety_and_can_retain_the_exact_transfer_stash() {
    let fixture = Fixture::new();
    let target = fixture.managed("feature/unabsorb");
    assert_success(&fixture.vw(
        &fixture.repo,
        &["switch", "feature/unabsorb", "--no-gh", "--json"],
    ));

    let wrong_branch = fixture.vw(
        &fixture.repo,
        &[
            "unabsorb",
            "feature/unabsorb",
            "--allow-agent",
            "--allow-unsafe",
            "--no-gh",
            "--json",
        ],
    );
    assert_error(&wrong_branch, "INVALID_ARGUMENT");

    git_ok(
        &fixture.repo,
        &["checkout", "--ignore-other-worktrees", "feature/unabsorb"],
    );
    fs::write(fixture.repo.join("README.md"), "primary\n").expect("primary change");
    let denied = fixture.vw(
        &fixture.repo,
        &["unabsorb", "feature/unabsorb", "--no-gh", "--json"],
    );
    assert_error(&denied, "UNSAFE_FLAG_REQUIRED");
    assert_eq!(
        fs::read_to_string(fixture.repo.join("README.md")).expect("primary change retained"),
        "primary\n"
    );

    fs::write(target.join("target-dirty.txt"), "dirty\n").expect("target dirty");
    let dirty_target = fixture.vw(
        &fixture.repo,
        &[
            "unabsorb",
            "feature/unabsorb",
            "--allow-agent",
            "--allow-unsafe",
            "--no-gh",
            "--json",
        ],
    );
    assert_error(&dirty_target, "DIRTY_WORKTREE");
    fs::remove_file(target.join("target-dirty.txt")).expect("clean target");

    let unabsorbed = fixture.vw(
        &fixture.repo,
        &[
            "unabsorb",
            "feature/unabsorb",
            "--keep-stash",
            "--allow-agent",
            "--allow-unsafe",
            "--no-gh",
            "--json",
        ],
    );
    assert_success(&unabsorbed);
    let unabsorbed = json(&unabsorbed);
    assert_eq!(unabsorbed["data"]["direction"], "unabsorb");
    assert_eq!(unabsorbed["data"]["stashed"], true);
    assert!(unabsorbed["data"]["stashRef"].as_str().is_some());
    assert_eq!(
        fs::read_to_string(target.join("README.md")).expect("unabsorbed change"),
        "primary\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("README.md")).expect("clean primary"),
        "initial\n"
    );
    let transfer_oid = git_text(&fixture.repo, &["stash", "list", "--format=%H"]);
    assert_eq!(transfer_oid.lines().count(), 1);
    assert_eq!(
        git_text(&fixture.repo, &["show", "stash@{0}:README.md"]),
        "primary\n"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn transfer_hook_cwd_and_strict_post_failure_preserve_the_applied_result() {
    let fixture = Fixture::new();
    let source = fixture.managed("feature/hook-cwd");
    assert_success(&fixture.vw(
        &fixture.repo,
        &["switch", "feature/hook-cwd", "--no-gh", "--json"],
    ));
    fs::write(source.join("from-source.txt"), "source\n").expect("source change");
    let pre_pwd = fixture.root.path().join("absorb-pre-pwd");
    let post_pwd = fixture.root.path().join("absorb-post-pwd");
    write_hook(
        &fixture.repo,
        "pre-absorb",
        &format!(
            "#!/bin/sh\nset -eu\npwd > '{}'\ntest \"$WT_SOURCE\" = '{}'\ntest \"$WT_TARGET\" = '{}'\n",
            pre_pwd.display(),
            source.display(),
            fixture.repo.display(),
        ),
    );
    write_hook(
        &fixture.repo,
        "post-absorb",
        &format!(
            "#!/bin/sh\nset -eu\npwd > '{}'\ntest \"$WT_SOURCE\" = '{}'\ntest \"$WT_TARGET\" = '{}'\nexit 19\n",
            post_pwd.display(),
            source.display(),
            fixture.repo.display(),
        ),
    );
    let non_strict = fixture.vw(
        &fixture.repo,
        &[
            "absorb",
            "feature/hook-cwd",
            "--allow-agent",
            "--allow-unsafe",
            "--no-gh",
            "--json",
        ],
    );
    assert_success(&non_strict);
    let report: Value = serde_json::from_slice(&non_strict.stdout).expect("JSON warning result");
    assert_eq!(report["warnings"][0]["code"], "HOOK_FAILED");
    assert_eq!(report["warnings"][0]["details"]["hook"], "post-absorb");
    assert!(String::from_utf8_lossy(&non_strict.stderr).contains("Warning:"));
    assert_eq!(
        fs::read_to_string(pre_pwd).expect("pre pwd").trim(),
        utf8(&source)
    );
    assert_eq!(
        fs::read_to_string(post_pwd).expect("post pwd").trim(),
        utf8(&fixture.repo)
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("from-source.txt")).expect("applied result"),
        "source\n"
    );
    git_ok(&fixture.repo, &["add", "from-source.txt"]);
    git_ok(
        &fixture.repo,
        &["commit", "--quiet", "-m", "complete first transfer"],
    );

    let second = fixture.managed("feature/hook-strict");
    assert_success(&fixture.vw(
        &fixture.repo,
        &["switch", "feature/hook-strict", "--no-gh", "--json"],
    ));
    fs::write(second.join("strict.txt"), "strict\n").expect("strict source change");
    let strict_post_pwd = fixture.root.path().join("strict-post-pwd");
    write_hook(&fixture.repo, "pre-absorb", "#!/bin/sh\nexit 0\n");
    write_hook(
        &fixture.repo,
        "post-absorb",
        &format!(
            "#!/bin/sh\npwd > '{}'\nexit 23\n",
            strict_post_pwd.display()
        ),
    );
    let strict = fixture.vw(
        &fixture.repo,
        &[
            "absorb",
            "feature/hook-strict",
            "--allow-agent",
            "--allow-unsafe",
            "--strict-post-hooks",
            "--no-gh",
            "--json",
        ],
    );
    let strict = assert_error(&strict, "HOOK_FAILED");
    assert_eq!(strict["data"]["branch"], "feature/hook-strict");
    assert_eq!(strict["data"]["path"], utf8(&fixture.repo));
    assert_eq!(strict["error"]["execution"]["phase"], "postHook");
    assert_eq!(strict["error"]["execution"]["state"], "applied");
    assert_eq!(strict["error"]["details"]["hook"], "post-absorb");
    assert_eq!(strict["error"]["details"]["phase"], "post");
    assert!(
        Path::new(
            strict["error"]["details"]["logPath"]
                .as_str()
                .expect("hook log path")
        )
        .is_file()
    );
    assert_eq!(
        fs::read_to_string(strict_post_pwd)
            .expect("strict post pwd")
            .trim(),
        utf8(&fixture.repo)
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("strict.txt")).expect("strict applied result"),
        "strict\n"
    );
}
