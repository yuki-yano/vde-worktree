use unicode_width::UnicodeWidthStr;

use crate::presentation::theme::{CatppuccinMocha, ColorPolicy, SemanticStyle};
use crate::state::config::{ListPathTruncate, ListTableColumn};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergedCellState {
    Base,
    Merged,
    Unmerged,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrCellState {
    Base,
    None,
    Open,
    Merged,
    ClosedUnmerged,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListTableRow {
    pub branch: Option<String>,
    pub current: bool,
    pub dirty: bool,
    pub merged: MergedCellState,
    pub pr: PrCellState,
    pub locked: bool,
    pub ahead: Option<i64>,
    pub behind: Option<i64>,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticCell {
    pub text: String,
    pub style: SemanticStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRenderOptions {
    pub columns: Vec<ListTableColumn>,
    pub terminal_width: Option<usize>,
    pub path_truncate: ListPathTruncate,
    pub path_min_width: usize,
    pub full_path: bool,
    pub color: ColorPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedTable {
    pub plain: String,
    pub styled: String,
    pub display_width: usize,
}

pub fn semantic_cells(row: &ListTableRow, columns: &[ListTableColumn]) -> Vec<SemanticCell> {
    columns
        .iter()
        .map(|column| semantic_cell(row, *column))
        .collect()
}

pub fn render_table(rows: &[ListTableRow], options: &TableRenderOptions) -> RenderedTable {
    if options.columns.is_empty() {
        return RenderedTable {
            plain: String::new(),
            styled: String::new(),
            display_width: 0,
        };
    }

    let headers = options
        .columns
        .iter()
        .map(|column| SemanticCell {
            text: column_name(*column).to_owned(),
            style: SemanticStyle::Header,
        })
        .collect::<Vec<_>>();
    let mut body = rows
        .iter()
        .map(|row| semantic_cells(row, &options.columns))
        .collect::<Vec<_>>();
    let mut widths = content_widths(&headers, &body);

    if let Some(path_index) = options
        .columns
        .iter()
        .position(|column| *column == ListTableColumn::Path)
        && !options.full_path
        && options.path_truncate == ListPathTruncate::Auto
        && let Some(terminal_width) = options.terminal_width
    {
        let non_path_width = widths
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != path_index)
            .map(|(_, width)| *width)
            .sum::<usize>();
        let framing_width = options.columns.len() * 3 + 1;
        let available = terminal_width.saturating_sub(non_path_width + framing_width);
        let minimum = options
            .path_min_width
            .max(UnicodeWidthStr::width(headers[path_index].text.as_str()));
        let path_width = widths[path_index].min(available.max(minimum));
        widths[path_index] = path_width;
        for row in &mut body {
            row[path_index].text = truncate_with_ellipsis(&row[path_index].text, path_width);
        }
    }

    let plain = draw_table(&headers, &body, &widths, None);
    let styled = draw_table(&headers, &body, &widths, Some(options.color));
    RenderedTable {
        display_width: widths.iter().sum::<usize>() + options.columns.len() * 3 + 1,
        plain,
        styled,
    }
}

fn semantic_cell(row: &ListTableRow, column: ListTableColumn) -> SemanticCell {
    match column {
        ListTableColumn::Branch => {
            let detached = row.branch.is_none();
            SemanticCell {
                text: format!(
                    "{} {}",
                    if row.current { '*' } else { ' ' },
                    sanitize_cell(row.branch.as_deref().unwrap_or("(detached)"))
                ),
                style: if detached {
                    SemanticStyle::BranchDetached
                } else if row.current {
                    SemanticStyle::BranchCurrent
                } else {
                    SemanticStyle::Branch
                },
            }
        }
        ListTableColumn::Dirty => SemanticCell {
            text: if row.dirty { "dirty" } else { "clean" }.to_owned(),
            style: if row.dirty {
                SemanticStyle::Dirty
            } else {
                SemanticStyle::Safe
            },
        },
        ListTableColumn::Merged => {
            let (text, style) = match row.merged {
                MergedCellState::Base => ("-", SemanticStyle::Base),
                MergedCellState::Merged => ("merged", SemanticStyle::Safe),
                MergedCellState::Unmerged => ("unmerged", SemanticStyle::Attention),
                MergedCellState::Unknown => ("unknown", SemanticStyle::Unknown),
            };
            SemanticCell {
                text: text.to_owned(),
                style,
            }
        }
        ListTableColumn::Pr => {
            let (text, style) = match row.pr {
                PrCellState::Base => ("-", SemanticStyle::Base),
                PrCellState::None => ("none", SemanticStyle::Muted),
                PrCellState::Open => ("open", SemanticStyle::Value),
                PrCellState::Merged => ("merged", SemanticStyle::Safe),
                PrCellState::ClosedUnmerged => ("closed_unmerged", SemanticStyle::Attention),
                PrCellState::Unknown => ("unknown", SemanticStyle::Unknown),
            };
            SemanticCell {
                text: text.to_owned(),
                style,
            }
        }
        ListTableColumn::Locked => SemanticCell {
            text: if row.locked { "locked" } else { "-" }.to_owned(),
            style: if row.locked {
                SemanticStyle::Attention
            } else {
                SemanticStyle::Muted
            },
        },
        ListTableColumn::Ahead => count_cell(row.ahead, true),
        ListTableColumn::Behind => count_cell(row.behind, false),
        ListTableColumn::Path => SemanticCell {
            text: sanitize_cell(&row.path),
            style: SemanticStyle::Path,
        },
    }
}

fn count_cell(value: Option<i64>, ahead: bool) -> SemanticCell {
    match value {
        None => SemanticCell {
            text: "-".to_owned(),
            style: SemanticStyle::Muted,
        },
        Some(0) => SemanticCell {
            text: "0".to_owned(),
            style: SemanticStyle::Safe,
        },
        Some(value) if value > 0 => SemanticCell {
            text: value.to_string(),
            style: if ahead {
                SemanticStyle::Attention
            } else {
                SemanticStyle::Unknown
            },
        },
        Some(value) => SemanticCell {
            text: value.to_string(),
            style: SemanticStyle::Unknown,
        },
    }
}

fn content_widths(headers: &[SemanticCell], rows: &[Vec<SemanticCell>]) -> Vec<usize> {
    headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter().fold(
                UnicodeWidthStr::width(header.text.as_str()),
                |width, row| width.max(UnicodeWidthStr::width(row[index].text.as_str())),
            )
        })
        .collect()
}

fn draw_table(
    headers: &[SemanticCell],
    rows: &[Vec<SemanticCell>],
    widths: &[usize],
    color: Option<ColorPolicy>,
) -> String {
    let mut lines = Vec::with_capacity(rows.len() + 4);
    lines.push(draw_border('┌', '┬', '┐', widths, color));
    lines.push(draw_row(headers, widths, color));
    lines.push(draw_border('├', '┼', '┤', widths, color));
    lines.extend(rows.iter().map(|row| draw_row(row, widths, color)));
    lines.push(draw_border('└', '┴', '┘', widths, color));
    lines.join("\n")
}

fn draw_border(
    left: char,
    middle: char,
    right: char,
    widths: &[usize],
    color: Option<ColorPolicy>,
) -> String {
    let mut line = String::new();
    line.push(left);
    for (index, width) in widths.iter().enumerate() {
        line.push_str(&"─".repeat(width + 2));
        line.push(if index + 1 == widths.len() {
            right
        } else {
            middle
        });
    }
    color.map_or(line.clone(), |policy| {
        CatppuccinMocha::paint(SemanticStyle::Muted, &line, policy)
    })
}

fn draw_row(cells: &[SemanticCell], widths: &[usize], color: Option<ColorPolicy>) -> String {
    let policy = color.unwrap_or(ColorPolicy {
        stream_is_terminal: false,
        json: false,
        no_color: false,
    });
    let border = color.map_or_else(
        || "│".to_owned(),
        |_| CatppuccinMocha::paint(SemanticStyle::Muted, "│", policy),
    );
    let mut line = border.clone();
    for (cell, width) in cells.iter().zip(widths) {
        let painted = color.map_or_else(
            || cell.text.clone(),
            |_| CatppuccinMocha::paint(cell.style, &cell.text, policy),
        );
        let padding = width.saturating_sub(UnicodeWidthStr::width(cell.text.as_str()));
        line.push(' ');
        line.push_str(&painted);
        line.push_str(&" ".repeat(padding + 1));
        line.push_str(&border);
    }
    line
}

fn truncate_with_ellipsis(value: &str, width: usize) -> String {
    if UnicodeWidthStr::width(value) <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_owned();
    }

    let content_width = width - 1;
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let candidate_end = index + character.len_utf8();
        if UnicodeWidthStr::width(&value[..candidate_end]) > content_width {
            break;
        }
        end = candidate_end;
    }
    format!("{}…", &value[..end])
}

fn sanitize_cell(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\t' => sanitized.push(' '),
            '\r' | '\n' => sanitized.push_str("\\n"),
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

const fn column_name(column: ListTableColumn) -> &'static str {
    match column {
        ListTableColumn::Branch => "branch",
        ListTableColumn::Dirty => "dirty",
        ListTableColumn::Merged => "merged",
        ListTableColumn::Pr => "pr",
        ListTableColumn::Locked => "locked",
        ListTableColumn::Ahead => "ahead",
        ListTableColumn::Behind => "behind",
        ListTableColumn::Path => "path",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> ListTableRow {
        ListTableRow {
            branch: Some("feature/日本語".to_owned()),
            current: true,
            dirty: true,
            merged: MergedCellState::Unmerged,
            pr: PrCellState::Open,
            locked: false,
            ahead: Some(2),
            behind: Some(1),
            path: "~/repos/長い名前/worktree".to_owned(),
        }
    }

    fn policy(enabled: bool) -> ColorPolicy {
        ColorPolicy {
            stream_is_terminal: enabled,
            json: false,
            no_color: false,
        }
    }

    #[test]
    fn semantic_styles_survive_column_reduction_and_reordering() {
        let columns = [
            ListTableColumn::Path,
            ListTableColumn::Dirty,
            ListTableColumn::Branch,
        ];
        let cells = semantic_cells(&row(), &columns);
        assert_eq!(
            cells
                .iter()
                .map(|cell| (cell.text.as_str(), cell.style))
                .collect::<Vec<_>>(),
            [
                ("~/repos/長い名前/worktree", SemanticStyle::Path),
                ("dirty", SemanticStyle::Dirty),
                ("* feature/日本語", SemanticStyle::BranchCurrent),
            ]
        );

        let rendered = render_table(
            &[row()],
            &TableRenderOptions {
                columns: columns.to_vec(),
                terminal_width: Some(120),
                path_truncate: ListPathTruncate::Auto,
                path_min_width: 12,
                full_path: false,
                color: policy(true),
            },
        );
        assert!(rendered.styled.contains("\u{1b}[38;2;116;199;236m"));
        assert!(rendered.styled.contains("\u{1b}[38;2;250;179;135m"));
        assert!(rendered.styled.contains("\u{1b}[38;2;203;166;247m"));
    }

    #[test]
    fn filesystem_text_cannot_inject_terminal_control_sequences() {
        let mut row = row();
        row.path = "/repo/evil\u{1b}[31m".to_owned();
        let cells = semantic_cells(&row, &[ListTableColumn::Path]);
        assert_eq!(cells[0].text, "/repo/evil\\u{1b}[31m");
        assert!(!cells[0].text.contains('\u{1b}'));
    }

    #[test]
    fn truncates_only_path_by_unicode_width() {
        let rendered = render_table(
            &[row()],
            &TableRenderOptions {
                columns: vec![ListTableColumn::Branch, ListTableColumn::Path],
                terminal_width: Some(45),
                path_truncate: ListPathTruncate::Auto,
                path_min_width: 8,
                full_path: false,
                color: policy(false),
            },
        );
        assert!(rendered.plain.contains("* feature/日本語"));
        assert!(rendered.plain.contains('…'));
        assert!(rendered.display_width <= 45);
        for line in rendered.plain.lines() {
            assert_eq!(UnicodeWidthStr::width(line), rendered.display_width);
        }
    }

    #[test]
    fn full_path_and_never_disable_truncation() {
        for (full_path, path_truncate) in [
            (true, ListPathTruncate::Auto),
            (false, ListPathTruncate::Never),
        ] {
            let rendered = render_table(
                &[row()],
                &TableRenderOptions {
                    columns: vec![ListTableColumn::Path],
                    terminal_width: Some(12),
                    path_truncate,
                    path_min_width: 8,
                    full_path,
                    color: policy(false),
                },
            );
            assert!(rendered.plain.contains("~/repos/長い名前/worktree"));
            assert!(!rendered.plain.contains('…'));
        }
    }

    #[test]
    fn color_policy_keeps_plain_and_machine_safe_snapshots_escape_free() {
        let options = |color| TableRenderOptions {
            columns: vec![ListTableColumn::Dirty, ListTableColumn::Path],
            terminal_width: None,
            path_truncate: ListPathTruncate::Auto,
            path_min_width: 12,
            full_path: false,
            color,
        };
        for color in [
            policy(false),
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
            let rendered = render_table(&[row()], &options(color));
            assert_eq!(rendered.plain, rendered.styled);
            assert!(!rendered.styled.contains('\u{1b}'));
        }
    }
}
