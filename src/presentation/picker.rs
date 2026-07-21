use std::path::{Path, PathBuf};

use unicode_width::UnicodeWidthStr;

use crate::presentation::theme::{CatppuccinMocha, ColorPolicy, SemanticStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerMergedState {
    Base,
    Merged,
    Unmerged,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerWorktree {
    pub branch: Option<String>,
    pub path: PathBuf,
    pub current: bool,
    pub dirty: bool,
    pub merged: PickerMergedState,
    pub locked: bool,
    pub remote: Option<String>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub lock_owner: Option<String>,
    pub lock_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerCandidate {
    pub line: String,
    pub path: PathBuf,
}

pub fn build_picker_candidates(
    worktrees: &[PickerWorktree],
    home: Option<&Path>,
    color: ColorPolicy,
) -> Vec<PickerCandidate> {
    let branch_width = worktrees
        .iter()
        .map(branch_label)
        .map(|label| UnicodeWidthStr::width(label.as_str()))
        .max()
        .unwrap_or(0);

    worktrees
        .iter()
        .map(|worktree| {
            let label = branch_label(worktree);
            let branch = pad_to_width(&label, branch_width);
            let branch_style = if worktree.branch.is_none() {
                SemanticStyle::BranchDetached
            } else if worktree.current {
                SemanticStyle::BranchCurrent
            } else if worktree.merged == PickerMergedState::Base {
                SemanticStyle::Base
            } else {
                SemanticStyle::Branch
            };
            let display = format!(
                "{}  {} {} {} {} {}",
                CatppuccinMocha::paint(branch_style, &branch, color),
                badge(
                    if worktree.dirty { "DIRTY" } else { "CLEAN" },
                    5,
                    if worktree.dirty {
                        SemanticStyle::Attention
                    } else {
                        SemanticStyle::Safe
                    },
                    color,
                ),
                CatppuccinMocha::paint(SemanticStyle::Muted, "|", color),
                badge(
                    merged_label(worktree.merged),
                    8,
                    merged_style(worktree.merged),
                    color,
                ),
                CatppuccinMocha::paint(SemanticStyle::Muted, "|", color),
                badge(
                    if worktree.locked { "LOCK" } else { "OPEN" },
                    4,
                    if worktree.locked {
                        SemanticStyle::Attention
                    } else {
                        SemanticStyle::Muted
                    },
                    color,
                ),
            );
            let preview = encode_preview_field(&build_preview(worktree, home, color));
            let path_field = sanitize_field(&worktree.path.to_string_lossy());
            PickerCandidate {
                line: format!("{display}\t{path_field}\t{preview}"),
                path: worktree.path.clone(),
            }
        })
        .collect()
}

pub fn home_relative_path(path: &Path, home: Option<&Path>) -> String {
    let Some(home) = home.filter(|home| !home.as_os_str().is_empty()) else {
        return path.to_string_lossy().into_owned();
    };
    if path == home {
        return "~".to_owned();
    }
    path.strip_prefix(home).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |relative| format!("~/{}", relative.to_string_lossy()),
    )
}

fn build_preview(worktree: &PickerWorktree, home: Option<&Path>, color: ColorPolicy) -> String {
    let branch = sanitize_preview(worktree.branch.as_deref().unwrap_or("(detached)"));
    let branch_style = if worktree.branch.is_none() {
        SemanticStyle::BranchDetached
    } else if worktree.merged == PickerMergedState::Base {
        SemanticStyle::Base
    } else {
        SemanticStyle::Branch
    };
    let divider = CatppuccinMocha::paint(
        SemanticStyle::Muted,
        "----------------------------------------",
        color,
    );
    let mut lines = vec![
        CatppuccinMocha::paint(SemanticStyle::Header, "WORKTREE", color),
        divider.clone(),
        preview_line("Branch ", &branch, branch_style, color),
        preview_line(
            "Path   ",
            &sanitize_preview(&home_relative_path(&worktree.path, home)),
            SemanticStyle::Path,
            color,
        ),
        String::new(),
    ];
    lines.extend(status_preview_lines(worktree, &divider, color));

    if worktree.locked {
        lines.push(String::new());
        lines.push(CatppuccinMocha::paint(SemanticStyle::Header, "LOCK", color));
        lines.push(divider);
        if let Some(reason) = worktree
            .lock_reason
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            lines.push(preview_line(
                "Reason ",
                &sanitize_preview(reason),
                SemanticStyle::Value,
                color,
            ));
        }
        if let Some(owner) = worktree
            .lock_owner
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            lines.push(preview_line(
                "Owner  ",
                &sanitize_preview(owner),
                SemanticStyle::Value,
                color,
            ));
        }
    }
    lines.join("\n")
}

fn status_preview_lines(
    worktree: &PickerWorktree,
    divider: &str,
    color: ColorPolicy,
) -> Vec<String> {
    vec![
        CatppuccinMocha::paint(SemanticStyle::Header, "STATUS", color),
        divider.to_owned(),
        preview_line(
            "Dirty  ",
            if worktree.dirty { "[DIRTY]" } else { "[CLEAN]" },
            if worktree.dirty {
                SemanticStyle::Attention
            } else {
                SemanticStyle::Safe
            },
            color,
        ),
        preview_line(
            "Locked ",
            if worktree.locked {
                "[LOCKED]"
            } else {
                "[OPEN]"
            },
            if worktree.locked {
                SemanticStyle::Attention
            } else {
                SemanticStyle::Safe
            },
            color,
        ),
        preview_line(
            "Merged ",
            merged_label(worktree.merged),
            merged_style(worktree.merged),
            color,
        ),
        preview_line(
            "Remote ",
            worktree.remote.as_deref().unwrap_or("none"),
            if worktree.remote.is_some() {
                SemanticStyle::Value
            } else {
                SemanticStyle::Muted
            },
            color,
        ),
        preview_line(
            "Ahead  ",
            &count_label(worktree.ahead),
            count_style(worktree.ahead, true),
            color,
        ),
        preview_line(
            "Behind ",
            &count_label(worktree.behind),
            count_style(worktree.behind, false),
            color,
        ),
    ]
}

fn preview_line(label: &str, value: &str, style: SemanticStyle, color: ColorPolicy) -> String {
    format!(
        "  {}: {}",
        CatppuccinMocha::paint(SemanticStyle::PreviewLabel, label, color),
        CatppuccinMocha::paint(style, value, color)
    )
}

fn branch_label(worktree: &PickerWorktree) -> String {
    format!(
        "{} {}",
        if worktree.current { '*' } else { ' ' },
        sanitize_field(worktree.branch.as_deref().unwrap_or("(detached)"))
    )
}

fn badge(value: &str, width: usize, style: SemanticStyle, color: ColorPolicy) -> String {
    CatppuccinMocha::paint(style, &pad_to_width(value, width), color)
}

fn pad_to_width(value: &str, width: usize) -> String {
    let padding = width.saturating_sub(UnicodeWidthStr::width(value));
    format!("{value}{}", " ".repeat(padding))
}

const fn merged_label(state: PickerMergedState) -> &'static str {
    match state {
        PickerMergedState::Base => "BASE",
        PickerMergedState::Merged => "MERGED",
        PickerMergedState::Unmerged => "UNMERGED",
        PickerMergedState::Unknown => "UNKNOWN",
    }
}

const fn merged_style(state: PickerMergedState) -> SemanticStyle {
    match state {
        PickerMergedState::Base => SemanticStyle::Base,
        PickerMergedState::Merged => SemanticStyle::Safe,
        PickerMergedState::Unmerged => SemanticStyle::Attention,
        PickerMergedState::Unknown => SemanticStyle::Unknown,
    }
}

fn count_label(value: Option<u64>) -> String {
    value.map_or_else(|| "UNKNOWN".to_owned(), |value| value.to_string())
}

const fn count_style(value: Option<u64>, ahead: bool) -> SemanticStyle {
    match value {
        Some(0) => SemanticStyle::Safe,
        Some(_) if ahead => SemanticStyle::Attention,
        None | Some(_) => SemanticStyle::Unknown,
    }
}

fn encode_preview_field(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\u{1b}', "\\033")
        .replace('\t', " ")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\\n")
}

fn sanitize_field(value: &str) -> String {
    sanitize_controls(value, false).trim().to_owned()
}

fn sanitize_preview(value: &str) -> String {
    sanitize_controls(value, true)
}

fn sanitize_controls(value: &str, preserve_tabs: bool) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\r' | '\n' => sanitized.push(' '),
            '\t' if !preserve_tabs => sanitized.push(' '),
            character if character.is_control() => {
                use std::fmt::Write as _;
                write!(sanitized, "\\u{{{:x}}}", character as u32)
                    .expect("writing to String cannot fail");
            }
            character => sanitized.push(character),
        }
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree(branch: &str, path: &str) -> PickerWorktree {
        PickerWorktree {
            branch: Some(branch.to_owned()),
            path: PathBuf::from(path),
            current: branch == "main",
            dirty: branch != "main",
            merged: if branch == "main" {
                PickerMergedState::Base
            } else {
                PickerMergedState::Unknown
            },
            locked: branch != "main",
            remote: Some("origin".to_owned()),
            ahead: Some(0),
            behind: Some(2),
            lock_owner: Some("agent".to_owned()),
            lock_reason: Some("review".to_owned()),
        }
    }

    fn plain() -> ColorPolicy {
        ColorPolicy {
            stream_is_terminal: false,
            json: false,
            no_color: false,
        }
    }

    #[test]
    fn rows_have_aligned_unicode_branch_and_state_columns() {
        let candidates = build_picker_candidates(
            &[
                worktree("main", "/home/yuki/repo"),
                worktree("機能/追加", "/home/yuki/repo/.worktree/ja"),
            ],
            Some(Path::new("/home/yuki")),
            plain(),
        );
        let first = candidates[0].line.split('\t').next().unwrap();
        let second = candidates[1].line.split('\t').next().unwrap();
        assert_eq!(
            UnicodeWidthStr::width(first),
            UnicodeWidthStr::width(second)
        );
        assert!(candidates[0].line.contains("~/repo"));
        assert!(candidates[1].line.contains("LOCK\\n"));
        assert_eq!(
            candidates[1].path,
            Path::new("/home/yuki/repo/.worktree/ja")
        );
    }

    #[test]
    fn preview_snapshot_contains_status_and_lock_metadata() {
        let candidate = build_picker_candidates(
            &[worktree("feature/demo", "/home/yuki/repo/.worktree/demo")],
            Some(Path::new("/home/yuki")),
            plain(),
        )
        .remove(0);
        let preview = candidate.line.split('\t').nth(2).unwrap();
        assert_eq!(
            preview,
            "WORKTREE\\n----------------------------------------\\n  Branch : feature/demo\\n  Path   : ~/repo/.worktree/demo\\n\\nSTATUS\\n----------------------------------------\\n  Dirty  : [DIRTY]\\n  Locked : [LOCKED]\\n  Merged : UNKNOWN\\n  Remote : origin\\n  Ahead  : 0\\n  Behind : 2\\n\\nLOCK\\n----------------------------------------\\n  Reason : review\\n  Owner  : agent"
        );
    }

    #[test]
    fn user_controlled_fields_cannot_inject_ansi() {
        let mut item = worktree("feature/evil\u{1b}[31m", "/repo/evil\u{1b}[2J");
        item.lock_reason = Some("reason\u{1b}[5m".to_owned());
        let candidates = build_picker_candidates(&[item], None, plain());

        assert!(!candidates[0].line.contains('\u{1b}'));
        assert!(candidates[0].line.contains("\\u{1b}"));
    }

    #[test]
    fn json_and_no_color_picker_models_have_no_ansi() {
        for color in [
            ColorPolicy {
                stream_is_terminal: true,
                json: true,
                no_color: false,
            },
            ColorPolicy {
                stream_is_terminal: true,
                json: false,
                no_color: true,
            },
        ] {
            let candidates = build_picker_candidates(&[worktree("main", "/repo")], None, color);
            assert!(!candidates[0].line.contains('\u{1b}'));
        }
    }
}
