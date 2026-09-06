//! Public command documentation and schemas. Argument metadata is read from the built Clap tree;
//! command semantics and result schemas are maintained here and exercised against real CLI output.
use clap::{ArgAction, Command as ClapCommand};
use serde_json::{Value, json};

use super::COMMAND_NAMES;
use crate::domain::error::{CliError, ErrorCode};
use crate::presentation::json::SCHEMA_VERSION;

pub struct CompletionBinding {
    pub command: &'static str,
    pub argument: &'static str,
    pub long: Option<&'static str>,
    pub kind: &'static str,
}

pub const COMPLETION_BINDINGS: &[CompletionBinding] = &[
    CompletionBinding {
        command: "status",
        argument: "branch",
        long: None,
        kind: "worktrees",
    },
    CompletionBinding {
        command: "path",
        argument: "branch",
        long: None,
        kind: "worktrees",
    },
    CompletionBinding {
        command: "switch",
        argument: "branch",
        long: None,
        kind: "use-branches",
    },
    CompletionBinding {
        command: "del",
        argument: "branch",
        long: None,
        kind: "worktrees",
    },
    CompletionBinding {
        command: "absorb",
        argument: "branch",
        long: None,
        kind: "worktrees",
    },
    CompletionBinding {
        command: "exec",
        argument: "branch",
        long: None,
        kind: "worktrees",
    },
    CompletionBinding {
        command: "lock",
        argument: "branch",
        long: None,
        kind: "worktrees",
    },
    CompletionBinding {
        command: "unlock",
        argument: "branch",
        long: None,
        kind: "worktrees",
    },
    CompletionBinding {
        command: "use",
        argument: "branch",
        long: None,
        kind: "use-branches",
    },
    CompletionBinding {
        command: "unabsorb",
        argument: "branch",
        long: None,
        kind: "use-branches",
    },
    CompletionBinding {
        command: "get",
        argument: "remote_branch",
        long: None,
        kind: "remote-branches",
    },
    CompletionBinding {
        command: "invoke",
        argument: "hook",
        long: None,
        kind: "hooks",
    },
    CompletionBinding {
        command: "absorb",
        argument: "from",
        long: Some("from"),
        kind: "managed-worktrees",
    },
    CompletionBinding {
        command: "unabsorb",
        argument: "to",
        long: Some("to"),
        kind: "managed-worktrees",
    },
];

struct Semantics {
    target: &'static str,
    prerequisites: &'static [&'static str],
    effects: &'static [&'static str],
    examples: &'static [&'static str],
}

#[allow(clippy::too_many_lines)]
fn semantics(name: &str) -> Semantics {
    let (target, prerequisites, effects, examples): (_, &[_], &[_], &[_]) = match name {
        "init" => (
            "primary repository",
            &["Git repository"],
            &[
                "Create managed root, metadata directories and hook templates; update Git exclude rules",
            ],
            &["vw init --json"],
        ),
        "list" => (
            "all registered worktrees",
            &["Git repository and a resolvable base branch"],
            &[
                "Query Git and optional GitHub PR state; persist lifecycle observations except in monitor mode",
            ],
            &["vw list --json --no-gh", "vw list --json --no-gh --monitor"],
        ),
        "status" => (
            "unambiguous branch, explicit --worktree path, or current worktree",
            &["Git repository and a resolvable base branch"],
            &["Query Git and optional GitHub PR state; persist lifecycle observations"],
            &["vw status --json --no-gh", "vw status feature/topic --json"],
        ),
        "path" => (
            "unambiguous branch or explicit --worktree path",
            &["Git repository; selected worktree must be registered"],
            &["Print the absolute worktree path"],
            &["vw path feature/topic", "vw path feature/topic --json"],
        ),
        "switch" => (
            "existing attachment or a new managed worktree",
            &["Initialized repository"],
            &[
                "Create a missing branch and worktree; save lifecycle state; run hooks even when reusing an attachment",
            ],
            &["vw switch feature/topic --json"],
        ),
        "new" => (
            "new branch and managed worktree",
            &["Initialized repository; branch and target must be absent"],
            &[
                "Create branch from the configured base and attach a worktree; copy .worktreeinclude paths; save lifecycle state; run hooks",
            ],
            &["vw new feature/topic --json", "vw new --json"],
        ),
        "mv" => (
            "current linked worktree",
            &["Initialized repository; invocation from a managed linked worktree"],
            &[
                "Rename branch and move worktree, lifecycle and lock metadata transactionally; run hooks",
            ],
            &["vw mv feature/renamed --json"],
        ),
        "del" => (
            "specified branch or current linked worktree",
            &[
                "Initialized repository; managed non-primary target; clean, unlocked, merged and no unpushed commits unless explicitly overridden",
            ],
            &["Remove worktree, branch, lifecycle and lock metadata; run hooks"],
            &[
                "vw del feature/topic --json",
                "vw del feature/topic --force-dirty --allow-unsafe --json",
            ],
        ),
        "gone" => (
            "eligible managed worktrees",
            &[
                "Initialized repository; candidates must be clean, unlocked, merged, attached and non-primary",
            ],
            &[
                "Preview by default; --apply deletes worktrees, branches and metadata and runs hooks; upstream-ahead is not a gone guard",
            ],
            &["vw gone --json", "vw gone --apply --json"],
        ),
        "adopt" => (
            "registered worktrees outside the managed root",
            &["Initialized repository; unambiguous branch and an absent destination"],
            &["Preview by default; --apply moves eligible worktrees and runs hooks"],
            &["vw adopt --json", "vw adopt --apply --json"],
        ),
        "get" => (
            "remote/branch attachment",
            &["Initialized repository; reachable remote branch"],
            &[
                "Fetch remote branch, create local tracking branch and attach worktree when absent; save lifecycle state; run hooks",
            ],
            &["vw get origin/feature/topic --json"],
        ),
        "extract" => (
            "current primary branch",
            &[
                "Initialized repository; primary invocation; --current; dirty worktree requires --stash",
            ],
            &[
                "Optionally stash changes; check out base in primary and attach the original branch under managed root; transfer changes; save state; run hooks",
            ],
            &["vw extract --current --stash --json"],
        ),
        "absorb" => (
            "managed source and primary destination",
            &[
                "Initialized repository; clean primary; non-interactive use requires --allow-agent and --allow-unsafe",
            ],
            &[
                "Stash source changes, check out branch in primary, apply exact stash and optionally drop it; run pre-hook at source and post-hook at target",
            ],
            &[
                "vw absorb feature/topic --allow-agent --allow-unsafe --json",
                "vw absorb feature/topic --from feature/topic --keep-stash --allow-agent --allow-unsafe --json",
            ],
        ),
        "unabsorb" => (
            "dirty primary source and clean managed destination",
            &[
                "Initialized repository; branch must be current in primary; non-interactive use requires --allow-agent and --allow-unsafe",
            ],
            &[
                "Stash primary changes, apply exact stash at target and optionally drop it; run pre-hook at source and post-hook at target",
            ],
            &["vw unabsorb feature/topic --allow-agent --allow-unsafe --json"],
        ),
        "use" => (
            "primary worktree",
            &[
                "Initialized repository; clean primary; non-interactive use requires --allow-agent and --allow-unsafe",
            ],
            &[
                "Check out the local branch; --allow-shared permits a linked attachment to remain; run hooks",
            ],
            &["vw use feature/topic --allow-agent --allow-unsafe --json"],
        ),
        "exec" => (
            "unambiguous branch or explicit --worktree path",
            &["Registered worktree; executable argv after --"],
            &[
                "Run arbitrary child process in the worktree; JSON captures child stdout/stderr, human mode inherits terminal streams",
            ],
            &[
                "vw exec feature/topic --json -- cargo test",
                "vw exec feature/topic -- git status --short",
            ],
        ),
        "invoke" => (
            "named repository hook at current worktree",
            &["Executable hook in .vde/worktree/hooks"],
            &["Run the named hook with optional argv; write hook log"],
            &[
                "vw invoke post-switch --json",
                "vw invoke post-switch -- --custom-argument",
            ],
        ),
        "copy" => (
            "explicit --worktree path, WT_WORKTREE_PATH, or current worktree",
            &["Existing target distinct from primary; repository-relative source paths"],
            &["Transactionally copy the requested paths; roll back the batch on failure"],
            &["vw copy .env.local config/local.yml --json"],
        ),
        "link" => (
            "explicit --worktree path, WT_WORKTREE_PATH, or current worktree",
            &[
                "Existing target distinct from primary; repository-relative source paths; symlink support",
            ],
            &[
                "Transactionally create symlinks for requested paths; roll back the batch on failure",
            ],
            &["vw link .env.local --json"],
        ),
        "lock" => (
            "attached branch",
            &["Initialized repository; an absent lock or matching owner"],
            &[
                "Persist worktree deletion protection with owner and reason; no automatic command hooks",
            ],
            &["vw lock feature/topic --owner agent-session-42 --reason 'active task' --json"],
        ),
        "unlock" => (
            "branch lock record",
            &["Initialized repository; owner must match unless --force"],
            &["Remove deletion protection; missing lock is a no-op; no automatic command hooks"],
            &["vw unlock feature/topic --owner agent-session-42 --json"],
        ),
        "cd" => (
            "worktree selected by fzf",
            &["Git repository; fzf; interactive stderr terminal"],
            &["Query status and show picker; stdout returns a path for the caller to use"],
            &["cd -- \"$(vw cd)\"", "vw cd --json"],
        ),
        "completion" => (
            "shell script or installation path",
            &[],
            &["Print generated script; --install atomically replaces the completion file"],
            &["vw completion zsh", "vw completion fish --install --json"],
        ),
        "describe" => (
            "public command definitions",
            &[],
            &["Print command metadata and JSON schemas; repository-independent"],
            &["vw describe --json", "vw describe exec --json"],
        ),
        "context" => (
            "repository and invocation directory",
            &["Git repository and valid configuration"],
            &[
                "Report effective settings and per-field sources, paths, initialization and pending journals; no writes or GitHub queries",
            ],
            &["vw -C /projects/repo context --json"],
        ),
        "doctor" => (
            "invocation directory, repository setup and optional dependencies",
            &[],
            &[
                "Read configuration and pending journals without recovery; probe dependencies with 5-second limits; enabled GitHub authentication check may access the network; setup errors retain diagnostics and exit 4",
            ],
            &[
                "vw doctor --json --no-gh",
                "vw -C /projects/repo doctor --json",
            ],
        ),
        _ => unreachable!("every public command has documented semantics: {name}"),
    };
    Semantics {
        target,
        prerequisites,
        effects,
        examples,
    }
}

pub(super) fn document_command(mut command: ClapCommand) -> ClapCommand {
    command = command.after_help("Workflow:\n  vw init\n  vw switch feature/topic --json\n  vw exec feature/topic --json -- cargo test\n\nDiscover command details:\n  vw <command> --help\n  vw describe <command> --json\n\nJSON errors preserve partial results. Inspect error.execution before retrying.\nExplicit --help / --version and bare vw print text and exit successfully.");
    for name in COMMAND_NAMES {
        let spec = semantics(name);
        let prerequisites = if spec.prerequisites.is_empty() {
            "None".to_owned()
        } else {
            spec.prerequisites.join("; ")
        };
        let help = format!(
            "Target: {}\nPrerequisites: {}\nEffects: {}\n\nExamples:\n  {}",
            spec.target,
            prerequisites,
            spec.effects.join("; "),
            spec.examples.join("\n  ")
        );
        command = command.mut_subcommand(name, |subcommand| subcommand.after_help(help));
    }
    command
}

pub fn describe(selected: Option<&str>) -> Result<Value, CliError> {
    if let Some(name) = selected
        && !COMMAND_NAMES.contains(&name)
    {
        return Err(CliError::new(
            ErrorCode::UnknownCommand,
            format!("unknown public command: {name}"),
        )
        .with_details(std::collections::BTreeMap::from([(
            "commands".to_owned(),
            json!(COMMAND_NAMES),
        )])));
    }
    let mut root = super::clap_command();
    root.build();
    let commands = root
        .get_subcommands()
        .filter(|command| {
            COMMAND_NAMES.contains(&command.get_name())
                && selected.is_none_or(|name| name == command.get_name())
        })
        .map(describe_command)
        .collect::<Vec<_>>();
    Ok(json!({
        "outputSchemaVersion": SCHEMA_VERSION,
        "binaries": ["vw", "vde-worktree"],
        "commands": commands,
        "envelopeSchema": envelope_schema(),
    }))
}

fn describe_command(command: &ClapCommand) -> Value {
    let name = command.get_name();
    let spec = semantics(name);
    let arguments = command.get_arguments().filter(|argument| !argument.is_hide_set()).map(|argument| {
        let range = argument.get_num_args().expect("Clap definition is built");
        json!({
            "id": argument.get_id().as_str(),
            "completion": COMPLETION_BINDINGS.iter().find(|binding| binding.command == name && binding.argument == argument.get_id().as_str()).map(|binding| binding.kind),
            "long": argument.get_long(),
            "short": argument.get_short().map(|value| value.to_string()),
            "description": argument.get_help().map(ToString::to_string),
            "required": argument.is_required_set(),
            "global": argument.is_global_set(),
            "positional": argument.is_positional(),
            "afterDoubleDash": argument.is_last_set(),
            "minValues": range.min_values(),
            "maxValues": (range.max_values() != usize::MAX).then_some(range.max_values()),
            "repeatable": matches!(argument.get_action(), ArgAction::Append | ArgAction::Count),
            "defaults": argument.get_default_values().iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>(),
            "choices": argument.get_possible_values().iter().map(clap::builder::PossibleValue::get_name).collect::<Vec<_>>(),
            "conflictsWith": command.get_arg_conflicts_with(argument).iter().map(|value| value.get_id().as_str()).collect::<Vec<_>>(),
        })
    }).collect::<Vec<_>>();
    json!({
        "name": name,
        "description": command.get_about().map(ToString::to_string),
        "target": spec.target,
        "requiresRepository": !matches!(name, "completion" | "describe" | "doctor"),
        "requiresInitialization": matches!(name, "new" | "switch" | "get" | "mv" | "del" | "gone" | "adopt" | "extract" | "absorb" | "unabsorb" | "use" | "lock" | "unlock"),
        "prerequisites": spec.prerequisites,
        "effects": spec.effects,
        "examples": spec.examples,
        "arguments": arguments,
        "constraints": semantic_constraints(name),
        "dataSchema": data_schema(name),
    })
}

fn semantic_constraints(command: &str) -> Vec<Value> {
    let mut constraints = vec![
        json!({"when": "no_hooks", "requires": ["allow_unsafe"]}),
        json!({"lastWins": ["hooks", "no_hooks"]}),
        json!({"lastWins": ["gh", "no_gh"]}),
    ];
    if matches!(command, "path" | "exec") {
        constraints.push(json!({"exactlyOne": ["branch", "worktree"]}));
    }
    if !matches!(command, "status" | "path" | "exec" | "copy" | "link") {
        constraints.push(json!({"unsupported": ["worktree"]}));
    }
    match command {
        "list" => constraints.push(json!({"when": "monitor", "requires": ["json", "no_gh"], "conflictsWith": ["gh"]})),
        "del" => constraints.push(json!({"when": "nonInteractive", "whenAny": ["force", "force_dirty", "allow_unpushed", "force_unmerged", "force_locked"], "requires": ["allow_unsafe"]})),
        "absorb" | "unabsorb" | "use" => constraints.push(json!({"when": "nonInteractive", "requires": ["allow_agent", "allow_unsafe"]})),
        _ => {}
    }
    constraints
}

fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    let required = fields.iter().map(|(key, _)| *key).collect::<Vec<_>>();
    let properties = fields
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect::<serde_json::Map<_, _>>();
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}
fn scalar(kind: &str) -> Value {
    json!({"type": kind})
}
fn nullable(schema: Value) -> Value {
    Value::Object(serde_json::Map::from_iter([(
        "anyOf".to_owned(),
        Value::Array(vec![schema, scalar("null")]),
    )]))
}
fn array(items: Value) -> Value {
    let mut schema = json!({"type": "array"});
    schema["items"] = items;
    schema
}
fn strings() -> Value {
    array(scalar("string"))
}
fn values(options: &[&str]) -> Value {
    json!({"enum": options})
}

fn execution_schema() -> Value {
    object([
        (
            "phase",
            values(&[
                "parse",
                "resolve",
                "configure",
                "lock",
                "recover",
                "preflight",
                "stage",
                "preHook",
                "apply",
                "finalize",
                "postHook",
                "process",
                "unknown",
            ]),
        ),
        (
            "state",
            values(&[
                "notStarted",
                "rolledBack",
                "applied",
                "partial",
                "recoveryRequired",
                "unknown",
            ]),
        ),
        ("completed", strings()),
        ("recovery", scalar("object")),
    ])
}
fn diagnostic_schema() -> Value {
    object([
        ("code", scalar("string")),
        ("message", scalar("string")),
        ("details", scalar("object")),
        ("execution", execution_schema()),
    ])
}
fn worktree_schema() -> Value {
    object([
        ("branch", nullable(scalar("string"))),
        ("path", scalar("string")),
        ("head", scalar("string")),
        ("dirty", scalar("boolean")),
        (
            "locked",
            object([
                ("value", scalar("boolean")),
                ("owner", nullable(scalar("string"))),
                ("reason", nullable(scalar("string"))),
            ]),
        ),
        (
            "merged",
            object([
                ("byAncestry", nullable(scalar("boolean"))),
                ("byPR", nullable(scalar("boolean"))),
                ("overall", nullable(scalar("boolean"))),
            ]),
        ),
        (
            "pr",
            object([
                (
                    "status",
                    nullable(values(&[
                        "none",
                        "open",
                        "merged",
                        "closed_unmerged",
                        "unknown",
                    ])),
                ),
                ("url", nullable(scalar("string"))),
                (
                    "diagnostic",
                    nullable(object([
                        (
                            "reason",
                            values(&[
                                "not_observed",
                                "disabled",
                                "dependency_missing",
                                "authentication_required",
                                "command_failed",
                                "timed_out",
                                "invalid_response",
                            ]),
                        ),
                        ("message", nullable(scalar("string"))),
                        ("exitCode", nullable(scalar("integer"))),
                    ])),
                ),
            ]),
        ),
        (
            "upstream",
            object([
                ("ahead", nullable(scalar("integer"))),
                ("behind", nullable(scalar("integer"))),
                ("remote", nullable(scalar("string"))),
            ]),
        ),
    ])
}

pub fn envelope_schema() -> Value {
    let mut schema = object([
        ("schemaVersion", json!({"const": SCHEMA_VERSION})),
        ("command", scalar("string")),
        ("status", values(&["ok", "error"])),
        ("repoRoot", nullable(scalar("string"))),
        ("data", nullable(scalar("object"))),
        ("error", nullable(diagnostic_schema())),
        ("warnings", array(diagnostic_schema())),
    ]);
    schema["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    schema
}

fn repository_schema() -> Value {
    object([
        ("repoRoot", scalar("string")),
        ("currentWorktreeRoot", scalar("string")),
        ("gitCommonDir", scalar("string")),
    ])
}
fn effective_config_schema() -> Value {
    object([
        ("paths", object([("worktreeRoot", scalar("string"))])),
        (
            "git",
            object([
                ("baseBranch", nullable(scalar("string"))),
                ("baseRemote", scalar("string")),
            ]),
        ),
        ("github", object([("enabled", scalar("boolean"))])),
        (
            "hooks",
            object([
                ("enabled", scalar("boolean")),
                ("timeoutMs", scalar("integer")),
            ]),
        ),
        ("locks", object([("timeoutMs", scalar("integer"))])),
        (
            "list",
            object([(
                "table",
                object([
                    (
                        "columns",
                        array(values(&[
                            "branch", "dirty", "merged", "pr", "locked", "ahead", "behind", "path",
                        ])),
                    ),
                    (
                        "path",
                        object([
                            ("truncate", values(&["auto", "never"])),
                            ("minWidth", scalar("integer")),
                        ]),
                    ),
                ]),
            )]),
        ),
        (
            "selector",
            object([(
                "cd",
                object([
                    ("prompt", scalar("string")),
                    ("surface", values(&["auto", "inline", "tmux-popup"])),
                    ("tmuxPopupOpts", scalar("string")),
                    ("fzf", object([("extraArgs", strings())])),
                ]),
            )]),
        ),
    ])
}

fn configuration_schema() -> Value {
    object([
        ("loadedFiles", strings()),
        (
            "sources",
            json!({"type": "object", "additionalProperties": array(json!({"anyOf": [
                object([("kind", values(&["default"]))]),
                object([("kind", values(&["file"])), ("path", scalar("string"))]),
                object([("kind", values(&["commandLine"])), ("argument", scalar("string"))]),
            ]}))}),
        ),
        ("effective", effective_config_schema()),
    ])
}
fn pending_recoveries_schema() -> Value {
    array(object([
        ("transactionId", scalar("string")),
        ("path", scalar("string")),
        ("journalState", values(&["valid", "invalid", "missing"])),
        (
            "phase",
            nullable(values(&[
                "prepared",
                "branchRenamed",
                "worktreeMoved",
                "commitForward",
                "committed",
            ])),
        ),
        ("fromBranch", nullable(scalar("string"))),
        ("toBranch", nullable(scalar("string"))),
        ("sourcePath", nullable(scalar("string"))),
        ("targetPath", nullable(scalar("string"))),
        ("problem", nullable(scalar("string"))),
    ]))
}

/// Schema for successful and partial-result data; ordinary failures use null data.
#[allow(clippy::too_many_lines)]
pub fn data_schema(command: &str) -> Value {
    let path = || object([("branch", scalar("string")), ("path", scalar("string"))]);
    let adopt_candidate = || {
        object([
            ("branch", scalar("string")),
            ("fromPath", scalar("string")),
            ("toPath", scalar("string")),
        ])
    };
    match command {
        "init" => object([("alreadyInitialized", scalar("boolean"))]),
        "list" => object([
            ("baseBranch", nullable(scalar("string"))),
            ("managedWorktreeRoot", scalar("string")),
            ("worktrees", array(worktree_schema())),
        ]),
        "status" => object([("worktree", worktree_schema())]),
        "new" | "mv" | "del" | "extract" | "use" => path(),
        "switch" | "get" => object([
            ("branch", scalar("string")),
            ("path", scalar("string")),
            ("disposition", values(&["created", "existing"])),
        ]),
        "gone" => object([
            ("dryRun", scalar("boolean")),
            ("candidates", strings()),
            ("deleted", strings()),
            (
                "failed",
                array(object([
                    ("branch", scalar("string")),
                    ("path", scalar("string")),
                    ("phase", scalar("string")),
                    ("code", scalar("string")),
                    ("message", scalar("string")),
                    ("details", scalar("object")),
                    ("execution", execution_schema()),
                ])),
            ),
        ]),
        "adopt" => object([
            ("dryRun", scalar("boolean")),
            ("managedWorktreeRoot", scalar("string")),
            ("candidates", array(adopt_candidate())),
            ("moved", array(adopt_candidate())),
            (
                "skipped",
                array(object([
                    ("branch", nullable(scalar("string"))),
                    ("fromPath", scalar("string")),
                    ("toPath", nullable(scalar("string"))),
                    ("reason", scalar("string")),
                ])),
            ),
            (
                "failed",
                array(object([
                    ("branch", scalar("string")),
                    ("fromPath", scalar("string")),
                    ("toPath", scalar("string")),
                    ("code", scalar("string")),
                    ("message", scalar("string")),
                    ("details", scalar("object")),
                    ("execution", execution_schema()),
                ])),
            ),
        ]),
        "absorb" | "unabsorb" => object([
            ("branch", scalar("string")),
            ("path", scalar("string")),
            ("sourcePath", scalar("string")),
            ("stashed", scalar("boolean")),
            ("stashRef", nullable(scalar("string"))),
            ("direction", values(&["absorb", "unabsorb"])),
        ]),
        "path" => object([
            ("branch", nullable(scalar("string"))),
            ("path", scalar("string")),
        ]),
        "exec" => object([
            ("branch", nullable(scalar("string"))),
            ("path", scalar("string")),
            ("childExitCode", scalar("integer")),
            ("childStdout", scalar("string")),
            ("childStderr", scalar("string")),
        ]),
        "invoke" => object([("hook", scalar("string"))]),
        "copy" | "link" => json!({"anyOf": [
            object([(if command == "copy" { "copied" } else { "linked" }, strings()), ("worktreePath", scalar("string"))]),
            object([("attempted", strings()), ("worktreePath", scalar("string")), ("transactionState", values(&["recovery-required"]))]),
        ]}),
        "lock" => object([
            ("branch", scalar("string")),
            ("owner", scalar("string")),
            ("reason", scalar("string")),
        ]),
        "unlock" => object([("branch", scalar("string"))]),
        "cd" => object([("path", scalar("string"))]),
        "completion" => json!({"anyOf": [
            object([("shell", values(&["zsh", "fish"])), ("installed", json!({"const": false})), ("script", scalar("string"))]),
            object([("shell", values(&["zsh", "fish"])), ("installed", scalar("boolean")), ("path", scalar("string"))]),
        ]}),
        "describe" => object([
            ("outputSchemaVersion", json!({"const": SCHEMA_VERSION})),
            ("binaries", strings()),
            ("commands", array(scalar("object"))),
            ("envelopeSchema", scalar("object")),
        ]),
        "context" => object([
            ("executionDirectory", scalar("string")),
            ("repository", repository_schema()),
            ("initialized", scalar("boolean")),
            ("managedWorktreeRoot", scalar("string")),
            ("baseBranch", nullable(scalar("string"))),
            ("baseBranchError", nullable(diagnostic_schema())),
            ("config", configuration_schema()),
            ("pendingRecoveries", pending_recoveries_schema()),
        ]),
        "doctor" => object([
            ("executionDirectory", scalar("string")),
            ("repository", nullable(repository_schema())),
            ("healthy", scalar("boolean")),
            ("config", nullable(configuration_schema())),
            (
                "checks",
                array(object([
                    ("name", scalar("string")),
                    ("status", values(&["ok", "warning", "error", "skipped"])),
                    ("message", scalar("string")),
                    ("details", scalar("object")),
                ])),
            ),
            ("pendingRecoveries", pending_recoveries_schema()),
        ]),
        _ => unreachable!("every public command has a data schema: {command}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_public_command_and_argument_has_documentation() {
        let mut root = super::super::clap_command();
        root.build();
        let actual = root
            .get_subcommands()
            .filter(|command| !command.is_hide_set() && command.get_name() != "help")
            .map(ClapCommand::get_name)
            .collect::<Vec<_>>();
        assert_eq!(actual, COMMAND_NAMES);
        let description = describe(None).unwrap();
        for command in description["commands"].as_array().unwrap() {
            assert!(!command["effects"].as_array().unwrap().is_empty());
            assert!(!command["examples"].as_array().unwrap().is_empty());
            for argument in command["arguments"].as_array().unwrap() {
                assert!(
                    argument["description"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty()),
                    "{}: {argument}",
                    command["name"]
                );
            }
        }
    }

    #[test]
    fn describe_exports_real_clap_constraints_and_child_argv_boundary() {
        let value = describe(Some("exec")).unwrap();
        let argv = value["commands"][0]["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["id"] == "argv")
            .unwrap();
        assert_eq!(argv["required"], true);
        assert_eq!(argv["afterDoubleDash"], true);
        assert_eq!(argv["minValues"], 1);
        assert_eq!(
            describe(Some("missing")).unwrap_err().code,
            ErrorCode::UnknownCommand
        );
    }
}
