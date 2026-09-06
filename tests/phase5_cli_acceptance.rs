#![cfg(unix)]

mod support;

use std::fs;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;
use vde_worktree::state::repo_lock::acquire_repo_lock;

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
        git_ok(&repo, &["config", "user.name", "Phase Five"]);
        git_ok(&repo, &["config", "user.email", "phase5@example.com"]);
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

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vw"));
        command
            .current_dir(cwd)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.xdg)
            .env("GIT_CONFIG_GLOBAL", &self.git_config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("USER", "phase5-user")
            .env_remove("NO_COLOR");
        command
    }

    fn vw(&self, cwd: &Path, args: &[&str]) -> Output {
        self.command(cwd).args(args).output().expect("run vw")
    }

    fn switch(&self, branch: &str) -> PathBuf {
        let output = self.vw(&self.repo, &["switch", branch, "--no-gh", "--json"]);
        assert_success(&output);
        PathBuf::from(
            json(&output)["data"]["path"]
                .as_str()
                .expect("switch data.path"),
        )
    }
}

fn utf8(path: &Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}

fn git_ok(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
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

fn assert_error(output: &Output, code: &str, exit_code: i32) -> Value {
    let value = json(output);
    assert_eq!(output.status.code(), Some(exit_code));
    assert_eq!(value["schemaVersion"], 3);
    assert_eq!(value["status"], "error");
    assert_eq!(value["error"]["code"], code);
    value
}

fn write_hook(repo: &Path, name: &str, script: &str, executable: bool) {
    let directory = repo.join(".vde/worktree/hooks");
    fs::create_dir_all(&directory).expect("hook directory");
    let path = directory.join(name);
    fs::write(&path, script).expect("write hook");
    let mut permissions = fs::metadata(&path).expect("hook metadata").permissions();
    permissions.set_mode(if executable { 0o755 } else { 0o644 });
    fs::set_permissions(path, permissions).expect("set hook permissions");
}

fn tree_snapshot(root: &Path) -> Vec<(PathBuf, String, Vec<u8>)> {
    fn visit(root: &Path, current: &Path, entries: &mut Vec<(PathBuf, String, Vec<u8>)>) {
        let mut children = fs::read_dir(current)
            .expect("read snapshot directory")
            .map(|entry| entry.expect("snapshot entry"))
            .collect::<Vec<_>>();
        children.sort_by_key(std::fs::DirEntry::file_name);
        for entry in children {
            let path = entry.path();
            let relative = path.strip_prefix(root).expect("snapshot relative path");
            let metadata = fs::symlink_metadata(&path).expect("snapshot metadata");
            if metadata.file_type().is_symlink() {
                entries.push((
                    relative.to_path_buf(),
                    "symlink".to_owned(),
                    fs::read_link(&path)
                        .expect("snapshot symlink")
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec(),
                ));
            } else if metadata.is_dir() {
                entries.push((relative.to_path_buf(), "directory".to_owned(), Vec::new()));
                visit(root, &path, entries);
            } else if metadata.is_file() {
                entries.push((
                    relative.to_path_buf(),
                    "file".to_owned(),
                    fs::read(&path).expect("snapshot file"),
                ));
            } else {
                entries.push((relative.to_path_buf(), "special".to_owned(), Vec::new()));
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    entries
}

#[test]
fn a021_exec_skips_repo_lock_and_preserves_human_json_and_failure_contracts() {
    let fixture = Fixture::new();
    let target = fixture.switch("topic");
    let _lock = acquire_repo_lock(
        &fixture.repo.join(".git"),
        Duration::from_secs(1),
        "phase5-acceptance",
    )
    .expect("hold repository lock");

    let captured = fixture.vw(
        &fixture.repo,
        &[
            "exec",
            "topic",
            "--json",
            "--lock-timeout-ms",
            "1",
            "--",
            "/bin/sh",
            "-c",
            "printf child-out; printf child-err >&2; printf %s \"$PWD\"",
        ],
    );
    assert_success(&captured);
    let captured = json(&captured);
    assert_eq!(captured["command"], "exec");
    assert_eq!(captured["data"]["branch"], "topic");
    assert_eq!(captured["data"]["path"], utf8(&target));
    assert_eq!(captured["data"]["childExitCode"], 0);
    assert_eq!(
        captured["data"]["childStdout"],
        format!("child-out{}", target.display())
    );
    assert_eq!(captured["data"]["childStderr"], "child-err");

    let human = fixture.vw(
        &fixture.repo,
        &[
            "exec",
            "topic",
            "--lock-timeout-ms",
            "1",
            "--",
            "/bin/sh",
            "-c",
            "printf human-out; printf human-err >&2",
        ],
    );
    assert_success(&human);
    assert_eq!(human.stdout, b"human-out");
    assert_eq!(human.stderr, b"human-err");

    let failed = fixture.vw(
        &fixture.repo,
        &[
            "exec",
            "topic",
            "--json",
            "--lock-timeout-ms",
            "1",
            "--",
            "/bin/sh",
            "-c",
            "printf partial; printf failed >&2; exit 7",
        ],
    );
    let failed = assert_error(&failed, "CHILD_PROCESS_FAILED", 21);
    assert_eq!(failed["data"]["childExitCode"], 7);
    assert_eq!(failed["data"]["childStdout"], "partial");
    assert_eq!(failed["data"]["childStderr"], "failed");

    let missing = fixture.vw(
        &fixture.repo,
        &[
            "exec",
            "absent",
            "--json",
            "--lock-timeout-ms",
            "1",
            "--",
            "/usr/bin/true",
        ],
    );
    assert_error(&missing, "WORKTREE_NOT_FOUND", 4);
}

#[test]
fn exec_rejects_non_utf8_target_before_starting_the_child() {
    let fixture = Fixture::new();

    let wrapper_directory = fixture.root.path().join("non-utf8-git-wrapper");
    fs::create_dir(&wrapper_directory).expect("git wrapper directory");
    let wrapper = wrapper_directory.join("git");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
if [ "$1" = worktree ] && [ "$2" = list ] && [ "$3" = --porcelain ] && [ "$4" = -z ]; then
  printf 'worktree %s\0HEAD abc\0branch refs/heads/main\0\0' "$FAKE_REPO_ROOT"
  printf 'worktree %s/non-utf8-\377\0HEAD def\0branch refs/heads/non-utf8\0\0' "$FAKE_REPO_ROOT"
  exit 0
fi
exec "$REAL_GIT" "$@"
"#,
    )
    .expect("write git wrapper");
    let mut wrapper_permissions = fs::metadata(&wrapper)
        .expect("git wrapper metadata")
        .permissions();
    wrapper_permissions.set_mode(0o755);
    fs::set_permissions(&wrapper, wrapper_permissions).expect("make git wrapper executable");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("resolve real git");
    assert!(
        real_git.status.success(),
        "resolve real git failed: {}",
        String::from_utf8_lossy(&real_git.stderr)
    );
    let real_git = String::from_utf8(real_git.stdout).expect("real git path is UTF-8");
    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let wrapper_path = std::env::join_paths(
        std::iter::once(wrapper_directory).chain(std::env::split_paths(&original_path)),
    )
    .expect("git wrapper PATH");
    let child_marker = fixture.root.path().join("non-utf8-child-ran");
    let rejected = fixture
        .command(&fixture.repo)
        .env("PATH", wrapper_path)
        .env("REAL_GIT", real_git.trim())
        .env("FAKE_REPO_ROOT", &fixture.repo)
        .args([
            "exec",
            "non-utf8",
            "--json",
            "--",
            "/usr/bin/touch",
            utf8(&child_marker),
        ])
        .output()
        .expect("run exec with non-UTF-8 target metadata");
    assert_error(&rejected, "UNSUPPORTED_REPOSITORY_LAYOUT", 4);
    assert!(
        !child_marker.exists(),
        "exec child must not run for a non-UTF-8 target"
    );
}

#[test]
fn a022_invoke_passes_argv_cwd_and_wt_environment_and_types_hook_failures() {
    let fixture = Fixture::new();
    let observation = fixture.repo.join("invoke-observation");
    write_hook(
        &fixture.repo,
        "post-acceptance",
        r#"#!/bin/sh
set -eu
{
  printf 'arg1=%s\n' "$1"
  printf 'arg2=%s\n' "$2"
  printf 'repo=%s\n' "$WT_REPO_ROOT"
  printf 'action=%s\n' "$WT_ACTION"
  printf 'branch=%s\n' "$WT_BRANCH"
  printf 'worktree=%s\n' "$WT_WORKTREE_PATH"
  printf 'tty=%s\n' "$WT_IS_TTY"
  printf 'tool=%s\n' "$WT_TOOL"
  printf 'cwd=%s\n' "$PWD"
} > "$WT_REPO_ROOT/invoke-observation"
printf hook-stdout
printf hook-stderr >&2
"#,
        true,
    );
    let invoked = fixture.vw(
        &fixture.repo,
        &[
            "invoke",
            "post-acceptance",
            "--json",
            "--hook-timeout-ms",
            "1000",
            "--",
            "a b",
            "$HOME",
        ],
    );
    assert_success(&invoked);
    assert!(invoked.stderr.is_empty());
    let invoked = json(&invoked);
    assert_eq!(invoked["data"]["hook"], "post-acceptance");
    let expected = format!(
        "arg1=a b\narg2=$HOME\nrepo={}\naction=invoke:post-acceptance\nbranch=main\nworktree={}\ntty=0\ntool=vde-worktree\ncwd={}\n",
        fixture.repo.display(),
        fixture.repo.display(),
        fixture.repo.display()
    );
    assert_eq!(
        fs::read_to_string(observation).expect("hook observation"),
        expected
    );

    let missing = fixture.vw(&fixture.repo, &["invoke", "post-missing", "--json"]);
    assert_error(&missing, "HOOK_NOT_FOUND", 4);

    write_hook(&fixture.repo, "post-nonexec", "#!/bin/sh\nexit 0\n", false);
    let non_executable = fixture.vw(&fixture.repo, &["invoke", "post-nonexec", "--json"]);
    assert_error(&non_executable, "HOOK_NOT_EXECUTABLE", 10);

    write_hook(
        &fixture.repo,
        "post-timeout",
        "#!/bin/sh\n/bin/sleep 30\n",
        true,
    );
    fs::write(
        fixture.repo.join(".vde/worktree/config.yml"),
        "hooks:\n  timeoutMs: 40\n",
    )
    .expect("write hook timeout config");
    let timed_out = fixture.vw(&fixture.repo, &["invoke", "post-timeout", "--json"]);
    assert_error(&timed_out, "HOOK_TIMEOUT", 10);
}

#[test]
#[allow(clippy::too_many_lines)]
fn a023_copy_and_link_reject_unsafe_sources_without_changing_targets() {
    let fixture = Fixture::new();
    let target = fixture.switch("placement");
    let _lock = acquire_repo_lock(
        &fixture.repo.join(".git"),
        Duration::from_secs(1),
        "phase5-placement-acceptance",
    )
    .expect("hold repository lock");

    fs::write(fixture.repo.join("self-placement"), "original\n").expect("self source");
    for command in ["copy", "link"] {
        let output = fixture
            .command(&fixture.repo)
            .env_remove("WT_WORKTREE_PATH")
            .args([
                command,
                "self-placement",
                "--json",
                "--lock-timeout-ms",
                "1",
            ])
            .output()
            .expect("reject placement onto the primary source itself");
        assert_error(&output, "INVALID_ARGUMENT", 3);
        assert_eq!(
            fs::read_to_string(fixture.repo.join("self-placement")).expect("unchanged self source"),
            "original\n"
        );
        assert!(
            fs::read_dir(&fixture.repo)
                .expect("repository entries")
                .all(|entry| {
                    !entry
                        .expect("repository entry")
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".vde-placement-")
                })
        );
    }

    fs::create_dir_all(fixture.repo.join("config")).expect("source directory");
    fs::write(fixture.repo.join("config/value"), "copied\n").expect("source file");
    fs::write(fixture.repo.join("linked-file"), "linked\n").expect("link source");
    let copied = fixture
        .command(&fixture.repo)
        .env("WT_WORKTREE_PATH", &target)
        .args(["copy", "config", "--json", "--lock-timeout-ms", "1"])
        .output()
        .expect("copy into target");
    assert_success(&copied);
    assert_eq!(
        fs::read_to_string(target.join("config/value")).expect("copied file"),
        "copied\n"
    );
    let linked = fixture
        .command(&fixture.repo)
        .env("WT_WORKTREE_PATH", &target)
        .args(["link", "linked-file", "--json", "--lock-timeout-ms", "1"])
        .output()
        .expect("link into target");
    assert_success(&linked);
    let link_target = fs::read_link(target.join("linked-file")).expect("relative link");
    assert!(link_target.is_relative());
    assert_eq!(
        fs::canonicalize(target.join("linked-file")).expect("resolved link"),
        fs::canonicalize(fixture.repo.join("linked-file")).expect("resolved source")
    );

    fs::create_dir_all(fixture.repo.join("shared")).expect("shared source parent");
    fs::write(fixture.repo.join("shared/x"), "x\n").expect("shared x source");
    fs::write(fixture.repo.join("shared/y"), "y\n").expect("shared y source");
    let shared = fixture
        .command(&fixture.repo)
        .env("WT_WORKTREE_PATH", &target)
        .args([
            "copy",
            "shared/x",
            "shared/y",
            "--json",
            "--lock-timeout-ms",
            "1",
        ])
        .output()
        .expect("copy batch into one newly-created parent");
    assert_success(&shared);
    assert_eq!(
        fs::read_to_string(target.join("shared/x")).expect("shared x target"),
        "x\n"
    );
    assert_eq!(
        fs::read_to_string(target.join("shared/y")).expect("shared y target"),
        "y\n"
    );

    for (command, path, code) in [
        ("copy", "missing", "PATH_OUTSIDE_REPO"),
        ("link", "missing", "PATH_OUTSIDE_REPO"),
        ("copy", "../outside", "PATH_OUTSIDE_REPO"),
        ("link", "../outside", "PATH_OUTSIDE_REPO"),
    ] {
        let before = tree_snapshot(&target);
        let output = fixture
            .command(&fixture.repo)
            .env("WT_WORKTREE_PATH", &target)
            .args([command, path, "--json", "--lock-timeout-ms", "1"])
            .output()
            .expect("run rejected placement");
        assert_error(&output, code, 4);
        assert_eq!(tree_snapshot(&target), before, "{command} {path}");
    }

    for command in ["copy", "link"] {
        let before = tree_snapshot(&target);
        let output = fixture
            .command(&fixture.repo)
            .env("WT_WORKTREE_PATH", &target)
            .arg(command)
            .arg(utf8(&fixture.repo.join("linked-file")))
            .args(["--json", "--lock-timeout-ms", "1"])
            .output()
            .expect("run rejected absolute placement");
        assert_error(&output, "ABSOLUTE_PATH_NOT_ALLOWED", 4);
        assert_eq!(tree_snapshot(&target), before, "{command} absolute");
    }

    fs::create_dir_all(fixture.repo.join("escaped")).expect("escaped source");
    fs::write(fixture.repo.join("escaped/value"), "safe source\n").expect("escaped value");
    let outside = fixture.root.path().join("outside");
    fs::create_dir(&outside).expect("outside directory");
    symlink(&outside, target.join("escaped")).expect("escaping destination ancestor");
    for command in ["copy", "link"] {
        let before = tree_snapshot(&target);
        let output = fixture
            .command(&fixture.repo)
            .env("WT_WORKTREE_PATH", &target)
            .args([command, "escaped/value", "--json", "--lock-timeout-ms", "1"])
            .output()
            .expect("run symlink escape rejection");
        assert_error(&output, "PATH_OUTSIDE_REPO", 4);
        assert_eq!(tree_snapshot(&target), before, "{command} symlink escape");
        assert!(
            fs::read_dir(&outside)
                .expect("outside read")
                .next()
                .is_none()
        );
    }

    fs::write(target.join("special-source"), "original target\n").expect("existing target");
    let socket_path = fixture.repo.join("special-source");
    let _socket = UnixListener::bind(&socket_path).expect("source Unix socket");
    let before = tree_snapshot(&target);
    let failed_copy = fixture
        .command(&fixture.repo)
        .env("WT_WORKTREE_PATH", &target)
        .args(["copy", "special-source", "--json", "--lock-timeout-ms", "1"])
        .output()
        .expect("copy unsupported source");
    assert_error(&failed_copy, "INVALID_ARGUMENT", 3);
    assert_eq!(tree_snapshot(&target), before);

    fs::write(fixture.repo.join("batch-first"), "new first\n").expect("first batch source");
    let batch_socket_path = fixture.repo.join("batch-second");
    let _batch_socket = UnixListener::bind(&batch_socket_path).expect("second batch source");
    fs::write(target.join("batch-first"), "old first\n").expect("first batch target");
    fs::write(target.join("batch-second"), "old second\n").expect("second batch target");
    let before_batch = tree_snapshot(&target);
    let failed_batch = fixture
        .command(&fixture.repo)
        .env("WT_WORKTREE_PATH", &target)
        .args([
            "copy",
            "batch-first",
            "batch-second",
            "--json",
            "--lock-timeout-ms",
            "1",
        ])
        .output()
        .expect("copy batch with a later unsupported source");
    assert_error(&failed_batch, "INVALID_ARGUMENT", 3);
    assert_eq!(tree_snapshot(&target), before_batch);
    assert!(fs::read_dir(&target).expect("target entries").all(|entry| {
        !entry
            .expect("target entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".vde-placement-")
    }));
}

fn clean_completion_command(cwd: &Path, home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vw"));
    command
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

fn assert_node_free(script: &str) {
    let lowercase = script.to_ascii_lowercase();
    for runtime in ["node", "npm", "pnpm"] {
        assert!(
            !lowercase.contains(runtime),
            "found {runtime} in completion"
        );
    }
    assert!(script.contains("__complete"));
}

#[test]
#[allow(clippy::too_many_lines)]
fn a024_completion_is_repo_independent_json_installable_and_node_free() {
    let root = tempfile::tempdir().expect("completion fixture");
    let outside_repo = root.path().join("outside-repository");
    let home = root.path().join("home");
    fs::create_dir_all(&outside_repo).expect("outside repository");

    for shell in ["zsh", "fish"] {
        let output = clean_completion_command(&outside_repo, &home)
            .args(["completion", shell, "--json"])
            .output()
            .expect("generate completion in clean environment");
        assert_success(&output);
        let value = json(&output);
        assert_eq!(value["schemaVersion"], 3);
        assert_eq!(value["command"], "completion");
        assert_eq!(value["repoRoot"], Value::Null);
        assert_eq!(value["data"]["shell"], shell);
        assert_eq!(value["data"]["installed"], false);
        let script = value["data"]["script"].as_str().expect("completion script");
        assert_node_free(script);
        if shell == "zsh" {
            assert!(
                script.contains(
                    "_vw_complete_use_branches() { _vw_dynamic_candidates use-branches }"
                )
            );
            assert!(script.lines().any(|line| line.starts_with("':branch -- ")
                && line.contains(":_vw_complete_use_branches'")));
        }
    }

    let custom = root.path().join("custom/_vw");
    fs::create_dir_all(custom.parent().expect("custom parent")).expect("custom directory");
    fs::write(&custom, "old completion\n").expect("old custom completion");
    let installed_custom = clean_completion_command(&outside_repo, &home)
        .args(["completion", "zsh", "--install", "--path"])
        .arg(&custom)
        .arg("--json")
        .output()
        .expect("custom completion install");
    assert_success(&installed_custom);
    let installed_custom = json(&installed_custom);
    assert_eq!(installed_custom["data"]["path"], utf8(&custom));
    assert_node_free(&fs::read_to_string(&custom).expect("custom completion"));
    assert!(
        fs::read_dir(custom.parent().expect("custom parent"))
            .expect("custom entries")
            .all(|entry| !entry
                .expect("custom entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".vde-completion-"))
    );

    for (shell, expected) in [
        ("zsh", home.join(".zsh/completions/_vw")),
        ("fish", home.join(".config/fish/completions/vw.fish")),
    ] {
        let output = clean_completion_command(&outside_repo, &home)
            .args(["completion", shell, "--install", "--json"])
            .output()
            .expect("default completion install");
        assert_success(&output);
        let value = json(&output);
        assert_eq!(value["repoRoot"], Value::Null);
        assert_eq!(value["data"]["path"], utf8(&expected));
        assert_node_free(&fs::read_to_string(expected).expect("default completion"));
    }

    let unsupported = clean_completion_command(&outside_repo, &home)
        .args(["completion", "bash", "--json"])
        .output()
        .expect("unsupported shell parse");
    let unsupported = assert_error(&unsupported, "INVALID_ARGUMENT", 3);
    assert_eq!(unsupported["command"], "completion");
    assert_eq!(unsupported["repoRoot"], Value::Null);
    assert_eq!(unsupported["data"], Value::Null);
}

#[test]
fn describe_and_help_cover_public_commands_without_repository_or_valid_config() {
    let directory = tempfile::tempdir().unwrap();
    let config_dir = directory.path().join(".vde/worktree");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yml"), "[invalid: yaml").unwrap();
    for binary in [env!("CARGO_BIN_EXE_vw"), env!("CARGO_BIN_EXE_vde-worktree")] {
        let output = Command::new(binary)
            .current_dir(directory.path())
            .args(["describe", "--json"])
            .output()
            .unwrap();
        assert_success(&output);
        let value = json(&output);
        assert!(value["repoRoot"].is_null());
        let names = value["data"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| command["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, vde_worktree::cli::COMMAND_NAMES);
        for command in names {
            let help = Command::new(binary)
                .current_dir(directory.path())
                .args([command, "--help"])
                .output()
                .unwrap();
            assert_success(&help);
            let text = String::from_utf8(help.stdout).unwrap();
            assert!(text.contains("Examples:"), "{command}");
            assert!(text.contains("Prerequisites:"), "{command}");
            assert!(text.contains("Effects:"), "{command}");
        }
        let selected = Command::new(binary)
            .current_dir(directory.path())
            .args(["describe", "exec", "--json"])
            .output()
            .unwrap();
        assert_success(&selected);
        let selected = json(&selected);
        assert_eq!(selected["data"]["commands"].as_array().unwrap().len(), 1);
        assert_eq!(selected["data"]["commands"][0]["name"], "exec");
        let missing = Command::new(binary)
            .current_dir(directory.path())
            .args(["describe", "unknown", "--json"])
            .output()
            .unwrap();
        assert_error(&missing, "UNKNOWN_COMMAND", 3);
    }
}
