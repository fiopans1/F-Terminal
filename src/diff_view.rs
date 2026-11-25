use crate::layout::Rect;
use crate::render::CellMetrics;
use crate::render::quads::Quad;
use crate::review::{DiffLineKind, FileDiff, FileStatus, ReviewDetail};
use crate::term::color::{Theme, to_linear};
use crate::term::frame::{Frame, Row, Run};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffRowKind {
    File,
    Add,
    Remove,
    Context,
    Hunk,
    Meta,
    Separator,
}

#[derive(Clone, Debug)]
pub(crate) struct DiffRow {
    pub kind: DiffRowKind,
    pub text: String,
}

/// Flatten a review detail into display rows. This is pure and can run on a
/// worker before the result reaches the UI thread.
pub(crate) fn build_diff_rows(detail: &ReviewDetail) -> Vec<DiffRow> {
    let mut rows = Vec::new();

    for (index, file) in detail.files.iter().enumerate() {
        if index > 0 {
            rows.push(DiffRow {
                kind: DiffRowKind::Separator,
                text: String::new(),
            });
        }

        let status = match file.status {
            FileStatus::Modified => "M",
            FileStatus::Added => "A",
            FileStatus::Deleted => "D",
            FileStatus::Renamed => "R",
            FileStatus::Binary => "B",
        };
        let path = match &file.old_path {
            Some(old) if old != &file.path => format!("{old} → {}", file.path),
            _ => file.path.clone(),
        };
        let stats = if file.binary {
            "binary".to_owned()
        } else {
            format!("+{} -{}", file.additions, file.deletions)
        };
        rows.push(DiffRow {
            kind: DiffRowKind::File,
            text: format!("{status}  {path}   {stats}"),
        });

        if file.binary {
            rows.push(DiffRow {
                kind: DiffRowKind::Meta,
                text: "Binary file".to_owned(),
            });
            continue;
        }

        for line in &file.lines {
            let text = match line.kind {
                DiffLineKind::Add => {
                    format!(
                        "{:>5} {:>5} + {}",
                        "",
                        line.new.unwrap_or_default(),
                        line.text
                    )
                }
                DiffLineKind::Remove => format!(
                    "{:>5} {:>5} - {}",
                    line.old.unwrap_or_default(),
                    "",
                    line.text
                ),
                DiffLineKind::Context => format!(
                    "{:>5} {:>5}   {}",
                    line.old.unwrap_or_default(),
                    line.new.unwrap_or_default(),
                    line.text
                ),
                DiffLineKind::Hunk | DiffLineKind::Meta => line.text.clone(),
            };
            let kind = match line.kind {
                DiffLineKind::Add => DiffRowKind::Add,
                DiffLineKind::Remove => DiffRowKind::Remove,
                DiffLineKind::Context => DiffRowKind::Context,
                DiffLineKind::Hunk => DiffRowKind::Hunk,
                DiffLineKind::Meta => DiffRowKind::Meta,
            };
            rows.push(DiffRow { kind, text });
        }
    }

    rows
}

/// Build a renderable frame for one side of a unified diff.
pub fn build_file_frame(
    file: &FileDiff,
    area: Rect,
    metrics: CellMetrics,
    theme: &Theme,
    scroll_row: usize,
    scroll_col: usize,
    quads: &mut Vec<Quad>,
) -> Frame {
    let cols = (area.width / metrics.width).floor().max(1.0) as usize;
    let rows = (area.height / metrics.height).floor().max(1.0) as usize;
    let start = scroll_row.min(file.lines.len().saturating_sub(rows));

    let mut frame_rows = Vec::with_capacity(rows);
    for index in 0..rows {
        let mut row = Row::default();
        let line_index = start + index;
        if let Some(line) = file.lines.get(line_index) {
            let prefix = match line.kind {
                DiffLineKind::Add => {
                    format!("{:>5} {:>5} + ", "", line.new.unwrap_or_default())
                }
                DiffLineKind::Remove => {
                    format!("{:>5} {:>5} - ", line.old.unwrap_or_default(), "")
                }
                DiffLineKind::Context => format!(
                    "{:>5} {:>5}   ",
                    line.old.unwrap_or_default(),
                    line.new.unwrap_or_default()
                ),
                _ => String::new(),
            };

            let text: String = format!("{prefix}{}", line.text)
                .chars()
                .skip(scroll_col)
                .take(cols)
                .collect();

            if matches!(line.kind, DiffLineKind::Add | DiffLineKind::Remove) {
                let color = if line.kind == DiffLineKind::Add {
                    theme.diff_add_background
                } else {
                    theme.diff_remove_background
                };
                quads.push(Quad::new(
                    area.x,
                    area.y + index as f32 * metrics.height,
                    area.width,
                    metrics.height,
                    to_linear(color, 1.0),
                ));
            }

            if !text.is_empty() {
                let color = match line.kind {
                    DiffLineKind::Add => theme.diff_add_text,
                    DiffLineKind::Remove => theme.diff_remove_text,
                    DiffLineKind::Hunk => theme.diff_hunk_text,
                    DiffLineKind::Meta => theme.diff_meta_text,
                    DiffLineKind::Context => theme.foreground,
                };
                row.runs.push(Run {
                    range: 0..text.len(),
                    color,
                });
                row.text = text;
            }
        }
        frame_rows.push(row);
    }

    Frame {
        rows: frame_rows,
        cols,
        cursor: None,
        background: theme.background,
    }
}
