use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use crate::term::color::Theme;

/// A fully resolved view of the visible terminal grid, ready to be rendered.
/// The renderer does not know anything about `alacritty_terminal`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub rows: Vec<Row>,
    pub cols: usize,
    pub cursor: Option<Cursor>,
    /// Effective terminal background, including OSC 11 overrides.
    pub background: Rgb,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// Visible text of the row, with a trailing newline excluded.
    pub text: String,
    /// Foreground color runs, as byte ranges into `text`.
    pub runs: Vec<Run>,
    /// Background color runs, in columns.
    pub backgrounds: Vec<Background>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub range: std::ops::Range<usize>,
    pub color: Rgb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Background {
    pub start: usize,
    pub end: usize,
    pub color: Rgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShapeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShapeKind {
    Block,
    Beam,
    Underline,
}

/// Build a render-ready frame from the terminal state.
pub fn build<T: EventListener>(term: &Term<T>, theme: &Theme) -> Frame {
    let content = term.renderable_content();
    let cols = term.columns();
    let screen_lines = term.screen_lines();
    let colors = content.colors;
    let selection = content.selection;
    let background = theme.resolve(Color::Named(NamedColor::Background), colors);
    let cursor = cursor_for(&content, cols, screen_lines);

    let mut rows: Vec<Row> = Vec::with_capacity(screen_lines);
    let mut row = Row::default();

    for (index, indexed) in content.display_iter.enumerate() {
        if index > 0 && index % cols == 0 {
            rows.push(std::mem::take(&mut row));
        }
        push_cell(
            &mut row,
            indexed.cell,
            indexed.point,
            theme,
            colors,
            selection,
        );
    }

    if rows.len() < screen_lines {
        rows.push(row);
    }
    while rows.len() < screen_lines {
        rows.push(Row::default());
    }

    Frame {
        rows,
        cols,
        cursor,
        background,
    }
}

/// Update only the requested viewport rows from the current grid. The row
/// buffers keep their capacity, so repeated output does not reallocate every
/// line of the screen.
pub fn update_rows<T: EventListener>(
    term: &Term<T>,
    theme: &Theme,
    frame: &mut Frame,
    damaged_rows: &[usize],
) {
    if damaged_rows.is_empty() {
        return;
    }
    let content = term.renderable_content();
    let cols = term.columns();
    let screen_lines = term.screen_lines();
    if frame.cols != cols || frame.rows.len() != screen_lines {
        *frame = build(term, theme);
        return;
    }

    let colors = content.colors;
    let selection = content.selection;
    let cursor = cursor_for(&content, cols, screen_lines);
    let mut damaged = vec![false; screen_lines];
    for row in damaged_rows {
        if let Some(flag) = damaged.get_mut(*row) {
            *flag = true;
        }
    }
    for (index, row) in frame.rows.iter_mut().enumerate() {
        if damaged[index] {
            row.text.clear();
            row.runs.clear();
            row.backgrounds.clear();
        }
    }

    for (index, indexed) in content.display_iter.enumerate() {
        let row_index = index / cols;
        if !damaged.get(row_index).copied().unwrap_or(false) {
            continue;
        }
        push_cell(
            &mut frame.rows[row_index],
            indexed.cell,
            indexed.point,
            theme,
            colors,
            selection,
        );
    }

    frame.background = theme.resolve(Color::Named(NamedColor::Background), colors);
    frame.cursor = cursor;
}

fn push_cell(
    row: &mut Row,
    cell: &alacritty_terminal::term::cell::Cell,
    point: alacritty_terminal::index::Point,
    theme: &Theme,
    colors: &Colors,
    selection: Option<alacritty_terminal::selection::SelectionRange>,
) {
    let selected = selection
        .as_ref()
        .is_some_and(|range| range.contains(point));

    let mut fg = theme.resolve(cell.fg, colors);
    let mut bg = theme.resolve(cell.bg, colors);

    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }

    if selected {
        bg = theme.selection;
    }

    // Draw every cell background, including the default one. This lets the
    // pane paint its full area (padding, footer and row-rounding leftovers)
    // with the same color as the TUI, including explicit SGR backgrounds.
    push_background(row, point.column.0, point.column.0 + 1, bg);

    let is_spacer = cell
        .flags
        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
    if !is_spacer {
        let character = if cell.flags.contains(Flags::HIDDEN) {
            ' '
        } else {
            cell.c
        };
        push_char(row, character, fg);

        if let Some(zerowidth) = cell.zerowidth() {
            for character in zerowidth {
                push_char(row, *character, fg);
            }
        }
    }
}

fn cursor_for(
    content: &alacritty_terminal::term::RenderableContent<'_>,
    cols: usize,
    screen_lines: usize,
) -> Option<Cursor> {
    let shape = content.cursor.shape;
    if matches!(shape, CursorShape::Hidden) {
        return None;
    }
    let line = content.cursor.point.line.0 + content.display_offset as i32;
    if line < 0 || line as usize >= screen_lines {
        return None;
    }
    Some(Cursor {
        row: line as usize,
        col: content.cursor.point.column.0.min(cols.saturating_sub(1)),
        shape: match shape {
            CursorShape::Beam => CursorShapeKind::Beam,
            CursorShape::Underline => CursorShapeKind::Underline,
            CursorShape::Block | CursorShape::HollowBlock => CursorShapeKind::Block,
            CursorShape::Hidden => unreachable!(),
        },
    })
}

fn push_char(row: &mut Row, character: char, color: Rgb) {
    let start = row.text.len();
    row.text.push(character);
    let end = row.text.len();

    if let Some(last) = row.runs.last_mut()
        && last.color == color
        && last.range.end == start
    {
        last.range.end = end;
        return;
    }

    row.runs.push(Run {
        range: start..end,
        color,
    });
}

fn push_background(row: &mut Row, start: usize, end: usize, color: Rgb) {
    if let Some(last) = row.backgrounds.last_mut()
        && last.color == color
        && last.end == start
    {
        last.end = end;
        return;
    }

    row.backgrounds.push(Background { start, end, color });
}

#[cfg(test)]
#[path = "../tests/term/frame.rs"]
mod tests;
