use std::path::PathBuf;

use alacritty_terminal::event::Event;
use winit::window::WindowId;

use crate::git_status::GitStatus;
use crate::highlight::StyledSpan;
use crate::menu::Action;
use crate::review::{PullRequestSummary, ReviewDetail};
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
    /// Request to open the Code Review window for a directory.
    OpenCodeReview { cwd: PathBuf },
    /// Code review diff loaded in the background.
    ReviewLoaded {
        window_id: WindowId,
        request_id: u64,
        result: Result<ReviewDetail, String>,
    },
    /// Open pull requests loaded from GitHub in the background.
    PullRequestsLoaded {
        window_id: WindowId,
        request_id: u64,
        result: Result<Vec<PullRequestSummary>, String>,
    },
    /// Git branch and diff stats for a terminal pane.
    GitStatus {
        pane: PaneId,
        cwd: PathBuf,
        status: Option<GitStatus>,
    },
}
