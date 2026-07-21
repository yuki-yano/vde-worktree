use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use wait_timeout::ChildExt;

use crate::ports::process::{
    OutputPolicy, ProcessCommand, ProcessError, ProcessOutput, ProcessRunner, StdinPolicy,
};

const STREAM_DRAIN_GRACE: Duration = Duration::from_secs(1);

#[derive(Debug, Default, Clone, Copy)]
pub struct StdProcessRunner;

impl ProcessRunner for StdProcessRunner {
    fn run(&self, command: &ProcessCommand) -> Result<ProcessOutput, ProcessError> {
        let started_at = Instant::now();
        let mut process = configured_command(command);
        let mut child = process.spawn().map_err(ProcessError::Spawn)?;
        let process_group = ProcessGroup::for_child(&child)?;
        let stdin_writer = match &command.stdin {
            StdinPolicy::Bytes(bytes) => child.stdin.take().map(|mut stdin| {
                let bytes = bytes.clone();
                spawn_task(move || stdin.write_all(&bytes))
            }),
            StdinPolicy::Inherit | StdinPolicy::Null => None,
        };
        let stdout_reader = child.stdout.take().map(spawn_reader);
        let stderr_reader = child.stderr.take().map(spawn_reader);

        let deadline = command.timeout.map(|timeout| started_at + timeout);
        let mut stdout = None;
        let mut stderr = None;
        let mut stdin = None;
        let mut status = None;
        let mut timed_out = false;

        match receive_reader(stdout_reader.as_ref(), remaining(deadline)) {
            Ok(bytes) => stdout = Some(bytes),
            Err(TaskFailure::TimedOut) => {
                status = Some(terminate_and_reap(&mut child, process_group)?);
                timed_out = true;
            }
            Err(error) => return Err(reader_error(error, "stdout")),
        }
        if !timed_out {
            match receive_reader(stderr_reader.as_ref(), remaining(deadline)) {
                Ok(bytes) => stderr = Some(bytes),
                Err(TaskFailure::TimedOut) => {
                    status = Some(terminate_and_reap(&mut child, process_group)?);
                    timed_out = true;
                }
                Err(error) => return Err(reader_error(error, "stderr")),
            }
        }
        if !timed_out {
            match receive_writer(stdin_writer.as_ref(), remaining(deadline)) {
                Ok(()) => stdin = Some(()),
                Err(TaskFailure::TimedOut) => {
                    status = Some(terminate_and_reap(&mut child, process_group)?);
                    timed_out = true;
                }
                Err(error) => return Err(writer_error(error)),
            }
        }

        if !timed_out {
            let (child_status, child_timed_out) =
                wait_for_child(&mut child, process_group, deadline)?;
            status = Some(child_status);
            timed_out = child_timed_out;
        }

        if timed_out {
            stdout = Some(
                stdout
                    .map_or_else(
                        || receive_reader(stdout_reader.as_ref(), Some(STREAM_DRAIN_GRACE)),
                        Ok,
                    )
                    .map_err(|error| reader_error(error, "stdout"))?,
            );
            stderr = Some(
                stderr
                    .map_or_else(
                        || receive_reader(stderr_reader.as_ref(), Some(STREAM_DRAIN_GRACE)),
                        Ok,
                    )
                    .map_err(|error| reader_error(error, "stderr"))?,
            );
            if stdin.is_none() {
                // BrokenPipe is expected after terminating the process tree. Receiving with a
                // deadline still guarantees that a descendant cannot block this writer forever.
                let _ = receive_writer(stdin_writer.as_ref(), Some(STREAM_DRAIN_GRACE));
            }
        }

        Ok(ProcessOutput {
            stdout: stdout.unwrap_or_default(),
            stderr: stderr.unwrap_or_default(),
            exit_code: status.expect("child status is always collected").code(),
            timed_out,
        })
    }
}

fn configured_command(command: &ProcessCommand) -> Command {
    let mut process = Command::new(&command.program);
    process.args(&command.args);
    if let Some(cwd) = &command.cwd {
        process.current_dir(cwd);
    }
    if !command.inherit_env {
        process.env_clear();
    }
    for variable in &command.env {
        match &variable.value {
            Some(value) => {
                process.env(&variable.name, value);
            }
            None => {
                process.env_remove(&variable.name);
            }
        }
    }
    process.stdin(stdin_configuration(&command.stdin));
    process.stdout(output_configuration(command.stdout));
    process.stderr(output_configuration(command.stderr));
    configure_process_group(&mut process);
    process
}

fn wait_for_child(
    child: &mut Child,
    process_group: ProcessGroup,
    deadline: Option<Instant>,
) -> Result<(std::process::ExitStatus, bool), ProcessError> {
    let Some(deadline) = deadline else {
        return child
            .wait()
            .map(|status| (status, false))
            .map_err(ProcessError::Wait);
    };
    let wait_duration = deadline.saturating_duration_since(Instant::now());
    if let Some(status) = child
        .wait_timeout(wait_duration)
        .map_err(ProcessError::Wait)?
    {
        return Ok((status, false));
    }

    terminate_and_reap(child, process_group).map(|status| (status, true))
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[derive(Debug, Clone, Copy)]
struct ProcessGroup {
    #[cfg(unix)]
    pgid: i32,
}

impl ProcessGroup {
    fn for_child(child: &Child) -> Result<Self, ProcessError> {
        #[cfg(unix)]
        {
            let pgid = i32::try_from(child.id())
                .map_err(std::io::Error::other)
                .map_err(ProcessError::Spawn)?;
            Ok(Self { pgid })
        }
        #[cfg(not(unix))]
        {
            let _ = child;
            Ok(Self {})
        }
    }
}

fn terminate_and_reap(
    child: &mut Child,
    process_group: ProcessGroup,
) -> Result<std::process::ExitStatus, ProcessError> {
    // Signal the captured group while the direct child is still unreaped. Its PID therefore
    // cannot have been reused as another process group's ID. Kill the direct child as well: it
    // remains the process handle's authority even if group signalling was ineffective.
    let group_kill = kill_process_group(process_group);
    let child_kill = kill_direct_child(child);
    let status = child
        .wait_timeout(STREAM_DRAIN_GRACE)
        .map_err(ProcessError::Wait)?
        .ok_or(ProcessError::ReapTimedOut(STREAM_DRAIN_GRACE))?;

    group_kill.map_err(ProcessError::Kill)?;
    child_kill.map_err(ProcessError::Kill)?;
    Ok(status)
}

fn kill_direct_child(child: &mut Child) -> std::io::Result<()> {
    match child.kill() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn kill_process_group(process_group: ProcessGroup) -> std::io::Result<()> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    match killpg(Pid::from_raw(process_group.pgid), Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(std::io::Error::from_raw_os_error(error as i32)),
    }
}

#[cfg(not(unix))]
fn kill_process_group(_process_group: ProcessGroup) -> std::io::Result<()> {
    Ok(())
}

fn stdin_configuration(policy: &StdinPolicy) -> Stdio {
    match policy {
        StdinPolicy::Inherit => Stdio::inherit(),
        StdinPolicy::Null => Stdio::null(),
        StdinPolicy::Bytes(_) => Stdio::piped(),
    }
}

fn output_configuration(policy: OutputPolicy) -> Stdio {
    match policy {
        OutputPolicy::Capture => Stdio::piped(),
        OutputPolicy::Inherit => Stdio::inherit(),
        OutputPolicy::Null => Stdio::null(),
    }
}

struct Task<T> {
    receiver: Receiver<std::io::Result<T>>,
}

fn spawn_task<T, F>(task: F) -> Task<T>
where
    T: Send + 'static,
    F: FnOnce() -> std::io::Result<T> + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(task());
    });
    Task { receiver }
}

fn spawn_reader<R>(mut reader: R) -> Task<Vec<u8>>
where
    R: Read + Send + 'static,
{
    spawn_task(move || {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

enum TaskFailure {
    Io(std::io::Error),
    Panicked,
    TimedOut,
}

fn receive_task<T>(task: Option<&Task<T>>, timeout: Option<Duration>) -> Result<T, TaskFailure>
where
    T: Default,
{
    let Some(task) = task else {
        return Ok(T::default());
    };
    let result = match timeout {
        Some(timeout) => task
            .receiver
            .recv_timeout(timeout)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => TaskFailure::TimedOut,
                RecvTimeoutError::Disconnected => TaskFailure::Panicked,
            })?,
        None => task.receiver.recv().map_err(|_| TaskFailure::Panicked)?,
    };
    result.map_err(TaskFailure::Io)
}

fn receive_reader(
    reader: Option<&Task<Vec<u8>>>,
    timeout: Option<Duration>,
) -> Result<Vec<u8>, TaskFailure> {
    receive_task(reader, timeout)
}

fn receive_writer(writer: Option<&Task<()>>, timeout: Option<Duration>) -> Result<(), TaskFailure> {
    receive_task(writer, timeout)
}

fn reader_error(error: TaskFailure, stream: &'static str) -> ProcessError {
    match error {
        TaskFailure::Io(error) if stream == "stdout" => ProcessError::Stdout(error),
        TaskFailure::Io(error) => ProcessError::Stderr(error),
        TaskFailure::Panicked => ProcessError::ReaderThreadPanicked(stream),
        TaskFailure::TimedOut => ProcessError::StreamDrainTimedOut(stream),
    }
}

fn writer_error(error: TaskFailure) -> ProcessError {
    match error {
        TaskFailure::Io(error) => ProcessError::Stdin(error),
        TaskFailure::Panicked => ProcessError::WriterThreadPanicked,
        TaskFailure::TimedOut => ProcessError::StreamDrainTimedOut("stdin"),
    }
}

fn remaining(deadline: Option<Instant>) -> Option<Duration> {
    deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()))
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsString;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use super::{ProcessGroup, StdProcessRunner, terminate_and_reap};
    use crate::ports::process::{
        EnvironmentVariable, OutputPolicy, ProcessCommand, ProcessRunner, StdinPolicy,
    };

    #[test]
    fn passes_argv_environment_cwd_and_stdin_without_shell_interpolation() {
        let cwd = tempdir().expect("create temporary directory");
        let mut command = ProcessCommand::new("sh");
        command.args = vec![
            OsString::from("-c"),
            OsString::from("printf '%s|%s|%s|' \"$1\" \"$TEST_VALUE\" \"$PWD\"; cat"),
            OsString::from("sh"),
            OsString::from("argument with spaces;$(false)"),
        ];
        command.cwd = Some(cwd.path().to_path_buf());
        command.inherit_env = false;
        command.env = vec![
            EnvironmentVariable::set("PATH", "/usr/bin:/bin"),
            EnvironmentVariable::set("TEST_VALUE", "environment value"),
        ];
        command.stdin = StdinPolicy::Bytes(b"stdin value".to_vec());

        let output = StdProcessRunner.run(&command).expect("run process");

        assert_eq!(output.exit_code, Some(0));
        assert!(!output.timed_out);
        assert_eq!(
            String::from_utf8(output.stdout).expect("stdout is utf-8"),
            format!(
                "argument with spaces;$(false)|environment value|{}|stdin value",
                cwd.path()
                    .canonicalize()
                    .expect("canonicalize temporary directory")
                    .display()
            )
        );
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn supports_inherited_streams() {
        let mut command = ProcessCommand::new("true");
        command.stdin = StdinPolicy::Inherit;
        command.stdout = OutputPolicy::Inherit;
        command.stderr = OutputPolicy::Inherit;

        let output = StdProcessRunner.run(&command).expect("run process");

        assert_eq!(output.exit_code, Some(0));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn kills_and_reaps_a_timed_out_child() {
        let mut command = ProcessCommand::new("sh");
        command.args = vec![
            OsString::from("-c"),
            OsString::from("printf '%s\\n' \"$$\"; exec sleep 30"),
        ];
        command.stdin = StdinPolicy::Bytes(vec![b'x'; 1024 * 1024]);
        command.timeout = Some(Duration::from_millis(100));

        let output = StdProcessRunner.run(&command).expect("run process");
        let pid = String::from_utf8(output.stdout)
            .expect("stdout is utf-8")
            .trim()
            .to_owned();

        assert!(output.timed_out);
        assert_eq!(output.exit_code, None);
        assert_process_exits(&pid);
    }

    #[test]
    fn timeout_kills_group_before_reaping_an_exited_leader() {
        let mut command = ProcessCommand::new("sh");
        command.args = vec![
            OsString::from("-c"),
            OsString::from("sleep 30 & printf '%s\\n' \"$!\""),
        ];
        command.timeout = Some(Duration::from_millis(100));
        let started_at = Instant::now();

        let output = StdProcessRunner.run(&command).expect("run process");
        let pid = String::from_utf8(output.stdout)
            .expect("stdout is utf-8")
            .trim()
            .to_owned();

        assert!(output.timed_out);
        assert!(started_at.elapsed() < Duration::from_secs(2));
        assert_process_exits(&pid);
    }

    #[test]
    fn direct_child_kill_bounds_reaping_when_group_is_already_absent() {
        let mut child = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn direct child");
        let nonexistent_group = ProcessGroup { pgid: i32::MAX };
        let started_at = Instant::now();

        let status = terminate_and_reap(&mut child, nonexistent_group)
            .expect("direct child kill must not depend on group membership");

        assert!(!status.success());
        assert!(started_at.elapsed() < Duration::from_secs(2));
        assert!(
            child.try_wait().expect("query collected child").is_some(),
            "the direct child must already be reaped"
        );
    }

    fn assert_process_exits(pid: &str) {
        for _ in 0..20 {
            let exists = Command::new("sh")
                .args(["-c", "kill -0 \"$1\" 2>/dev/null", "sh", pid])
                .status()
                .expect("check process status")
                .success();
            if !exists {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("process {pid} is still running");
    }
}
