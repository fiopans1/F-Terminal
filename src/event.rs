use alacritty_terminal::event::Event;

use crate::highlight::StyledSpan;
use crate::menu::Action;
use crate::workspace::PaneId;

/// Events sent from background threads (PTY readers, terminal event proxy)
/// into the winit event loop. Every event is tagged with the pane that
/// produced it so multiple shells can coexist.
#[derive(Debug)]
pub enum UserEvent {
    /// Raw bytes produced by the child process.
    PtyData { pane: PaneId, bytes: Vec<u8> },
    /// The child process closed its PTY.
    PtyClosed { pane: PaneId },
    /// Event emitted by the VT state machine (title, DA responses, clipboard...).
    Term { pane: PaneId, event: Event },
    /// Action selected from the native macOS menu.
    Menu(Action),
    /// Syntax highlighting result from the background worker.
    Highlight {
        generation: u64,
        revision: u64,
        spans: Vec<Vec<StyledSpan>>,
    },
}
