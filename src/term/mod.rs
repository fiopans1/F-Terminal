pub mod color;
pub mod frame;
pub mod program_status;

use std::sync::OnceLock;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Grid, Scroll};
use alacritty_terminal::index::{Column, Direction, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::search::{RegexIter, RegexSearch};
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

impl GridSize {
    /// PTYs use 16-bit dimensions. Keep malformed session data or absurd
    /// window sizes from allocating gigantic terminal grids.
    pub const MAX_COLS: usize = 4096;
    pub const MAX_LINES: usize = 4096;

    pub fn clamped(self) -> Self {
        Self {
            cols: self.cols.clamp(2, Self::MAX_COLS),
            lines: self.lines.clamp(1, Self::MAX_LINES),
        }
    }
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

    /// Snapshot of the last `max_lines` non-empty visible viewport lines,
    /// used by Agent Mode's live terminal preview.
    pub fn tail_lines(&self, max_lines: usize) -> Vec<String> {
        tail_lines_from(&self.term, max_lines)
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        self.term.scroll_display(scroll);
    }

    pub fn scroll_to_point(&mut self, point: Point) {
        self.term.scroll_to_point(point);
    }

    /// Find an OSC 8 hyperlink or a plain URL under a terminal cell.
    pub fn link_at(&self, point: Point) -> Option<(String, std::ops::RangeInclusive<Point>)> {
        link_at_in_term(&self.term, point)
    }

    /// Search the scrollback and visible screen for a literal, smart-case
    /// query. Results are capped so a broad query cannot allocate forever.
    pub fn search_matches(
        &self,
        query: &str,
        max_matches: usize,
    ) -> Vec<std::ops::RangeInclusive<Point>> {
        if query.is_empty() || max_matches == 0 {
            return Vec::new();
        }
        // Smart case: an all-lowercase query ignores case, any uppercase
        // character makes the search case-sensitive.
        let pattern = search_pattern(query);
        let Ok(mut regex) = RegexSearch::new(&pattern) else {
            return Vec::new();
        };
        let start = Point::new(self.term.topmost_line(), Column(0));
        let end = Point::new(self.term.bottommost_line(), self.term.last_column());
        RegexIter::new(start, end, Direction::Right, &self.term, &mut regex)
            .take(max_matches)
            .collect()
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

fn tail_lines_from<T: EventListener>(term: &Term<T>, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }
    let cols = term.columns();
    let screen_lines = term.screen_lines();
    if cols == 0 || screen_lines == 0 {
        return Vec::new();
    }

    let content = term.renderable_content();
    let mut rows: Vec<String> = Vec::with_capacity(screen_lines);
    let mut line = String::with_capacity(cols);

    for (index, indexed) in content.display_iter.enumerate() {
        if index > 0 && index % cols == 0 {
            rows.push(std::mem::take(&mut line));
        }
        let cell = indexed.cell;
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        if cell.flags.contains(Flags::HIDDEN) {
            line.push(' ');
        } else {
            line.push(cell.c);
        }
    }
    if !line.is_empty() && rows.len() < screen_lines {
        rows.push(line);
    }

    let mut trimmed: Vec<String> = rows
        .into_iter()
        .map(|row| row.trim_end().to_owned())
        .collect();
    while trimmed.last().is_some_and(String::is_empty) {
        trimmed.pop();
    }

    let keep_from = trimmed.len().saturating_sub(max_lines);
    trimmed.drain(..keep_from);
    trimmed
}

fn link_at_in_term<T: EventListener>(
    term: &Term<T>,
    point: Point,
) -> Option<(String, std::ops::RangeInclusive<Point>)> {
    if let Some(link) = term.grid()[point].hyperlink() {
        let uri = link.uri().to_owned();
        if is_link_uri(&uri) {
            let mut start = point;
            while start.column.0 > 0 {
                let previous = Point::new(start.line, Column(start.column.0 - 1));
                if term.grid()[previous]
                    .hyperlink()
                    .is_some_and(|other| other == link)
                {
                    start = previous;
                } else {
                    break;
                }
            }

            let last_column = term.last_column().0;
            let mut end = point;
            while end.column.0 < last_column {
                let next = Point::new(end.line, Column(end.column.0 + 1));
                if term.grid()[next]
                    .hyperlink()
                    .is_some_and(|other| other == link)
                {
                    end = next;
                } else {
                    break;
                }
            }
            return Some((uri, start..=end));
        }
    }

    let start = term.line_search_left(point);
    let end = term.line_search_right(point);
    static LINK_REGEX: OnceLock<Option<RegexSearch>> = OnceLock::new();
    let template = LINK_REGEX
        .get_or_init(|| RegexSearch::new(r"(?:https?://|www\.)[^\s]+|mailto:[^\s]+").ok());
    let mut regex = template.clone()?;
    let iter = RegexIter::new(start, end, Direction::Right, term, &mut regex);
    for matched in iter {
        if matched.contains(&point) {
            let text = term.bounds_to_string(*matched.start(), *matched.end());
            if let Some(uri) = normalize_link(&text) {
                return Some((uri, matched));
            }
        }
    }
    None
}

fn search_pattern(query: &str) -> String {
    if query.chars().any(char::is_uppercase) {
        regex_literal(query)
    } else {
        format!("(?i){}", regex_literal(query))
    }
}

fn regex_literal(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len());
    for character in query.chars() {
        if "\\.^$*+?()[]{}|".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn is_link_uri(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://") || uri.starts_with("mailto:")
}

fn normalize_link(text: &str) -> Option<String> {
    let mut text = text.trim();
    loop {
        let last = text.chars().last()?;
        let strip = match last {
            '.' | ',' | ';' | ':' | '!' | '?' => true,
            ')' => {
                text.chars().filter(|character| *character == ')').count()
                    > text.chars().filter(|character| *character == '(').count()
            }
            ']' => {
                text.chars().filter(|character| *character == ']').count()
                    > text.chars().filter(|character| *character == '[').count()
            }
            '}' => {
                text.chars().filter(|character| *character == '}').count()
                    > text.chars().filter(|character| *character == '{').count()
            }
            _ => false,
        };
        if !strip {
            break;
        }
        text = &text[..text.len() - last.len_utf8()];
        text = text.trim_end();
    }

    if text.starts_with("www.") {
        Some(format!("https://{text}"))
    } else if is_link_uri(text) {
        Some(text.to_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::Line;
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

    #[test]
    fn reads_tail_lines_trimmed() {
        let size = GridSize { cols: 40, lines: 6 };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
        parser.advance(&mut term, b"line 1\r\nline 2  \r\nline 3\r\nline 4");
        assert_eq!(
            tail_lines_from(&term, 2),
            vec!["line 3".to_owned(), "line 4".to_owned()]
        );
        assert_eq!(tail_lines_from(&term, 0), Vec::<String>::new());
    }

    fn search_count(term: &Term<VoidListener>, query: &str) -> usize {
        let mut regex = RegexSearch::new(&search_pattern(query)).unwrap();
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        RegexIter::new(start, end, Direction::Right, term, &mut regex).count()
    }

    #[test]
    fn search_uses_smart_case() {
        let size = GridSize { cols: 20, lines: 4 };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
        parser.advance(&mut term, b"Hello\r\nhello\r\nHELLO");

        assert_eq!(search_count(&term, "hello"), 3);
        assert_eq!(search_count(&term, "Hello"), 1);
    }

    #[test]
    fn search_treats_the_query_as_a_literal() {
        assert_eq!(regex_literal("a.b[c]"), r"a\.b\[c\]");
        assert_eq!(search_pattern("a.b"), r"(?i)a\.b");
    }

    #[test]
    fn detects_plain_links_and_strips_trailing_punctuation() {
        let size = GridSize { cols: 40, lines: 3 };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
        parser.advance(&mut term, b"see https://example.com/docs).");

        let point = Point::new(Line(0), Column(8));
        let (uri, range) = link_at_in_term(&term, point).unwrap();
        assert_eq!(uri, "https://example.com/docs");
        assert!(range.contains(&point));
        assert_eq!(link_at_in_term(&term, Point::new(Line(0), Column(0))), None);
    }

    #[test]
    fn detects_osc8_hyperlinks_across_their_cells() {
        let size = GridSize { cols: 20, lines: 2 };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
        parser.advance(
            &mut term,
            b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\",
        );

        let (uri, range) = link_at_in_term(&term, Point::new(Line(0), Column(1))).unwrap();
        assert_eq!(uri, "https://example.com");
        assert_eq!(*range.start(), Point::new(Line(0), Column(0)));
        assert_eq!(*range.end(), Point::new(Line(0), Column(3)));
    }

    #[test]
    fn link_normalization_strips_punctuation_and_rejects_unsafe_schemes() {
        assert_eq!(
            normalize_link("https://example.com).").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            normalize_link("www.example.com").as_deref(),
            Some("https://www.example.com")
        );
        assert_eq!(
            normalize_link("https://en.wikipedia.org/wiki/Foo_(bar)").as_deref(),
            Some("https://en.wikipedia.org/wiki/Foo_(bar)")
        );
        assert_eq!(normalize_link("file:///etc/passwd"), None);
        assert_eq!(
            normalize_link("mailto:dev@example.com").as_deref(),
            Some("mailto:dev@example.com")
        );
    }
}
