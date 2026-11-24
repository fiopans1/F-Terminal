use std::path::PathBuf;

use alacritty_terminal::event::Event;
use winit::window::WindowId;

use crate::complete::history::History;
use crate::explorer::FileRow;
use crate::forge::RemoteChange;
use crate::git::refs::BranchInfo;
use crate::git::{Snapshot, StatusSnapshot};
use crate::git_status::GitStatus;
use crate::highlight::StyledSpan;
use crate::menu::Action;
use crate::review::ReviewDetail;
use crate::shell::TerminalProfile;
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
    /// Executable names discovered in `PATH` for completion.
    CompletionCommands(Vec<String>),
    /// Shell history loaded by a background worker.
    CompletionHistory(History),
    /// Extra shell profiles discovered after startup (WSL distributions).
    #[cfg_attr(not(windows), allow(dead_code))]
    ShellProfiles(Vec<TerminalProfile>),
    /// Initial directory scan for the file explorer sidebar.
    ExplorerLoaded {
        root: PathBuf,
        rows: Vec<FileRow>,
        error: Option<String>,
    },
    /// Action selected from the native macOS menu.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Menu(Action),
    /// Open the Agent Mode window.
    OpenAgentMode,
    /// Focus a pane selected from the Agent Mode window.
    FocusAgentPane { pane: PaneId },
    /// Agent processes discovered by the background watcher.
    AgentsDetected {
        generation: u64,
        panes: Vec<crate::agents::DetectedAgent>,
        external: Vec<crate::agents::DetectedAgent>,
    },
    /// Force an immediate agent process scan (Agent Mode refresh button).
    RefreshAgents,
    /// Terminate an agent process selected in Agent Mode.
    KillAgent { pid: u32 },
    /// Result of a kill request handled by a background worker.
    AgentKilled {
        pid: u32,
        result: Result<String, String>,
    },
    /// Change the terminal font size at runtime.
    SetFontSize(f32),
    /// Syntax highlighting result from the background worker.
    Highlight {
        generation: u64,
        revision: u64,
        spans: Vec<Vec<StyledSpan>>,
    },
    /// Request to open the Code Review window for a directory.
    OpenCodeReview { cwd: PathBuf },
    /// Request to open the Git manager window for a directory.
    OpenGitManager { cwd: PathBuf },
    /// Code review diff loaded in the background.
    ReviewLoaded {
        window_id: WindowId,
        request_id: u64,
        result: Result<ReviewDetail, String>,
    },
    /// Open pull requests / merge requests loaded from the forge.
    PullRequestsLoaded {
        window_id: WindowId,
        request_id: u64,
        result: Result<Vec<RemoteChange>, String>,
    },
    /// Git branch and diff stats for a terminal pane.
    GitStatus {
        pane: PaneId,
        cwd: PathBuf,
        status: Option<GitStatus>,
    },
    /// Local branches loaded for the footer branch popup.
    BranchesLoaded {
        pane: PaneId,
        cwd: PathBuf,
        result: Result<Vec<BranchInfo>, String>,
    },
    /// A footer branch checkout finished.
    BranchCheckedOut {
        pane: PaneId,
        cwd: PathBuf,
        branch: String,
        result: Result<String, String>,
    },
    /// Working-tree diff loaded for the embedded split diff view.
    WorkingDiffLoaded {
        pane: PaneId,
        cwd: PathBuf,
        generation: u64,
        result: Result<ReviewDetail, String>,
    },
    /// Full repository snapshot for the Git manager window.
    GitSnapshotLoaded {
        window_id: WindowId,
        request_id: u64,
        result: Result<Snapshot, String>,
    },
    /// Lightweight working-tree status refresh for the Git manager.
    GitStatusLoaded {
        window_id: WindowId,
        request_id: u64,
        result: Result<StatusSnapshot, String>,
    },
    /// Diff for the selected commit or working tree.
    GitDetailLoaded {
        window_id: WindowId,
        request_id: u64,
        result: Result<ReviewDetail, String>,
    },
    /// A mutating Git operation finished.
    GitOperationFinished {
        window_id: WindowId,
        request_id: u64,
        result: Result<String, String>,
    },
}
