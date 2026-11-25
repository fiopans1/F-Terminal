pub mod color;
pub mod frame;
pub mod program_status;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Grid, Scroll};
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config as TermConfig, Term, TermDamage, TermMode};
use winit::event_loop::EventLoopProxy;

use crate::event::UserEvent;
use crate::term::color::Theme;
use crate::term::frame::Frame;
use crate::term::program_status::{ProgramNotification, ProgramRecord, ProgramStatus};
use crate::workspace::PaneId;

/// Forwards VT state-machine events to the UI thread, tagged with the pane
/// that produced them.
#[derive(Clone)]
pub struct EventProxy {
    pane: PaneId,
    proxy: EventLoopProxy<UserEvent>,
}

impl EventProxy {
    pub fn new(pane: PaneId, proxy: EventLoopProxy<UserEvent>) -> Self {
        Self { pane, proxy }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let _ = self.proxy.send_event(UserEvent::Term {
            pane: self.pane,
            event,
        });
    }
}

/// Size of the visible grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: usize,
    pub lines: usize,
}

/// The visible row that contains the cursor, plus one character per column.
/// Wide-character spacers are represented by spaces so the column index can be
/// used as a character index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputLine {
    pub row: usize,
    pub col: usize,
    pub text: String,
    /// First column after the cursor that contains non-blank text, or the
    /// terminal width. This is where a right prompt starts.
    pub right_limit: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// Owns the VT parser and the terminal state (grid, scrollback, selection).
pub struct Terminal {
    term: Term<EventProxy>,
    parser: vte::ansi::Processor,
    program_status: ProgramStatus,
    size: GridSize,
    scrollback_limit: usize,
}

impl Terminal {
    pub fn new(size: GridSize, proxy: EventProxy, scrollback_lines: usize) -> Self {
        let scrollback_limit = scrollback_lines.max(1);
        let config = TermConfig {
            scrolling_history: scrollback_limit,
            // Kitty keyboard protocol is intentionally not advertised yet: the
            // input encoder below speaks the classic xterm protocol.
            kitty_keyboard: false,
            ..TermConfig::default()
        };

        let term = Term::new(config, &size, proxy);

        Self {
            term,
            parser: vte::ansi::Processor::new(),
            program_status: ProgramStatus::default(),
            size,
            scrollback_limit,
        }
    }

    pub fn set_scrollback_limit(&mut self, scrollback_lines: usize) {
        let scrollback_lines = scrollback_lines.max(1);
        if scrollback_lines == self.scrollback_limit {
            return;
        }
        self.scrollback_limit = scrollback_lines;
        self.term.grid_mut().update_history(scrollback_lines);
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.program_status.feed(bytes);
        self.parser.advance(&mut self.term, bytes);
    }

    /// Replay sanitized scrollback before the shell starts. This deliberately
    /// skips the program-status scanner: saved history is plain text and SGR,
    /// never OSC notifications or other protocol messages.
    pub fn restore_scrollback(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Clone the complete grid (scrollback plus visible rows) so a background
    /// worker can turn it into ANSI without holding the UI thread.
    pub fn clone_grid(&self) -> Grid<Cell> {
        self.term.grid().clone()
    }

    pub fn in_alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn program_status_revision(&self) -> u64 {
        self.program_status.revision()
    }

    pub fn has_program_status(&self) -> bool {
        self.program_status.has_records()
    }

    pub fn program_status(&self) -> Vec<ProgramRecord> {
        self.program_status.snapshot()
    }

    pub fn clear_active_program_status(&mut self) {
        self.program_status.clear_active();
    }

    pub fn take_program_status_replies(&mut self) -> Vec<String> {
        self.program_status.take_replies()
    }

    pub fn take_program_notifications(&mut self) -> Vec<ProgramNotification> {
        self.program_status.take_notifications()
    }

    pub fn take_cwd(&mut self) -> Option<std::path::PathBuf> {
        self.program_status.take_cwd()
    }

    pub fn resize(&mut self, size: GridSize) {
        if size != self.size {
            self.term.resize(size);
            self.size = size;
        }
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Snapshot of the cursor row, used by the completion engine. Returns
    /// `None` in the alternate screen, while scrolled back, or whenever there
    /// is text to the right of the cursor (right prompts, full-screen apps).
    pub fn input_line(&self) -> Option<InputLine> {
        input_line_from(&self.term, self.size)
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        self.term.scroll_display(scroll);
    }

    pub fn frame(&self, theme: &Theme) -> Frame {
        frame::build(&self.term, theme)
    }

    /// Update a cached frame using alacritty's per-line damage. Returns the
    /// viewport rows that changed. A full rebuild is forced when the display
    /// is scrolled back or the grid dimensions no longer match.
    pub fn update_frame(&mut self, theme: &Theme, frame: &mut Frame) -> Vec<usize> {
        let screen_lines = self.term.screen_lines();
        let cols = self.term.columns();
        let display_offset = self.term.grid().display_offset();
        if display_offset != 0 || frame.cols != cols || frame.rows.len() != screen_lines {
            *frame = frame::build(&self.term, theme);
            self.term.reset_damage();
            return (0..screen_lines).collect();
        }

        let damage = self.term.damage();
        let mut full = false;
        let rows: Vec<usize> = match damage {
            TermDamage::Full => {
                full = true;
                Vec::new()
            }
            TermDamage::Partial(lines) => lines
                .map(|line| line.line)
                .filter(|line| *line < screen_lines)
                .collect(),
        };
        self.term.reset_damage();

        if full {
            *frame = frame::build(&self.term, theme);
            return (0..screen_lines).collect();
        }
        if rows.is_empty() {
            return rows;
        }
        frame::update_rows(&self.term, theme, frame, &rows);
        rows
    }

    pub fn start_selection(&mut self, point: Point, side: Side) {
        self.term.selection = Some(Selection::new(SelectionType::Simple, point, side));
    }

    pub fn start_word_selection(&mut self, point: Point, side: Side) {
        self.term.selection = Some(Selection::new(SelectionType::Semantic, point, side));
    }

    pub fn update_selection(&mut self, point: Point, side: Side) {
        if let Some(selection) = &mut self.term.selection {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub fn selection_to_string(&self) -> Option<String> {
        self.term.selection_to_string()
    }
}

fn input_line_from<T: EventListener>(term: &Term<T>, size: GridSize) -> Option<InputLine> {
    let content = term.renderable_content();
    if content.mode.contains(TermMode::ALT_SCREEN) || content.display_offset != 0 {
        return None;
    }

    let cursor_point = content.cursor.point;
    let cursor_line = cursor_point.line.0;
    if cursor_line < 0 || cursor_line as usize >= size.lines {
        return None;
    }

    let mut chars = vec![' '; size.cols];
    let mut found = false;
    for indexed in content.display_iter {
        if indexed.point.line.0 != cursor_line {
            continue;
        }
        found = true;
        let column = indexed.point.column.0;
        if column >= chars.len() {
            continue;
        }
        let cell = indexed.cell;
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        chars[column] = if cell.flags.contains(Flags::HIDDEN) {
            ' '
        } else {
            cell.c
        };
    }

    if !found {
        return None;
    }

    let col = cursor_point.column.0.min(size.cols.saturating_sub(1));
    let right_limit = chars[col.saturating_add(1)..]
        .iter()
        .position(|character| *character != ' ')
        .map(|offset| col + 1 + offset)
        .unwrap_or(size.cols);

    Some(InputLine {
        row: cursor_line as usize,
        col,
        text: chars.into_iter().collect(),
        right_limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;

    fn line_for(bytes: &[u8]) -> Option<InputLine> {
        let size = GridSize { cols: 40, lines: 4 };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
        parser.advance(&mut term, bytes);
        input_line_from(&term, size)
    }

    #[test]
    fn reads_the_cursor_row_and_prompt() {
        let line = line_for(b"user@host:~$ git ch").unwrap();
        assert_eq!(line.row, 0);
        assert_eq!(line.col, 19);
        assert_eq!(&line.text[..19], "user@host:~$ git ch");
        assert_eq!(line.right_limit, 40);
    }

    #[test]
    fn reports_a_right_prompt_limit() {
        let line = line_for(b"user@host:~$ git ch   12:30\x1b[1;20H").unwrap();
        assert_eq!(line.col, 19);
        assert_eq!(line.right_limit, 22);
    }

    #[test]
    fn ignores_alternate_screen() {
        assert!(line_for(b"\x1b[?1049huser@host:~$ git").is_none());
    }
}
