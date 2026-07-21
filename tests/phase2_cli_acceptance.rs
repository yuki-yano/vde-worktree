#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;
use vde_worktree::adapters::fzf::{FzfAdapter, FzfError, FzfRequest, FzfSelection};
use vde_worktree::adapters::process::StdProcessRunner;
use vde_worktree::app::dispatch::{
    ApplicationBackend, ApplicationHookResult, CommandOutput, dispatch,
};
use vde_worktree::app::result::TerminalCapabilities;
use vde_worktree::cli::{CliParseResult, ParsedRequest, parse_from};
use vde_worktree::domain::error::{CliError, ErrorCode};
use vde_worktree::domain::repo::RepoContext;
use vde_worktree::ports::process::{ProcessCommand, ProcessError, ProcessOutput, ProcessRunner};
use vde_worktree::presentation::picker::PickerCandidate;
use vde_worktree::presentation::table::{
    ListTableRow, MergedCellState, PrCellState, TableRenderOptions, render_table,
};
use vde_worktree::presentation::theme::ColorPolicy;
use vde_worktree::state::config::{ListPathTruncate, ListTableColumn, SelectorCdSurface};
use vde_worktree::state::hooks::{HookContext, HookPhase, MutationHookContexts};
use vde_worktree::state::json_store::JsonRecordState;
use vde_worktree::state::lifecycle::{
    lifecycle_file_path, lifecycle_observation_lock_path, read_worktree_lifecycle,
};

fn repository() -> (TempDir, PathBuf) {
    let fixture = tempfile::tempdir().expect("fixture directory");
    let repo = fixture.path().join("repo");
    git(
        fixture.path(),
        ["init", "--quiet", "-b", "main", repo.to_str().unwrap()],
    );
    git(&repo, ["config", "user.email", "test@example.com"]);
    git(&repo, ["config", "user.name", "Test"]);
    fs::write(repo.join("README.md"), "initial\n").expect("initial file");
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "--quiet", "-m", "initial"]);
    let repo = fs::canonicalize(repo).expect("canonical repository");
    (fixture, repo)
}

fn git<const N: usize>(cwd: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_vw(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vw"))
        .current_dir(repo)
        .args(args)
        .env("HOME", repo.join("isolated-home"))
        .env("XDG_CONFIG_HOME", repo.join("isolated-config"))
        .env("GIT_CONFIG_GLOBAL", repo.join("isolated-gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("NO_COLOR")
        .output()
        .expect("run vw")
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("one JSON stdout object")
}

#[test]
fn a005_status_and_path_missing_are_typed_in_human_and_json_modes() {
    let (_fixture, repo) = repository();
    for command in ["status", "path"] {
        let human = run_vw(&repo, &[command, "missing", "--no-gh"]);
        assert_eq!(human.status.code(), Some(4), "{command}");
        assert!(human.stdout.is_empty(), "{command}");
        assert!(
            String::from_utf8_lossy(&human.stderr).contains("[WORKTREE_NOT_FOUND]"),
            "{command}"
        );

        let json = run_vw(&repo, &[command, "missing", "--json", "--no-gh"]);
        let value = json_stdout(&json);
        assert_eq!(json.status.code(), Some(4), "{command}");
        assert_eq!(value["error"]["code"], "WORKTREE_NOT_FOUND", "{command}");
        assert_eq!(value["error"]["details"]["branch"], "missing");
    }
}

#[test]
fn a006_a007_list_keeps_semantic_color_after_reordering_and_machine_output_is_plain() {
    let row = ListTableRow {
        branch: Some("feature/日本語".to_owned()),
        current: true,
        dirty: true,
        merged: MergedCellState::Unmerged,
        pr: PrCellState::Open,
        locked: false,
        ahead: Some(2),
        behind: Some(1),
        path: "~/repo/長い名前".to_owned(),
    };
    let columns = vec![
        ListTableColumn::Path,
        ListTableColumn::Dirty,
        ListTableColumn::Behind,
        ListTableColumn::Branch,
        ListTableColumn::Pr,
        ListTableColumn::Merged,
        ListTableColumn::Ahead,
        ListTableColumn::Locked,
    ];
    let colored = render_table(
        std::slice::from_ref(&row),
        &TableRenderOptions {
            columns: columns.clone(),
            terminal_width: Some(100),
            path_truncate: ListPathTruncate::Auto,
            path_min_width: 12,
            full_path: false,
            color: ColorPolicy {
                stream_is_terminal: true,
                json: false,
                no_color: false,
            },
        },
    );
    assert!(colored.styled.contains("\u{1b}[38;2;116;199;236m"));
    assert!(colored.styled.contains("\u{1b}[38;2;250;179;135m"));
    assert!(colored.styled.contains("\u{1b}[38;2;203;166;247m"));
    assert!(colored.styled.contains("\u{1b}[38;2;243;139;168m"));
    assert!(colored.styled.contains("\u{1b}[38;2;249;226;175m"));
    assert!(colored.styled.contains("\u{1b}[38;2;205;214;244m"));

    for policy in [
        ColorPolicy {
            stream_is_terminal: false,
            json: false,
            no_color: false,
        },
        ColorPolicy {
            stream_is_terminal: true,
            json: false,
            no_color: true,
        },
        ColorPolicy {
            stream_is_terminal: true,
            json: true,
            no_color: false,
        },
    ] {
        let rendered = render_table(
            std::slice::from_ref(&row),
            &TableRenderOptions {
                columns: columns.clone(),
                terminal_width: Some(100),
                path_truncate: ListPathTruncate::Auto,
                path_min_width: 12,
                full_path: false,
                color: policy,
            },
        );
        assert!(!rendered.styled.contains('\u{1b}'));
    }

    let (_fixture, repo) = repository();
    let human = run_vw(&repo, &["list", "--no-gh"]);
    assert!(human.status.success());
    assert!(!human.stdout.contains(&0x1b));
    let narrow_non_tty = Command::new(env!("CARGO_BIN_EXE_vw"))
        .current_dir(&repo)
        .args(["list", "--no-gh"])
        .env("HOME", repo.join("isolated-home"))
        .env("XDG_CONFIG_HOME", repo.join("isolated-config"))
        .env("GIT_CONFIG_GLOBAL", repo.join("isolated-gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("COLUMNS", "20")
        .output()
        .expect("run non-TTY list with COLUMNS");
    assert!(narrow_non_tty.status.success());
    assert!(!String::from_utf8_lossy(&narrow_non_tty.stdout).contains('…'));
    let json = run_vw(&repo, &["list", "--json", "--no-gh"]);
    assert!(json.status.success());
    assert!(!json.stdout.contains(&0x1b));
    assert_eq!(json_stdout(&json)["data"]["baseBranch"], "main");
}

fn run_list_on_pty(repo: &Path, no_color: bool) -> Vec<u8> {
    use nix::pty::{Winsize, openpty};

    let pty = openpty(
        Some(&Winsize {
            ws_row: 40,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )
    .expect("open PTY");
    let slave = fs::File::from(pty.slave);
    let mut command = Command::new(env!("CARGO_BIN_EXE_vw"));
    command
        .current_dir(repo)
        .args(["list", "--no-gh"])
        .env("HOME", repo.join("isolated-home"))
        .env("XDG_CONFIG_HOME", repo.join("isolated-config"))
        .env("GIT_CONFIG_GLOBAL", repo.join("isolated-gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1");
    if no_color {
        command.env("NO_COLOR", "1");
    } else {
        command.env_remove("NO_COLOR");
    }
    let mut child = command
        .stdin(Stdio::from(slave.try_clone().expect("clone PTY slave")))
        .stdout(Stdio::from(slave.try_clone().expect("clone PTY slave")))
        .stderr(Stdio::from(slave.try_clone().expect("clone PTY slave")))
        .spawn()
        .expect("spawn vw on PTY");
    // Command retains its configured Stdio handles after spawn. On macOS those
    // parent-side slave handles prevent EOF from reaching the PTY master.
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
                // Linux PTYs commonly report EIO after the slave has closed.
                Err(error) if error.raw_os_error() == Some(5) => break,
                Err(error) => panic!("read PTY output: {error}"),
            }
        }
        output
    });
    let status = child.wait().expect("wait for vw");
    let output = reader.join().expect("join PTY reader");
    assert!(
        status.success(),
        "PTY output: {}",
        String::from_utf8_lossy(&output)
    );
    output
}

#[test]
fn a006_production_list_emits_truecolor_on_a_real_pty() {
    let (_fixture, repo) = repository();
    let output = run_list_on_pty(&repo, false);
    assert!(
        output
            .windows(b"\x1b[38;2;".len())
            .any(|window| window == b"\x1b[38;2;"),
        "PTY output did not contain truecolor ANSI: {:?}",
        String::from_utf8_lossy(&output)
    );
    assert!(String::from_utf8_lossy(&output).contains("branch"));
}

#[test]
fn a007_no_color_removes_ansi_even_on_a_real_pty() {
    let (_fixture, repo) = repository();
    let output = run_list_on_pty(&repo, true);

    assert!(!output.contains(&0x1b));
    assert!(String::from_utf8_lossy(&output).contains("branch"));
}

#[test]
fn production_status_and_path_keep_human_and_json_path_contracts() {
    let (_fixture, repo) = repository();
    let path = run_vw(&repo, &["path", "main", "--no-gh"]);
    assert!(path.status.success());
    assert_eq!(
        String::from_utf8(path.stdout).unwrap(),
        format!("{}\n", repo.display())
    );

    let status = run_vw(&repo, &["status", "--json", "--no-gh"]);
    let value = json_stdout(&status);
    assert!(status.status.success());
    assert_eq!(
        value["data"]["worktree"]["path"],
        repo.to_string_lossy().as_ref()
    );
    assert_eq!(value["data"]["worktree"]["pr"]["status"], Value::Null);
}

#[test]
fn snapshot_warnings_use_stderr_without_corrupting_json_or_metadata() {
    let (_fixture, repo) = repository();
    let lifecycle = lifecycle_file_path(&repo, "main");
    fs::create_dir_all(lifecycle.parent().unwrap()).expect("state directory");
    fs::write(&lifecycle, b"{invalid-json}\n").expect("invalid lifecycle");
    let before = fs::read(&lifecycle).unwrap();

    for args in [&["list", "--no-gh"][..], &["list", "--json", "--no-gh"][..]] {
        let output = run_vw(&repo, args);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("[INVALID_LIFECYCLE]"));
        if args.contains(&"--json") {
            let value = json_stdout(&output);
            assert_eq!(value["status"], "ok");
            assert!(value["data"].get("warnings").is_none());
        }
        assert_eq!(fs::read(&lifecycle).unwrap(), before);
    }
}

#[test]
fn lifecycle_observation_failure_warns_on_stderr_without_corrupting_json() {
    let (_fixture, repo) = repository();
    let observation_lock = lifecycle_observation_lock_path(&repo);
    fs::create_dir_all(&observation_lock).expect("blocking observation lock directory");

    let output = run_vw(&repo, &["list", "--json", "--no-gh"]);
    let value = json_stdout(&output);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("[LIFECYCLE_OBSERVATION_FAILED]"));
    assert_eq!(value["status"], "ok");
    assert!(value["data"].get("warnings").is_none());
    assert!(!lifecycle_file_path(&repo, "main").exists());
}

#[test]
fn persisted_divergence_survives_branch_reset_and_reflog_expiry() {
    let (fixture, repo) = repository();
    let feature_path = fixture.path().join("feature-observed");
    git(
        &repo,
        [
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature/observed",
            feature_path.to_str().unwrap(),
        ],
    );
    fs::write(feature_path.join("feature.txt"), "feature\n").expect("feature file");
    git(&feature_path, ["add", "feature.txt"]);
    git(
        &feature_path,
        ["commit", "--quiet", "-m", "feature observation"],
    );
    fs::create_dir_all(repo.join(".vde/worktree/state")).expect("state directory");

    let before_merge = run_vw(&repo, &["list", "--json", "--no-gh"]);
    assert!(before_merge.status.success());
    assert!(before_merge.stderr.is_empty());
    let before_merge = json_stdout(&before_merge);
    let observed_head = before_merge["data"]["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worktree| worktree["branch"] == "feature/observed")
        .and_then(|worktree| worktree["head"].as_str())
        .expect("observed feature head")
        .to_owned();
    let JsonRecordState::Valid(record) = read_worktree_lifecycle(&repo, "feature/observed").state
    else {
        panic!("lifecycle observation was not persisted");
    };
    assert_eq!(
        record.last_diverged_head.as_deref(),
        Some(observed_head.as_str())
    );

    git(
        &repo,
        [
            "merge",
            "--quiet",
            "--no-ff",
            "feature/observed",
            "-m",
            "merge feature",
        ],
    );
    git(&feature_path, ["reset", "--quiet", "--hard", "main"]);
    git(&repo, ["reflog", "expire", "--expire=now", "--all"]);

    let after_reset = run_vw(&repo, &["list", "--json", "--no-gh"]);
    assert!(after_reset.status.success());
    assert!(after_reset.stderr.is_empty());
    let after_reset = json_stdout(&after_reset);
    let feature = after_reset["data"]["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worktree| worktree["branch"] == "feature/observed")
        .expect("feature snapshot");
    assert_eq!(feature["merged"]["overall"], true);
}

#[test]
fn list_ahead_behind_is_measured_against_base_not_upstream() {
    let (fixture, repo) = repository();
    let feature = fixture.path().join("feature");
    git(
        &repo,
        [
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature/a",
            feature.to_str().unwrap(),
        ],
    );
    fs::write(feature.join("feature.txt"), "feature\n").expect("feature file");
    git(&feature, ["add", "feature.txt"]);
    git(&feature, ["commit", "--quiet", "-m", "feature"]);
    let config_directory = repo.join(".vde/worktree");
    fs::create_dir_all(&config_directory).expect("config directory");
    fs::write(
        config_directory.join("config.yml"),
        "list:\n  table:\n    columns: [branch, ahead, behind]\n",
    )
    .expect("list config");

    let human = run_vw(&repo, &["list", "--no-gh"]);
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    let feature_line = human
        .lines()
        .find(|line| line.contains("feature/a"))
        .unwrap();
    assert!(
        feature_line.contains("│ 1     │ 0      │"),
        "{feature_line}"
    );

    let json = run_vw(&repo, &["list", "--json", "--no-gh"]);
    let value = json_stdout(&json);
    let feature = value["data"]["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worktree| worktree["branch"] == "feature/a")
        .unwrap();
    assert!(feature["upstream"]["remote"].is_null());
    assert!(feature["upstream"]["ahead"].is_null());
}

#[derive(Debug)]
struct ExecutableRunner {
    executable: PathBuf,
}

impl ProcessRunner for ExecutableRunner {
    fn run(&self, command: &ProcessCommand) -> Result<ProcessOutput, ProcessError> {
        let mut command = command.clone();
        assert_eq!(command.program, OsString::from("fzf"));
        command.program = self.executable.clone().into_os_string();
        StdProcessRunner.run(&command)
    }
}

fn fake_fzf(script: &str) -> (TempDir, FzfAdapter<ExecutableRunner>) {
    let fixture = tempfile::tempdir().expect("fake fzf directory");
    let executable = fixture.path().join("fzf");
    fs::write(&executable, script).expect("fake fzf script");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).expect("executable fzf");
    let adapter = FzfAdapter::new(ExecutableRunner { executable });
    (fixture, adapter)
}

fn fzf_request(candidates: &[PickerCandidate]) -> FzfRequest<'_> {
    FzfRequest {
        candidates,
        cwd: Path::new("/"),
        prompt: "worktree> ",
        surface: SelectorCdSurface::Inline,
        tmux_popup_opts: "80%,70%",
        extra_args: &[],
        stderr_is_terminal: true,
        in_tmux: false,
    }
}

#[test]
fn a008_fake_fzf_covers_zero_inline_cancel_and_popup_unsupported() {
    let no_candidates = FzfAdapter::new(ExecutableRunner {
        executable: PathBuf::from("/not-used"),
    });
    assert!(matches!(
        no_candidates.select_path(&fzf_request(&[])),
        Err(FzfError::NoCandidates)
    ));

    let candidates = [PickerCandidate {
        line: "* main\t/repo\tpreview".to_owned(),
        path: PathBuf::from("/repo"),
    }];
    let (_fixture, inline) = fake_fzf(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 0.60.0; exit 0; fi\ncat >/dev/null\nprintf '* main\\t/repo\\tpreview\\n'\n",
    );
    assert_eq!(
        inline.select_path(&fzf_request(&candidates)).unwrap(),
        FzfSelection::Selected(PathBuf::from("/repo"))
    );

    let (_fixture, cancel) = fake_fzf(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 0.60.0; exit 0; fi\ncat >/dev/null\nexit 130\n",
    );
    assert_eq!(
        cancel.select_path(&fzf_request(&candidates)).unwrap(),
        FzfSelection::Cancelled
    );

    let (_fixture, popup) = fake_fzf(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 0.50.0; exit 0; fi\nif [ \"$1\" = \"--help\" ]; then echo usage; exit 0; fi\nexit 2\n",
    );
    let mut request = fzf_request(&candidates);
    request.surface = SelectorCdSurface::TmuxPopup;
    request.in_tmux = true;
    assert!(matches!(
        popup.select_path(&request),
        Err(FzfError::TmuxPopupUnsupported)
    ));
}

#[test]
fn a008_production_cd_supports_stdout_pipe_with_stderr_pty() {
    use nix::pty::{Winsize, openpty};

    let (fixture, repo) = repository();
    let fake_bin = fixture.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("fake bin directory");
    let fzf = fake_bin.join("fzf");
    fs::write(
        &fzf,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 0.60.0; exit 0; fi\nIFS= read -r selected\nprintf '%s\\n' \"$selected\"\n",
    )
    .expect("fake fzf");
    fs::set_permissions(&fzf, fs::Permissions::from_mode(0o755)).expect("executable fake fzf");
    let path = std::env::join_paths(
        std::iter::once(fake_bin.as_os_str()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(PathBuf::into_os_string)
                .collect::<Vec<_>>()
                .iter()
                .map(OsString::as_os_str),
        ),
    )
    .expect("joined PATH");

    let pty = openpty(
        Some(&Winsize {
            ws_row: 40,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )
    .expect("open stderr PTY");
    let slave = fs::File::from(pty.slave);
    let master = fs::File::from(pty.master);
    let stderr_reader = std::thread::spawn(move || {
        let mut master = master;
        let mut output = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.raw_os_error() == Some(5) => break,
                Err(error) => panic!("read stderr PTY: {error}"),
            }
        }
        output
    });
    let child = Command::new(env!("CARGO_BIN_EXE_vw"))
        .current_dir(&repo)
        .arg("cd")
        .env("PATH", path)
        .env("HOME", repo.join("isolated-home"))
        .env("XDG_CONFIG_HOME", repo.join("isolated-config"))
        .env("GIT_CONFIG_GLOBAL", repo.join("isolated-gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(slave.try_clone().expect("clone stderr PTY")))
        .spawn()
        .expect("spawn production cd");
    drop(slave);
    let output = child.wait_with_output().expect("wait for production cd");
    let picker_stderr = stderr_reader.join().expect("join stderr reader");

    assert!(
        output.status.success(),
        "cd stderr: {}",
        String::from_utf8_lossy(&picker_stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 path output"),
        format!("{}\n", repo.display())
    );
}

#[derive(Debug)]
struct ReadBackend {
    result: Result<CommandOutput, CliError>,
}

impl ApplicationBackend for ReadBackend {
    type LockGuard = ();
    type MutationPlan = MutationHookContexts;
    type MutationStage = ();
    type MutationResult = ();

    fn resolve_repo_context(&self) -> Result<RepoContext, CliError> {
        Ok(RepoContext {
            repo_root: PathBuf::from("/repo"),
            current_worktree_root: PathBuf::from("/repo"),
            git_common_dir: PathBuf::from("/repo/.git"),
        })
    }

    fn resolve_config(
        &self,
        _context: &RepoContext,
    ) -> Result<vde_worktree::state::config::ResolvedConfig, CliError> {
        Ok(vde_worktree::state::config::ResolvedConfig::default())
    }

    fn is_initialized(&self, _repo_root: &Path) -> Result<bool, CliError> {
        Ok(true)
    }

    fn acquire_repo_lock(
        &self,
        _context: &RepoContext,
        _timeout: Duration,
        _command: &str,
    ) -> Result<Self::LockGuard, CliError> {
        Ok(())
    }

    fn run_hook(
        &self,
        _phase: HookPhase,
        _request: &ParsedRequest,
        _context: &HookContext,
        _timeout: Duration,
    ) -> Result<ApplicationHookResult, CliError> {
        Ok(ApplicationHookResult::Continue)
    }

    fn prepare_mutation(
        &self,
        _request: &ParsedRequest,
        _context: &RepoContext,
    ) -> Result<Self::MutationPlan, CliError> {
        unreachable!("read backend never prepares mutations")
    }

    fn apply_mutation(
        &self,
        _request: &ParsedRequest,
        _context: &RepoContext,
        _plan: &Self::MutationPlan,
        _stage: Self::MutationStage,
    ) -> Result<Self::MutationResult, CliError> {
        unreachable!("read backend never applies mutations")
    }

    fn stage_mutation(
        &self,
        _request: &ParsedRequest,
        _context: &RepoContext,
        _plan: &Self::MutationPlan,
    ) -> Result<Self::MutationStage, CliError> {
        unreachable!("read backend never stages mutations")
    }

    fn rollback_mutation_stage(
        &self,
        _request: &ParsedRequest,
        _context: &RepoContext,
        _plan: &Self::MutationPlan,
        _stage: &Self::MutationStage,
    ) -> Result<(), CliError> {
        unreachable!("read backend never rolls back mutations")
    }

    fn update_mutation_state(
        &self,
        _request: &ParsedRequest,
        _context: &RepoContext,
        _plan: &Self::MutationPlan,
        _result: Self::MutationResult,
    ) -> Result<CommandOutput, CliError> {
        unreachable!("read backend never updates mutation state")
    }

    fn execute(
        &self,
        _request: &ParsedRequest,
        _context: Option<&RepoContext>,
    ) -> Result<CommandOutput, CliError> {
        self.result.clone()
    }
}

fn parsed(args: &[&str]) -> ParsedRequest {
    let mut command_line = vec!["vw"];
    command_line.extend_from_slice(args);
    match parse_from(command_line) {
        CliParseResult::Parsed(request) => request,
        outcome => panic!("expected parsed request, got {outcome:?}"),
    }
}

#[test]
fn a008_cancel_is_silent_for_humans_and_a_json_error_envelope() {
    let backend = ReadBackend {
        result: Err(CliError::new(ErrorCode::Cancelled, "selection cancelled")),
    };
    let human = dispatch(&parsed(&["cd"]), &backend);
    assert_eq!(human.exit_code, 130);
    assert!(human.stdout.is_empty());
    assert!(human.stderr.is_empty());

    let json = dispatch(&parsed(&["cd", "--json"]), &backend);
    assert_eq!(json.exit_code, 130);
    assert!(json.stderr.is_empty());
    let value: Value = serde_json::from_str(&json.stdout).expect("cancel JSON envelope");
    assert_eq!(value["error"]["code"], "CANCELLED");
}

#[test]
fn a008_cd_success_is_one_absolute_path_line_or_json_data_path() {
    let path = "/repo/.worktree/feature-a";
    let backend = ReadBackend {
        result: Ok(CommandOutput {
            data: serde_json::json!({ "path": path }),
            human_stdout: format!("{path}\n"),
            human_stderr: String::new(),
            partial_error: None,
        }),
    };
    let human = dispatch(&parsed(&["cd"]), &backend);
    assert_eq!(human.exit_code, 0);
    assert_eq!(human.stdout, format!("{path}\n"));
    assert!(Path::new(human.stdout.trim_end()).is_absolute());
    assert!(human.stderr.is_empty());
    assert!(!human.stdout.contains('\u{1b}'));

    let json = dispatch(&parsed(&["cd", "--json"]), &backend);
    assert_eq!(json.exit_code, 0);
    assert!(json.stderr.is_empty());
    assert!(!json.stdout.contains('\u{1b}'));
    let value: Value = serde_json::from_str(&json.stdout).expect("cd JSON envelope");
    assert_eq!(value["data"]["path"], path);
}

#[test]
fn terminal_capabilities_keep_stdout_and_stderr_policies_independent() {
    let terminal = TerminalCapabilities {
        stdout_tty: false,
        stderr_tty: true,
        stdout_columns: Some(80),
        no_color: false,
    };
    assert!(!terminal.stdout_color_enabled());
    assert!(terminal.picker_interactive());
}
