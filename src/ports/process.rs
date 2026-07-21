use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinPolicy {
    Inherit,
    Null,
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPolicy {
    Capture,
    Inherit,
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariable {
    pub name: OsString,
    pub value: Option<OsString>,
}

impl EnvironmentVariable {
    pub fn set(name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        Self {
            name: name.into(),
            value: Some(value.into()),
        }
    }

    pub fn remove(name: impl Into<OsString>) -> Self {
        Self {
            name: name.into(),
            value: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub inherit_env: bool,
    pub env: Vec<EnvironmentVariable>,
    pub stdin: StdinPolicy,
    pub stdout: OutputPolicy,
    pub stderr: OutputPolicy,
    pub timeout: Option<Duration>,
}

impl ProcessCommand {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            inherit_env: true,
            env: Vec::new(),
            stdin: StdinPolicy::Null,
            stdout: OutputPolicy::Capture,
            stderr: OutputPolicy::Capture,
            timeout: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

#[derive(Debug)]
pub enum ProcessError {
    Spawn(std::io::Error),
    Wait(std::io::Error),
    Kill(std::io::Error),
    ReapTimedOut(Duration),
    Stdin(std::io::Error),
    Stdout(std::io::Error),
    Stderr(std::io::Error),
    ReaderThreadPanicked(&'static str),
    WriterThreadPanicked,
    StreamDrainTimedOut(&'static str),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "failed to spawn process: {error}"),
            Self::Wait(error) => write!(formatter, "failed to wait for process: {error}"),
            Self::Kill(error) => write!(formatter, "failed to kill process after timeout: {error}"),
            Self::ReapTimedOut(timeout) => write!(
                formatter,
                "timed out after {timeout:?} while reaping a killed process"
            ),
            Self::Stdin(error) => write!(formatter, "failed to write process stdin: {error}"),
            Self::Stdout(error) => write!(formatter, "failed to read process stdout: {error}"),
            Self::Stderr(error) => write!(formatter, "failed to read process stderr: {error}"),
            Self::ReaderThreadPanicked(stream) => {
                write!(formatter, "{stream} reader thread panicked")
            }
            Self::WriterThreadPanicked => write!(formatter, "stdin writer thread panicked"),
            Self::StreamDrainTimedOut(stream) => {
                write!(formatter, "timed out while draining process {stream}")
            }
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error)
            | Self::Wait(error)
            | Self::Kill(error)
            | Self::Stdin(error)
            | Self::Stdout(error)
            | Self::Stderr(error) => Some(error),
            Self::ReaderThreadPanicked(_)
            | Self::WriterThreadPanicked
            | Self::StreamDrainTimedOut(_)
            | Self::ReapTimedOut(_) => None,
        }
    }
}

pub trait ProcessRunner {
    fn run(&self, command: &ProcessCommand) -> Result<ProcessOutput, ProcessError>;
}
