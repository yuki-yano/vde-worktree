use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::fzf::validate_fzf_extra_args;
use crate::ports::process::{
    OutputPolicy, ProcessCommand, ProcessError, ProcessOutput, ProcessRunner, StdinPolicy,
};
use crate::presentation::picker::PickerCandidate;
use crate::state::config::SelectorCdSurface;

const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct FzfRequest<'a> {
    pub candidates: &'a [PickerCandidate],
    pub cwd: &'a Path,
    pub prompt: &'a str,
    pub surface: SelectorCdSurface,
    pub tmux_popup_opts: &'a str,
    pub extra_args: &'a [String],
    pub stderr_is_terminal: bool,
    pub in_tmux: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FzfSelection {
    Selected(PathBuf),
    Cancelled,
}

#[derive(Debug)]
pub enum FzfError {
    NoCandidates,
    InteractiveRequired,
    DependencyMissing,
    InvalidArgument(String),
    TmuxPopupUnsupported,
    CapabilityCheckFailed(FzfCommandFailure),
    CommandFailed(FzfCommandFailure),
    InvalidOutput(String),
    AmbiguousCandidate(String),
    Process(ProcessError),
}

#[derive(Debug)]
pub struct FzfCommandFailure {
    pub signal: Option<i32>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl fmt::Display for FzfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCandidates => formatter.write_str("no worktree candidates were provided"),
            Self::InteractiveRequired => {
                formatter.write_str("fzf selection requires an interactive stderr terminal")
            }
            Self::DependencyMissing => {
                formatter.write_str("fzf is required for interactive selection")
            }
            Self::InvalidArgument(message) => formatter.write_str(message),
            Self::TmuxPopupUnsupported => formatter.write_str(
                "selector.cd.surface=tmux-popup requires an fzf version with native --tmux support",
            ),
            Self::CapabilityCheckFailed(failure) => write!(
                formatter,
                "failed to check fzf --tmux capability (exit code: {:?}, timed out: {})",
                failure.exit_code, failure.timed_out
            ),
            Self::CommandFailed(failure) => write!(
                formatter,
                "fzf selection failed (exit code: {:?}, timed out: {})",
                failure.exit_code, failure.timed_out
            ),
            Self::InvalidOutput(message) | Self::AmbiguousCandidate(message) => {
                formatter.write_str(message)
            }
            Self::Process(source) => write!(formatter, "failed to execute fzf: {source}"),
        }
    }
}

impl std::error::Error for FzfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Process(source) => Some(source),
            Self::NoCandidates
            | Self::InteractiveRequired
            | Self::DependencyMissing
            | Self::InvalidArgument(_)
            | Self::TmuxPopupUnsupported
            | Self::CapabilityCheckFailed(_)
            | Self::CommandFailed(_)
            | Self::InvalidOutput(_)
            | Self::AmbiguousCandidate(_) => None,
        }
    }
}

#[derive(Debug)]
pub struct FzfAdapter<R> {
    runner: R,
}

impl<R> FzfAdapter<R>
where
    R: ProcessRunner,
{
    pub const fn new(runner: R) -> Self {
        Self { runner }
    }

    pub fn select_path(&self, request: &FzfRequest<'_>) -> Result<FzfSelection, FzfError> {
        if request.candidates.is_empty() {
            return Err(FzfError::NoCandidates);
        }
        if !request.stderr_is_terminal {
            return Err(FzfError::InteractiveRequired);
        }
        validate_fzf_extra_args(request.extra_args)
            .map_err(|error| FzfError::InvalidArgument(error.to_string()))?;
        self.ensure_available(request.cwd)?;

        let popup = match request.surface {
            SelectorCdSurface::Inline => false,
            SelectorCdSurface::Auto if !request.in_tmux => false,
            SelectorCdSurface::Auto => self.supports_tmux(request.cwd).unwrap_or(false),
            SelectorCdSurface::TmuxPopup => {
                if !request.in_tmux {
                    return Err(FzfError::TmuxPopupUnsupported);
                }
                if !self.supports_tmux(request.cwd)? {
                    return Err(FzfError::TmuxPopupUnsupported);
                }
                true
            }
        };
        let (input, candidates) = candidate_input(request.candidates)?;
        let mut command = ProcessCommand::new("fzf");
        command.args = build_args(request, popup);
        command.cwd = Some(request.cwd.to_path_buf());
        command.stdin = StdinPolicy::Bytes(input.into_bytes());
        command.stdout = OutputPolicy::Capture;
        command.stderr = OutputPolicy::Inherit;
        command.timeout = None;
        let output = self.runner.run(&command).map_err(map_process_error)?;
        if output.exit_code == Some(130) || output.signal == Some(2) {
            return Ok(FzfSelection::Cancelled);
        }
        if output.exit_code != Some(0) || output.timed_out || output.is_truncated() {
            return Err(FzfError::CommandFailed(output.into()));
        }

        let selected = String::from_utf8(output.stdout).map_err(|_| {
            FzfError::InvalidOutput("fzf returned non-UTF-8 selection output".to_owned())
        })?;
        let selected = strip_ansi(trim_trailing_newlines(&selected));
        if selected.is_empty() {
            return Ok(FzfSelection::Cancelled);
        }
        candidates
            .get(&selected)
            .cloned()
            .map(FzfSelection::Selected)
            .ok_or_else(|| {
                FzfError::InvalidOutput(
                    "fzf returned a value that is not in the candidate list".to_owned(),
                )
            })
    }

    fn ensure_available(&self, cwd: &Path) -> Result<(), FzfError> {
        let output = match self.runner.run(&check_command(cwd, "--version")) {
            Ok(output) => output,
            Err(ProcessError::Spawn(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(FzfError::DependencyMissing);
            }
            Err(error) => return Err(FzfError::Process(error)),
        };
        if output.exit_code == Some(0) && !output.timed_out && !output.is_truncated() {
            Ok(())
        } else {
            Err(FzfError::DependencyMissing)
        }
    }

    fn supports_tmux(&self, cwd: &Path) -> Result<bool, FzfError> {
        let output = self
            .runner
            .run(&check_command(cwd, "--help"))
            .map_err(map_process_error)?;
        if output.exit_code != Some(0) || output.timed_out || output.is_truncated() {
            return Err(FzfError::CapabilityCheckFailed(output.into()));
        }
        Ok(output
            .stdout
            .windows(b"--tmux".len())
            .any(|window| window == b"--tmux"))
    }
}

impl From<ProcessOutput> for FzfCommandFailure {
    fn from(output: ProcessOutput) -> Self {
        Self {
            signal: output.signal,
            stdout_truncated: output.stdout_truncated,
            stderr_truncated: output.stderr_truncated,
            exit_code: output.exit_code,
            timed_out: output.timed_out,
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}

fn check_command(cwd: &Path, argument: &str) -> ProcessCommand {
    let mut command = ProcessCommand::new("fzf");
    command.args = vec![OsString::from(argument)];
    command.cwd = Some(cwd.to_path_buf());
    command.stdin = StdinPolicy::Null;
    command.stdout = OutputPolicy::Capture;
    command.stderr = OutputPolicy::Capture;
    command.timeout = Some(CHECK_TIMEOUT);
    command
}

fn build_args(request: &FzfRequest<'_>, popup: bool) -> Vec<OsString> {
    let mut args = [
        format!("--prompt={}", request.prompt),
        "--layout=reverse".to_owned(),
        "--height=80%".to_owned(),
        "--border".to_owned(),
        "--delimiter=\t".to_owned(),
        "--with-nth=1".to_owned(),
        "--preview=printf '%b' {3}".to_owned(),
        "--preview-window=right,60%,wrap".to_owned(),
        "--ansi".to_owned(),
    ]
    .into_iter()
    .chain(request.extra_args.iter().cloned())
    .map(OsString::from)
    .collect::<Vec<_>>();
    if popup {
        args.push(OsString::from(format!(
            "--tmux={}",
            request.tmux_popup_opts
        )));
    }
    args
}

fn candidate_input(
    candidates: &[PickerCandidate],
) -> Result<(String, HashMap<String, PathBuf>), FzfError> {
    let mut lines = Vec::with_capacity(candidates.len());
    let mut mapping = HashMap::with_capacity(candidates.len());
    for candidate in candidates {
        let line = sanitize_candidate(&candidate.line);
        if line.trim().is_empty() {
            continue;
        }
        let key = strip_ansi(&line);
        if mapping
            .insert(key.clone(), candidate.path.clone())
            .is_some()
        {
            return Err(FzfError::AmbiguousCandidate(format!(
                "multiple fzf candidates render as the same selection: {key}"
            )));
        }
        lines.push(line);
    }
    if lines.is_empty() {
        return Err(FzfError::NoCandidates);
    }
    Ok((lines.join("\n"), mapping))
}

fn sanitize_candidate(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn trim_trailing_newlines(value: &str) -> &str {
    value.trim_end_matches(['\r', '\n'])
}

fn strip_ansi(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
            index += 2;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn map_process_error(error: ProcessError) -> FzfError {
    if matches!(&error, ProcessError::Spawn(source) if source.kind() == std::io::ErrorKind::NotFound)
    {
        FzfError::DependencyMissing
    } else {
        FzfError::Process(error)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    #[derive(Debug)]
    struct FakeRunner {
        commands: RefCell<Vec<ProcessCommand>>,
        outputs: RefCell<VecDeque<Result<ProcessOutput, ProcessError>>>,
    }

    impl FakeRunner {
        fn with_outputs(outputs: Vec<ProcessOutput>) -> Self {
            Self {
                commands: RefCell::new(Vec::new()),
                outputs: RefCell::new(outputs.into_iter().map(Ok).collect()),
            }
        }
    }

    impl ProcessRunner for FakeRunner {
        fn run(&self, command: &ProcessCommand) -> Result<ProcessOutput, ProcessError> {
            self.commands.borrow_mut().push(command.clone());
            self.outputs.borrow_mut().pop_front().expect("fake output")
        }
    }

    fn output(stdout: &str, exit_code: i32) -> ProcessOutput {
        ProcessOutput {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: Some(exit_code),
            timed_out: false,
            ..Default::default()
        }
    }

    fn candidate() -> PickerCandidate {
        PickerCandidate {
            line: "\u{1b}[38;2;1;2;3m* main\u{1b}[0m\t/repo\tpreview".to_owned(),
            path: PathBuf::from("/repo"),
        }
    }

    fn request(candidates: &[PickerCandidate]) -> FzfRequest<'_> {
        FzfRequest {
            candidates,
            cwd: Path::new("/repo"),
            prompt: "worktree> ",
            surface: SelectorCdSurface::Inline,
            tmux_popup_opts: "80%,70%",
            extra_args: &[],
            stderr_is_terminal: true,
            in_tmux: false,
        }
    }

    #[test]
    fn truncated_selection_is_rejected_even_if_its_prefix_matches_a_candidate() {
        let mut selection = output("* main\t/repo\tpreview\n", 0);
        selection.stdout_truncated = true;
        let adapter = FzfAdapter::new(FakeRunner::with_outputs(vec![
            output("0.60.0", 0),
            selection,
        ]));
        assert!(
            matches!(adapter.select_path(&request(&[candidate()])), Err(FzfError::CommandFailed(failure)) if failure.stdout_truncated)
        );
    }

    #[test]
    fn uses_shell_free_argv_inherited_stderr_and_captured_stdout() {
        let candidates = [candidate()];
        let runner = FakeRunner::with_outputs(vec![
            output("0.60.0\n", 0),
            output("* main\t/repo\tpreview\n", 0),
        ]);
        let adapter = FzfAdapter::new(runner);
        assert_eq!(
            adapter.select_path(&request(&candidates)).unwrap(),
            FzfSelection::Selected(PathBuf::from("/repo"))
        );
        let commands = adapter.runner.commands.borrow();
        let selection = &commands[1];
        assert_eq!(selection.program, "fzf");
        assert_eq!(selection.stderr, OutputPolicy::Inherit);
        assert_eq!(selection.stdout, OutputPolicy::Capture);
        assert!(selection.args.contains(&OsString::from("--delimiter=\t")));
        assert!(selection.args.contains(&OsString::from("--ansi")));
        assert!(
            selection
                .args
                .contains(&OsString::from("--preview=printf '%b' {3}"))
        );
    }

    #[test]
    fn no_color_candidate_keeps_leading_alignment_spaces() {
        let candidates = [PickerCandidate {
            line: "  feature/a  CLEAN | UNKNOWN | OPEN\t/repo/a\tpreview".to_owned(),
            path: PathBuf::from("/repo/a"),
        }];
        let runner = FakeRunner::with_outputs(vec![
            output("0.60.0\n", 0),
            output("  feature/a  CLEAN | UNKNOWN | OPEN\t/repo/a\tpreview\n", 0),
        ]);
        let adapter = FzfAdapter::new(runner);

        assert_eq!(
            adapter.select_path(&request(&candidates)).unwrap(),
            FzfSelection::Selected(PathBuf::from("/repo/a"))
        );
        let commands = adapter.runner.commands.borrow();
        let StdinPolicy::Bytes(input) = &commands[1].stdin else {
            panic!("fzf input must be captured bytes");
        };
        assert!(input.starts_with(b"  feature/a"));
    }

    #[test]
    fn auto_uses_native_tmux_only_when_capability_is_present() {
        let candidates = [candidate()];
        let runner = FakeRunner::with_outputs(vec![
            output("0.60.0", 0),
            output("usage: fzf --tmux", 0),
            output("* main\t/repo\tpreview", 0),
        ]);
        let adapter = FzfAdapter::new(runner);
        let mut request = request(&candidates);
        request.surface = SelectorCdSurface::Auto;
        request.in_tmux = true;
        request.tmux_popup_opts = "90%,80%";
        adapter.select_path(&request).unwrap();
        let commands = adapter.runner.commands.borrow();
        assert!(commands[2].args.contains(&OsString::from("--tmux=90%,80%")));
    }

    #[test]
    fn explicit_popup_without_capability_is_typed_and_never_falls_back() {
        let candidates = [candidate()];
        let runner = FakeRunner::with_outputs(vec![output("0.50.0", 0), output("usage: fzf", 0)]);
        let adapter = FzfAdapter::new(runner);
        let mut request = request(&candidates);
        request.surface = SelectorCdSurface::TmuxPopup;
        request.in_tmux = true;
        assert!(matches!(
            adapter.select_path(&request),
            Err(FzfError::TmuxPopupUnsupported)
        ));
        assert_eq!(adapter.runner.commands.borrow().len(), 2);
    }

    #[test]
    fn explicit_popup_outside_tmux_is_typed_without_attempting_inline_or_capability_fallback() {
        let candidates = [candidate()];
        let runner = FakeRunner::with_outputs(vec![output("0.60.0", 0)]);
        let adapter = FzfAdapter::new(runner);
        let mut request = request(&candidates);
        request.surface = SelectorCdSurface::TmuxPopup;
        request.in_tmux = false;

        assert!(matches!(
            adapter.select_path(&request),
            Err(FzfError::TmuxPopupUnsupported)
        ));
        assert_eq!(adapter.runner.commands.borrow().len(), 1);
    }

    #[test]
    fn auto_uses_inline_when_the_tmux_capability_check_fails() {
        let candidates = [candidate()];
        let runner = FakeRunner::with_outputs(vec![
            output("0.60.0", 0),
            output("capability check failed", 2),
            output("* main\t/repo\tpreview", 0),
        ]);
        let adapter = FzfAdapter::new(runner);
        let mut request = request(&candidates);
        request.surface = SelectorCdSurface::Auto;
        request.in_tmux = true;

        assert_eq!(
            adapter.select_path(&request).unwrap(),
            FzfSelection::Selected(PathBuf::from("/repo"))
        );
        let commands = adapter.runner.commands.borrow();
        assert!(
            !commands[2]
                .args
                .iter()
                .any(|arg| arg.to_string_lossy().starts_with("--tmux="))
        );
    }

    #[test]
    fn popup_runtime_failure_is_returned_without_inline_retry() {
        let candidates = [candidate()];
        let runner = FakeRunner::with_outputs(vec![
            output("0.60.0", 0),
            output("usage: fzf --tmux", 0),
            output("unknown option: --tmux", 2),
        ]);
        let adapter = FzfAdapter::new(runner);
        let mut request = request(&candidates);
        request.surface = SelectorCdSurface::TmuxPopup;
        request.in_tmux = true;
        assert!(matches!(
            adapter.select_path(&request),
            Err(FzfError::CommandFailed(FzfCommandFailure {
                exit_code: Some(2),
                ..
            }))
        ));
        let commands = adapter.runner.commands.borrow();
        assert_eq!(commands.len(), 3);
        assert!(
            commands[2]
                .args
                .iter()
                .any(|argument| argument.to_string_lossy().starts_with("--tmux="))
        );
    }

    #[test]
    fn reports_cancel_130_and_rejects_unknown_selection() {
        let candidates = [candidate()];
        let cancelled = FzfAdapter::new(FakeRunner::with_outputs(vec![
            output("0.60.0", 0),
            output("", 130),
        ]));
        assert_eq!(
            cancelled.select_path(&request(&candidates)).unwrap(),
            FzfSelection::Cancelled
        );

        let invalid = FzfAdapter::new(FakeRunner::with_outputs(vec![
            output("0.60.0", 0),
            output("outside", 0),
        ]));
        assert!(matches!(
            invalid.select_path(&request(&candidates)),
            Err(FzfError::InvalidOutput(_))
        ));
    }

    #[test]
    fn stdout_may_be_piped_but_stderr_must_be_a_terminal() {
        let candidates = [candidate()];
        let adapter = FzfAdapter::new(FakeRunner::with_outputs(Vec::new()));
        let mut request = request(&candidates);
        request.stderr_is_terminal = false;
        assert!(matches!(
            adapter.select_path(&request),
            Err(FzfError::InteractiveRequired)
        ));
        assert!(adapter.runner.commands.borrow().is_empty());
    }

    #[test]
    fn empty_candidates_fail_before_dependency_checks() {
        let adapter = FzfAdapter::new(FakeRunner::with_outputs(Vec::new()));
        assert!(matches!(
            adapter.select_path(&request(&[])),
            Err(FzfError::NoCandidates)
        ));
        assert!(adapter.runner.commands.borrow().is_empty());
    }
}
