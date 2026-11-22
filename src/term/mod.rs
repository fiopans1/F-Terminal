pub mod color;
pub mod frame;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use winit::event_loop::EventLoopProxy;

use crate::event::UserEvent;
use crate::term::color::Theme;
use crate::term::frame::Frame;
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
        self.parser.advance(&mut self.term, bytes);
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

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        self.term.scroll_display(scroll);
    }

    pub fn frame(&self, theme: &Theme) -> Frame {
        frame::build(&self.term, theme)
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
