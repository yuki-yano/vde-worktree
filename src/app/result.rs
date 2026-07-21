use std::env;
use std::io::{self, IsTerminal};

use crate::cli::ParsedRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCapabilities {
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    pub stdout_columns: Option<u16>,
    pub no_color: bool,
}

impl TerminalCapabilities {
    pub fn from_environment() -> Self {
        let stdout_tty = io::stdout().is_terminal();
        let stdout_columns = stdout_tty
            .then(|| {
                env::var("COLUMNS")
                    .ok()
                    .and_then(|value| value.parse::<u16>().ok())
                    .filter(|value| *value > 0)
                    .or_else(|| {
                        terminal_size::terminal_size().map(|(terminal_size::Width(width), _)| width)
                    })
            })
            .flatten();
        Self {
            stdout_tty,
            stderr_tty: io::stderr().is_terminal(),
            stdout_columns,
            no_color: env::var_os("NO_COLOR").is_some(),
        }
    }

    pub const fn stdout_color_enabled(self) -> bool {
        self.stdout_tty && !self.no_color
    }

    pub const fn picker_interactive(self) -> bool {
        self.stderr_tty
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl ProcessOutput {
    pub fn stdout(exit_code: i32, stdout: impl Into<String>) -> Self {
        Self {
            exit_code,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn stderr(exit_code: i32, stderr: impl Into<String>) -> Self {
        Self {
            exit_code,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntrypointOutcome {
    Dispatch(ParsedRequest),
    Rendered(ProcessOutput),
}

#[cfg(test)]
mod tests {
    use super::TerminalCapabilities;

    #[test]
    fn terminal_policy_uses_stdout_for_color_and_stderr_for_picker() {
        let capabilities = TerminalCapabilities {
            stdout_tty: false,
            stderr_tty: true,
            stdout_columns: Some(80),
            no_color: false,
        };

        assert!(!capabilities.stdout_color_enabled());
        assert!(capabilities.picker_interactive());

        let no_color = TerminalCapabilities {
            stdout_tty: true,
            no_color: true,
            ..capabilities
        };
        assert!(!no_color.stdout_color_enabled());
    }
}
