pub mod adapters;
pub mod app;
pub mod cli;
pub mod domain;
pub mod ports;
pub mod presentation;
pub mod state;

use std::ffi::OsString;
use std::io::{self, Write};

use app::dispatch::{SystemBackend, dispatch};
use app::result::{EntrypointOutcome, ProcessOutput};
use cli::CliParseResult;
use domain::error::{ExecutionPhase, ExecutionState};
use presentation::json::{ErrorEnvelope, ErrorPayload, to_stdout_json};

pub fn entrypoint<I, T>(args: I) -> EntrypointOutcome
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let hints = cli::argument_hints(&args);
    let json_requested = hints.json;
    let command = hints.command.unwrap_or_else(|| "vw".to_owned());

    match cli::parse_from(args) {
        CliParseResult::Parsed(request) => EntrypointOutcome::Dispatch(request),
        CliParseResult::Display(text) => {
            EntrypointOutcome::Rendered(ProcessOutput::stdout(0, text))
        }
        CliParseResult::Invalid { error, rendered: _ } => {
            let error = error.at_phase(ExecutionPhase::Parse, ExecutionState::NotStarted, &[]);
            if json_requested {
                let envelope = ErrorEnvelope::new(command, None, ErrorPayload::from(&error));
                match to_stdout_json(&envelope) {
                    Ok(stdout) => EntrypointOutcome::Rendered(ProcessOutput::stdout(
                        error.exit_code(),
                        stdout,
                    )),
                    Err(serialization_error) => EntrypointOutcome::Rendered(ProcessOutput::stderr(
                        30,
                        format!("{serialization_error}\n"),
                    )),
                }
            } else {
                EntrypointOutcome::Rendered(ProcessOutput::stderr(
                    error.exit_code(),
                    format!("[{}] {}\n", error.code, error.message.trim_end()),
                ))
            }
        }
    }
}

pub fn run_from_env() -> i32 {
    match entrypoint(std::env::args_os()) {
        EntrypointOutcome::Dispatch(request) => {
            let output = match SystemBackend::from_environment() {
                Ok(backend) => dispatch(&request, &backend),
                Err(error) => {
                    if request.common.json {
                        let envelope = ErrorEnvelope::new(
                            request.command.name(),
                            None,
                            ErrorPayload::from(&error),
                        );
                        match to_stdout_json(&envelope) {
                            Ok(stdout) => ProcessOutput::stdout(error.exit_code(), stdout),
                            Err(serialization_error) => {
                                ProcessOutput::stderr(30, format!("{serialization_error}\n"))
                            }
                        }
                    } else {
                        ProcessOutput::stderr(
                            error.exit_code(),
                            format!("[{}] {}\n", error.code, error.message),
                        )
                    }
                }
            };
            write_process_output(&output);
            output.exit_code
        }
        EntrypointOutcome::Rendered(output) => {
            write_process_output(&output);
            output.exit_code
        }
    }
}

fn write_process_output(output: &ProcessOutput) {
    if !output.stdout.is_empty() {
        let _ = io::stdout().lock().write_all(output.stdout.as_bytes());
    }
    if !output.stderr.is_empty() {
        let _ = io::stderr().lock().write_all(output.stderr.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::entrypoint;
    use crate::app::result::EntrypointOutcome;

    #[test]
    fn invalid_request_json_routing_skips_values_and_child_arguments() {
        for args in [
            vec!["vw", "--fzf-arg", "--json", "list", "--invalid"],
            vec!["vw", "--fzf-arg=--json", "list", "--invalid"],
            vec!["vw", "exec", "--invalid", "main", "--", "echo", "--json"],
        ] {
            let EntrypointOutcome::Rendered(output) = entrypoint(args.clone()) else {
                panic!("expected error")
            };
            assert_eq!(output.exit_code, 3, "{args:?}");
            assert_eq!(output.stdout, "", "{args:?}");
            assert!(output.stderr.contains("INVALID_ARGUMENT"));
        }
        for args in [
            vec!["vw", "--json"],
            vec!["vw", "--prompt", "--json", "list"],
            vec!["vw", "--hook-timeout-ms", "--json", "list"],
            vec!["vw", "--fzf-arg", "--json", "--json", "list", "--invalid"],
            vec![
                "vw",
                "exec",
                "main",
                "--json",
                "--invalid",
                "--",
                "echo",
                "--no-hooks",
            ],
        ] {
            let EntrypointOutcome::Rendered(output) = entrypoint(args.clone()) else {
                panic!("expected error")
            };
            assert_eq!(output.exit_code, 3, "{args:?}");
            assert_eq!(output.stderr, "", "{args:?}");
            let value: Value = serde_json::from_str(&output.stdout).unwrap();
            assert_eq!(value["status"], "error");
            assert_eq!(value["error"]["execution"]["phase"], "parse");
        }
    }

    #[test]
    fn entrypoint_returns_parsed_requests_for_dispatch() {
        match entrypoint(["vw", "list", "--json"]) {
            EntrypointOutcome::Dispatch(request) => {
                assert_eq!(request.command.name(), "list");
                assert!(request.common.json);
            }
            outcome @ EntrypointOutcome::Rendered(_) => {
                panic!("expected dispatch, got {outcome:?}")
            }
        }
    }

    #[test]
    fn entrypoint_renders_help_and_version_to_stdout_with_exit_zero() {
        for args in [["vw", "--help"], ["vw", "--version"]] {
            match entrypoint(args) {
                EntrypointOutcome::Rendered(output) => {
                    assert_eq!(output.exit_code, 0);
                    assert!(!output.stdout.is_empty());
                    assert!(output.stderr.is_empty());
                }
                outcome @ EntrypointOutcome::Dispatch(_) => {
                    panic!("expected rendered output, got {outcome:?}")
                }
            }
        }
    }

    #[test]
    fn entrypoint_renders_invalid_arguments_as_human_or_json_errors() {
        match entrypoint(["vw", "list", "--unknown"]) {
            EntrypointOutcome::Rendered(output) => {
                assert_eq!(output.exit_code, 3);
                assert!(output.stdout.is_empty());
                assert!(output.stderr.contains("unexpected argument"));
            }
            outcome @ EntrypointOutcome::Dispatch(_) => {
                panic!("expected rendered output, got {outcome:?}")
            }
        }

        match entrypoint(["vw", "list", "--unknown", "--json"]) {
            EntrypointOutcome::Rendered(output) => {
                assert_eq!(output.exit_code, 3);
                assert!(output.stderr.is_empty());
                assert!(output.stdout.ends_with('\n'));
                let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
                assert_eq!(value["schemaVersion"], 3);
                assert_eq!(value["command"], "list");
                assert_eq!(value["status"], "error");
                assert!(value["data"].is_null());
                assert_eq!(value["error"]["code"], "INVALID_ARGUMENT");
            }
            outcome @ EntrypointOutcome::Dispatch(_) => {
                panic!("expected rendered output, got {outcome:?}")
            }
        }

        match entrypoint(["vw", "--json", "not-a-command"]) {
            EntrypointOutcome::Rendered(output) => {
                let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
                assert_eq!(value["command"], "not-a-command");
                assert_eq!(value["error"]["code"], "UNKNOWN_COMMAND");
            }
            outcome @ EntrypointOutcome::Dispatch(_) => {
                panic!("expected rendered output, got {outcome:?}")
            }
        }
    }

    #[test]
    fn entrypoint_applies_common_safety_policy_to_human_and_json_requests() {
        match entrypoint(["vw", "list", "--no-hooks"]) {
            EntrypointOutcome::Rendered(output) => {
                assert_eq!(output.exit_code, 4);
                assert!(output.stdout.is_empty());
                assert!(output.stderr.contains("--allow-unsafe"));
            }
            outcome @ EntrypointOutcome::Dispatch(_) => {
                panic!("expected policy rejection, got {outcome:?}")
            }
        }

        match entrypoint(["vw", "list", "--no-hooks", "--json"]) {
            EntrypointOutcome::Rendered(output) => {
                assert_eq!(output.exit_code, 4);
                assert!(output.stderr.is_empty());
                let value: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
                assert_eq!(value["error"]["code"], "UNSAFE_FLAG_REQUIRED");
            }
            outcome @ EntrypointOutcome::Dispatch(_) => {
                panic!("expected policy rejection, got {outcome:?}")
            }
        }
    }
}
