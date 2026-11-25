use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime};

use alacritty_terminal::event::{Event, WindowSize};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::term::TermMode;
use anyhow::Context;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{CursorIcon, Theme as WindowTheme, Window, WindowId};

use crate::agent_window::AgentWindow;
use crate::agents::{
    self, AgentChild, AgentId, AgentKind, AgentRow, AgentSnapshot, AgentSource, AgentState,
    AgentWatchRequest, DetectedAgent, kind_from_protocol_app,
};
use crate::complete;
use crate::complete::history::History;
use crate::config::{Config, RenderMode, config_path, save_font_size};
use crate::diff_view::{DiffRow, DiffRowKind, build_diff_rows};
use crate::editor::Editor;
use crate::event::UserEvent;
use crate::explorer::{FileExplorer, FileRow};
use crate::git::ops::{self as git_ops, Action as GitOpsAction};
use crate::git::refs::{self as git_refs, BranchInfo};
use crate::git_status::{self, GitStatus};
use crate::git_window::GitWindow;
use crate::highlight::{self, HighlightRequest, StyledSpan};
use crate::input;
use crate::layout::{DividerLayout, PaneLayout, Rect, compute_layout};
use crate::menu;
use crate::render::quads::Quad;
use crate::render::scene::{GhostText, Label, PaneLayer, Scene};
use crate::render::{CellMetrics, Renderer, SharedRenderResources, grid_size_for};
use crate::review::{self, ReviewDetail, ReviewKind, ReviewSummary};
use crate::review_window::ReviewWindow;
use crate::session::{
    self, SavedDirection, SavedLayout, SavedPane, SavedTab, SavedWindow, ScrollbackSource,
    SessionState, SessionWriter,
};
use crate::shell::{ShellCatalog, TerminalProfile};
use crate::sidebar::{
    self, LayoutSpec, Sidebar, SidebarHit, SidebarLayout, SidebarMode, SidebarRow, TabSummary,
};
use crate::term::color::{Rgb, Theme, to_linear};
use crate::term::frame::{Background, Cursor, CursorShapeKind, Frame, Row, Run};
use crate::term::program_status::{BlockKind, ProgramRecord, ProgramState};
use crate::ui;
use crate::workspace::{CloseOutcome, Pane, PaneId, SplitDirection, TabId, Workspace};

const GIT_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(3);

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    window: Option<WindowState>,
    review: Option<ReviewWindow>,
    git: Option<GitWindow>,
    agent: Option<AgentWindow>,
    shared_render: Option<SharedRenderResources>,
    _menu: Option<menu::MenuState>,
    config_modified: Option<SystemTime>,
    last_config_poll: Instant,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>, config: Config) -> Self {
        log::info!(
            "config: render {:?}, scrollback {} active / {} hidden, budget {} MB",
            config.render_mode,
            config.scrollback_lines,
            config.hidden_scrollback_lines,
            config.scrollback_budget_mb
        );

        Self {
            proxy,
            config,
            window: None,
            review: None,
            git: None,
            agent: None,
            shared_render: None,
            _menu: None,
            config_modified: config_modified_time(),
            last_config_poll: Instant::now(),
        }
    }

    fn poll_config_reload(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_config_poll) < Duration::from_secs(1) {
            return;
        }
        self.last_config_poll = now;

        let Some(path) = config_path() else {
            return;
        };
        let Ok(modified) = std::fs::metadata(&path).and_then(|metadata| metadata.modified()) else {
            return;
        };
        if self.config_modified == Some(modified) {
            return;
        }

        let Some(new_config) = Config::try_load() else {
            self.config_modified = Some(modified);
            return;
        };
        let old = std::mem::replace(&mut self.config, new_config);
        self.config_modified = Some(modified);
        log::info!("reloaded config from {}", path.display());
        self.apply_config(&old);
    }

    fn apply_config(&mut self, old: &Config) {
        if let Some(state) = &mut self.window {
            state.apply_config(old, &self.config);
            state.window.request_redraw();
        }
        if let Some(git) = &mut self.git {
            git.apply_config(&self.config);
        }
        if let Some(review) = &mut self.review {
            review.apply_config(&self.config);
        }
        if let Some(agent) = &mut self.agent {
            agent.apply_config(&self.config);
        }
    }

    /// Create a renderer for a secondary window, reusing the main window's
    /// GPU device and text atlas when shared rendering is enabled. If shared
    /// initialization fails, fall back to an isolated renderer so the window
    /// still opens.
    fn create_secondary_renderer(
        &self,
        window: &Arc<Window>,
        event_loop: &ActiveEventLoop,
        label: &str,
    ) -> Option<Renderer> {
        let shared = if self.config.render_mode == RenderMode::Shared {
            self.shared_render.clone()
        } else {
            None
        };

        match pollster::block_on(Renderer::new(
            window.clone(),
            event_loop,
            &self.config,
            shared.as_ref(),
        )) {
            Ok(renderer) => Some(renderer),
            Err(error) if shared.is_some() => {
                log::warn!(
                    "shared renderer for the {label} window failed; falling back to isolated: {error:#}"
                );
                match pollster::block_on(Renderer::new(
                    window.clone(),
                    event_loop,
                    &self.config,
                    None,
                )) {
                    Ok(renderer) => Some(renderer),
                    Err(error) => {
                        log::error!("failed to initialize the {label} renderer: {error:#}");
                        None
                    }
                }
            }
            Err(error) => {
                log::error!("failed to initialize the {label} renderer: {error:#}");
                None
            }
        }
    }

    fn open_review_window(&mut self, event_loop: &ActiveEventLoop, cwd: PathBuf) {
        if let Some(review) = &self.review {
            review.focus();
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Code Review — F-Terminal")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(720.0, 480.0))
            .with_theme(Some(WindowTheme::Dark))
            .with_visible(false);

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                log::error!("failed to create the code review window: {error}");
                return;
            }
        };

        let Some(renderer) = self.create_secondary_renderer(&window, event_loop, "code review")
        else {
            return;
        };

        let review = ReviewWindow::new(
            window,
            renderer,
            cwd,
            self.proxy.clone(),
            self.config.forge_tokens.clone(),
        );
        self.review = Some(review);
        if let Some(review) = &mut self.review {
            review.reveal(&self.config);
        }
        log::info!("opened code review window");
    }

    fn open_git_window(&mut self, event_loop: &ActiveEventLoop, cwd: PathBuf) {
        if let Some(git) = &self.git {
            git.focus();
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Git — F-Terminal")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(820.0, 520.0))
            .with_theme(Some(WindowTheme::Dark))
            .with_visible(false);

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                log::error!("failed to create the git window: {error}");
                return;
            }
        };

        let Some(renderer) = self.create_secondary_renderer(&window, event_loop, "git") else {
            return;
        };

        let git = GitWindow::new(window, renderer, cwd, self.proxy.clone());
        self.git = Some(git);
        if let Some(git) = &mut self.git {
            git.reveal(&self.config);
        }
        log::info!("opened git manager window");
    }

    fn open_agent_window(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(agent) = &self.agent {
            agent.focus();
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Centro de Agentes — F-Terminal")
            .with_inner_size(LogicalSize::new(1140.0, 740.0))
            .with_min_inner_size(LogicalSize::new(760.0, 480.0))
            .with_theme(Some(WindowTheme::Dark))
            .with_visible(false);

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                log::error!("failed to create the agent window: {error}");
                return;
            }
        };

        let Some(renderer) = self.create_secondary_renderer(&window, event_loop, "agent mode")
        else {
            return;
        };

        let agent = AgentWindow::new(window, renderer, self.proxy.clone());
        log::info!("opened agent mode window");
        self.agent = Some(agent);
        if let Some(state) = &mut self.window {
            state.force_agent_refresh(&self.config);
        }
        self.sync_agent_window();
        if let Some(agent) = &mut self.agent {
            agent.reveal(&self.config);
        }
    }

    fn focus_agent_pane(&mut self, pane: PaneId) {
        let Some(state) = &mut self.window else {
            return;
        };
        if state.focus_agent_pane(pane, &self.config) {
            state.window.focus_window();
        }
        self.sync_agent_window();
    }

    fn sync_agent_window(&mut self) {
        let Some(agent_window) = &mut self.agent else {
            return;
        };
        let Some(state) = &mut self.window else {
            return;
        };
        state.rebuild_agent_snapshot();
        let snapshot = state.agent_snapshot.clone();
        agent_window.apply_snapshot(snapshot);
    }

    fn focus_main(&self) {
        if let Some(window) = &self.window {
            window.window.focus_window();
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self._menu.is_none() {
            match menu::MenuState::install(self.proxy.clone()) {
                Ok(menu) => self._menu = Some(menu),
                Err(error) => log::warn!("failed to install the application menu: {error:#}"),
            }
        }

        if self.window.is_some() {
            return;
        }

        match WindowState::new(event_loop, &self.config, self.proxy.clone()) {
            Ok(window) => {
                if self.config.render_mode == RenderMode::Shared {
                    self.shared_render = Some(window.shared_render_resources());
                    log::info!("shared renderer enabled for secondary windows");
                }
                self.window = Some(window);
            }
            Err(error) => {
                log::error!("failed to start the terminal: {error:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let main_id = self.window.as_ref().map(|window| window.window_id());

        if Some(window_id) == main_id {
            let Some(state) = &mut self.window else {
                return;
            };

            if matches!(event, WindowEvent::CloseRequested) {
                state.save_session(&self.config, true);
                event_loop.exit();
                return;
            }

            state.on_window_event(event, event_loop, &self.config);
            return;
        }

        if self
            .git
            .as_ref()
            .is_some_and(|git| git.window_id() == window_id)
        {
            if matches!(event, WindowEvent::CloseRequested) {
                self.git = None;
                self.focus_main();
                return;
            }

            let close_requested = {
                let Some(git) = &mut self.git else {
                    return;
                };
                git.on_window_event(event, &self.config);
                git.close_requested()
            };

            if close_requested {
                self.git = None;
                self.focus_main();
            }
            return;
        }

        if self
            .agent
            .as_ref()
            .is_some_and(|agent| agent.window_id() == window_id)
        {
            if matches!(event, WindowEvent::CloseRequested) {
                self.agent = None;
                self.focus_main();
                return;
            }

            let close_requested = {
                let Some(agent) = &mut self.agent else {
                    return;
                };
                agent.on_window_event(event, &self.config);
                agent.close_requested()
            };

            if close_requested {
                self.agent = None;
                self.focus_main();
            }
            return;
        }

        if !self
            .review
            .as_ref()
            .is_some_and(|review| review.window_id() == window_id)
        {
            return;
        }

        if matches!(event, WindowEvent::CloseRequested) {
            self.review = None;
            self.focus_main();
            return;
        }

        let close_requested = {
            let Some(review) = &mut self.review else {
                return;
            };
            review.on_window_event(event, &self.config);
            review.close_requested()
        };

        if close_requested {
            self.review = None;
            self.focus_main();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::OpenCodeReview { cwd } => {
                self.open_review_window(event_loop, cwd);
                return;
            }
            UserEvent::OpenGitManager { cwd } => {
                self.open_git_window(event_loop, cwd);
                return;
            }
            UserEvent::OpenAgentMode => {
                self.open_agent_window(event_loop);
                return;
            }
            UserEvent::FocusAgentPane { pane } => {
                self.focus_agent_pane(pane);
                return;
            }
            UserEvent::RefreshAgents => {
                if let Some(state) = &mut self.window {
                    state.force_agent_refresh(&self.config);
                    state.window.request_redraw();
                }
                self.sync_agent_window();
                if let Some(agent) = &self.agent {
                    agent.request_redraw();
                }
                return;
            }
            UserEvent::SetFontSize(size) => {
                let size = size.clamp(6.0, 48.0);
                if (self.config.font_size - size).abs() < f32::EPSILON {
                    return;
                }
                self.config.font_size = size;
                if let Some(state) = &mut self.window {
                    state.apply_font_size(&self.config);
                }
                if let Some(review) = &mut self.review {
                    review.apply_config(&self.config);
                }
                if let Some(git) = &mut self.git {
                    git.apply_config(&self.config);
                }
                if let Some(agent) = &mut self.agent {
                    agent.apply_config(&self.config);
                }
                if let Err(error) = save_font_size(size) {
                    log::warn!("could not save font size: {error}");
                }
                return;
            }
            UserEvent::KillAgent { pid } => {
                if let Some(agent) = &mut self.agent {
                    agent.on_kill_requested(pid);
                }
                let proxy = self.proxy.clone();
                let spawned =
                    std::thread::Builder::new()
                        .name("agent-kill".into())
                        .spawn(move || {
                            let result = agents::terminate_process(pid);
                            let _ = proxy.send_event(UserEvent::AgentKilled { pid, result });
                        });
                if let Err(error) = spawned
                    && let Some(agent) = &mut self.agent
                {
                    agent.on_agent_killed(
                        pid,
                        &Err(format!("could not start the kill worker: {error}")),
                    );
                }
                return;
            }
            UserEvent::AgentKilled { pid, result } => {
                if let Some(agent) = &mut self.agent {
                    agent.on_agent_killed(pid, &result);
                }
                if result.is_ok() {
                    if let Some(state) = &mut self.window {
                        state.force_agent_refresh(&self.config);
                        state.window.request_redraw();
                    }
                    self.sync_agent_window();
                }
                return;
            }
            UserEvent::SendAgentInput { pane, bytes, label } => {
                let sent = self
                    .window
                    .as_mut()
                    .is_some_and(|state| state.send_agent_input(pane, &bytes));
                if let Some(agent) = &mut self.agent {
                    if sent {
                        agent
                            .on_action_notice(format!("Enviado a pane {}: {label}", pane.0), false);
                    } else {
                        agent.on_action_notice(
                            format!("El pane {} ya no está disponible", pane.0),
                            true,
                        );
                    }
                }
                if sent {
                    if let Some(state) = &mut self.window {
                        state.window.request_redraw();
                    }
                    self.sync_agent_window();
                }
                return;
            }
            UserEvent::LaunchAgent { kind, cwd } => {
                let launched = self
                    .window
                    .as_mut()
                    .and_then(|state| state.launch_agent(&kind, cwd, &self.config));
                if let Some(agent) = &mut self.agent {
                    if let Some(pane) = launched {
                        agent.on_action_notice(
                            format!(
                                "Lanzando {} en pane {} (Enter para enfocar)",
                                kind.label(),
                                pane.0
                            ),
                            false,
                        );
                    } else {
                        agent.on_action_notice(format!("No se pudo lanzar {}", kind.label()), true);
                    }
                }
                if launched.is_some() {
                    if let Some(state) = &mut self.window {
                        state.force_agent_refresh(&self.config);
                        state.window.request_redraw();
                    }
                    self.sync_agent_window();
                }
                return;
            }
            UserEvent::MarkAgentSeen { id } => {
                if let Some(state) = &mut self.window {
                    state.mark_agent_seen(&id);
                    state.window.request_redraw();
                }
                if let Some(agent) = &mut self.agent {
                    agent.on_action_notice("Marcado como visto".into(), false);
                }
                self.sync_agent_window();
                return;
            }
            UserEvent::MarkAllAgentsSeen => {
                let marked = self
                    .window
                    .as_mut()
                    .map(|state| {
                        let count = state.mark_all_agents_seen();
                        state.window.request_redraw();
                        count
                    })
                    .unwrap_or(0);
                if let Some(agent) = &mut self.agent {
                    let text = if marked == 0 {
                        "No había avisos pendientes por marcar".into()
                    } else {
                        format!("{marked} agente(s) marcados como vistos")
                    };
                    agent.on_action_notice(text, false);
                }
                self.sync_agent_window();
                return;
            }
            UserEvent::AgentsDetected {
                generation,
                panes,
                external,
            } => {
                if let Some(state) = &mut self.window {
                    state.on_agents_detected(generation, panes, external);
                }
                self.sync_agent_window();
                if let Some(state) = &self.window {
                    state.window.request_redraw();
                }
                return;
            }
            UserEvent::ReviewListLoaded {
                window_id,
                request_id,
                result,
            } => {
                if let Some(review) = &mut self.review
                    && review.window_id() == window_id
                {
                    review.apply_list(request_id, result);
                }
                return;
            }
            UserEvent::ReviewLoaded {
                window_id,
                request_id,
                result,
            } => {
                if let Some(review) = &mut self.review
                    && review.window_id() == window_id
                {
                    review.apply_detail(request_id, result);
                }
                return;
            }
            UserEvent::PullRequestsLoaded {
                window_id,
                request_id,
                result,
            } => {
                if let Some(review) = &mut self.review
                    && review.window_id() == window_id
                {
                    review.apply_pull_requests(request_id, result);
                }
                return;
            }
            UserEvent::GitSnapshotLoaded {
                window_id,
                request_id,
                result,
            } => {
                if let Some(git) = &mut self.git
                    && git.window_id() == window_id
                {
                    git.apply_snapshot(request_id, result);
                }
                return;
            }
            UserEvent::GitStatusLoaded {
                window_id,
                request_id,
                result,
            } => {
                if let Some(git) = &mut self.git
                    && git.window_id() == window_id
                {
                    git.apply_status(request_id, result);
                }
                return;
            }
            UserEvent::GitDetailLoaded {
                window_id,
                request_id,
                result,
            } => {
                if let Some(git) = &mut self.git
                    && git.window_id() == window_id
                {
                    git.apply_detail(request_id, result);
                }
                return;
            }
            UserEvent::GitOperationFinished {
                window_id,
                request_id,
                result,
            } => {
                if let Some(git) = &mut self.git
                    && git.window_id() == window_id
                {
                    git.apply_operation(request_id, result);
                }
                return;
            }
            UserEvent::Menu(menu::Action::CodeReview) => {
                let cwd = if let Some(state) = &mut self.window {
                    state.refresh_cwds(&self.config);
                    state.focused_cwd()
                } else {
                    default_directory()
                };
                self.open_review_window(event_loop, cwd);
                return;
            }
            UserEvent::Menu(menu::Action::GitManager) => {
                let cwd = if let Some(state) = &mut self.window {
                    state.refresh_cwds(&self.config);
                    state.focused_cwd()
                } else {
                    default_directory()
                };
                self.open_git_window(event_loop, cwd);
                return;
            }
            UserEvent::Menu(menu::Action::AgentMode) => {
                self.open_agent_window(event_loop);
                return;
            }
            _ => {}
        }

        let Some(state) = &mut self.window else {
            return;
        };

        match event {
            UserEvent::PtyData { pane, bytes } => {
                if state.on_pty_data(pane, &bytes) {
                    state.relayout(&self.config);
                }
            }
            UserEvent::PtyClosed { pane } => {
                state.close_pane(pane, event_loop, &self.config);
            }
            UserEvent::Term { pane, event } => {
                state.on_term_event(pane, event, event_loop, &self.config);
            }
            UserEvent::Menu(action) => state.dispatch_menu(action, &self.config),
            UserEvent::Highlight {
                generation,
                revision,
                spans,
            } => state.on_highlight(generation, revision, spans),
            UserEvent::GitStatus { panes, cwd, status } => {
                state.on_git_status(panes, cwd, status);
            }
            UserEvent::BranchesLoaded { pane, cwd, result } => {
                state.on_branches_loaded(pane, cwd, result);
            }
            UserEvent::BranchCheckedOut {
                pane,
                cwd,
                branch,
                result,
            } => {
                state.on_branch_checked_out(pane, cwd, branch, result);
            }
            UserEvent::WorkingDiffLoaded {
                pane,
                cwd,
                generation,
                result,
            } => {
                state.on_working_diff_loaded(pane, cwd, generation, result);
            }
            UserEvent::CompletionCommands(commands) => {
                state.on_completion_commands(commands, &self.config);
            }
            UserEvent::CompletionHistory(history) => {
                state.on_completion_history(history);
            }
            UserEvent::ShellProfiles(profiles) => {
                state.on_shell_profiles(profiles, &self.config);
            }
            UserEvent::ExplorerLoaded {
                root,
                scan_id,
                rows,
                error,
            } => {
                state.on_explorer_loaded(root, scan_id, rows, error, &self.config);
            }
            UserEvent::OpenCodeReview { .. }
            | UserEvent::OpenGitManager { .. }
            | UserEvent::ReviewListLoaded { .. }
            | UserEvent::ReviewLoaded { .. }
            | UserEvent::PullRequestsLoaded { .. }
            | UserEvent::GitSnapshotLoaded { .. }
            | UserEvent::GitStatusLoaded { .. }
            | UserEvent::GitDetailLoaded { .. }
            | UserEvent::GitOperationFinished { .. }
            | UserEvent::OpenAgentMode
            | UserEvent::FocusAgentPane { .. }
            | UserEvent::AgentsDetected { .. }
            | UserEvent::RefreshAgents
            | UserEvent::KillAgent { .. }
            | UserEvent::AgentKilled { .. }
            | UserEvent::SendAgentInput { .. }
            | UserEvent::LaunchAgent { .. }
            | UserEvent::MarkAgentSeen { .. }
            | UserEvent::MarkAllAgentsSeen
            | UserEvent::SetFontSize(_) => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let mut wait = Duration::from_millis(750);
        let agent_open = self.agent.is_some();

        if let Some(state) = &mut self.window {
            state.poll_background(&self.config, agent_open);
            state.poll_agent_watcher(&self.config, agent_open);
            state.autosave_session(&self.config);
            if state.search.as_ref().is_some_and(|search| search.dirty) {
                state.refresh_search();
            }
            if agent_open
                && state.agent_preview_dirty
                && state.last_agent_preview_sync.elapsed() >= Duration::from_millis(800)
            {
                state.agent_preview_dirty = false;
                state.last_agent_preview_sync = Instant::now();
                state.agents_dirty = true;
            }
            if state.agents_dirty {
                state.rebuild_agent_snapshot();
                state.window.request_redraw();
            }
            wait = state.next_wake();
        }

        if let Some(agent) = &mut self.agent {
            agent.tick();
            wait = wait.min(Duration::from_secs(1));
        }
        if agent_open {
            self.sync_agent_window();
        }

        if let Some(git) = &mut self.git {
            git.poll();
            wait = wait.min(git.next_wake());
        }

        self.poll_config_reload();
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + wait.min(Duration::from_secs(1)),
        ));
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = &mut self.window {
            state.save_session(&self.config, true);
        }
    }
}

struct ViewLayout {
    sidebar: SidebarLayout,
    panes: Vec<PaneLayout>,
    dividers: Vec<DividerLayout>,
    /// Right-side panel layout, used by the editor or the working diff.
    editor: Option<EditorLayout>,
}

impl ViewLayout {
    fn empty() -> Self {
        Self {
            sidebar: SidebarLayout::empty(),
            panes: Vec::new(),
            dividers: Vec::new(),
            editor: None,
        }
    }
}

/// Layout shared by the editor and the embedded working-diff panel.
#[derive(Clone, Copy, Debug)]
struct EditorLayout {
    panel: Rect,
    header: Rect,
    text: Rect,
    close: Rect,
    divider: Rect,
    split_area: Rect,
    padding: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Terminal,
    Editor,
    Diff,
}

#[derive(Clone, Copy, Debug)]
enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug)]
enum DragState {
    Split(u64),
    Editor,
}

#[derive(Clone, Copy, Debug)]
struct SidebarDrag {
    tab: TabId,
    start_y: f32,
    active: bool,
    target: usize,
}

#[derive(Clone, Debug)]
struct SearchState {
    pane: PaneId,
    query: String,
    matches: Vec<std::ops::RangeInclusive<Point>>,
    current: usize,
    dirty: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct LinkHover {
    pane: PaneId,
    uri: String,
    range: std::ops::RangeInclusive<Point>,
}

struct WindowState {
    window: Arc<Window>,
    renderer: Renderer,
    workspace: Workspace,
    proxy: EventLoopProxy<UserEvent>,
    clipboard: Option<arboard::Clipboard>,
    sidebar: Sidebar,
    sidebar_drag: Option<SidebarDrag>,
    search: Option<SearchState>,
    hovered_link: Option<LinkHover>,
    view: ViewLayout,
    modifiers: ModifiersState,
    window_focused: bool,
    cursor_position: PhysicalPosition<f64>,
    scroll_accumulators: HashMap<PaneId, f32>,
    drag: Option<DragState>,
    selecting: Option<PaneId>,
    last_click: Option<(Instant, Point, PaneId)>,
    processes: Option<System>,
    agent_watch_tx: Option<Sender<AgentWatchRequest>>,
    agent_watch_generation: u64,
    agent_watch_pending: bool,
    last_agent_poll: Instant,
    detected_agents: Vec<DetectedAgent>,
    external_agents: Vec<DetectedAgent>,
    agent_snapshot: Arc<AgentSnapshot>,
    agent_times: HashMap<AgentId, u64>,
    seen_agents: HashSet<AgentId>,
    agent_notifications: HashMap<PaneId, PaneNotification>,
    hook_log: agents::HookLog,
    hook_states: HashMap<PaneId, HookState>,
    agents_dirty: bool,
    agent_preview_dirty: bool,
    last_agent_preview_sync: Instant,
    explorer: FileExplorer,
    explorer_scan_id: u64,
    explorer_pinned: bool,
    explorer_pane: Option<PaneId>,
    editor: Option<Editor>,
    highlight_tx: Option<Sender<HighlightRequest>>,
    highlight_generation: u64,
    highlight_sent_generation: u64,
    highlight_revision_seen: u64,
    highlight_due: Option<Instant>,
    dirty_panes: HashSet<PaneId>,
    frame_cache: HashMap<PaneId, Arc<Frame>>,
    frame_dominant: HashMap<PaneId, Rgb>,
    editor_frame_cache: Option<Arc<Frame>>,
    diff_frame_cache: Option<Arc<Frame>>,
    quad_scratch: Vec<Quad>,
    label_scratch: Vec<Label>,
    editor_dirty: bool,
    last_cwd_poll: Instant,
    last_git_poll: Instant,
    git_in_flight: HashSet<PaneId>,
    footer_hits: HashMap<PaneId, FooterHits>,
    prompt_trims: HashMap<PaneId, Vec<PromptTrim>>,
    branch_menu: Option<BranchMenu>,
    diff_view: Option<DiffView>,
    diff_generation: u64,
    editor_ratio: f32,
    pending_spawns: HashMap<PaneId, PendingSpawn>,
    shells: ShellCatalog,
    focus: Focus,
    completion: complete::Engine,
    completion_view: Option<CompletionView>,
    completion_open: bool,
    completion_selected: usize,
    popup_occlusion: Option<(PaneId, usize, usize, usize, usize)>,
    session_writer: Option<SessionWriter>,
    next_session_key: u64,
    last_session_save: Instant,
    last_scrollback_save: Instant,
    scrollback_queue: Vec<PaneId>,
    last_saved_session: Option<SessionState>,
    saved_scrollback: HashMap<PaneId, u64>,
}

#[derive(Clone, Debug)]
struct CompletionView {
    pane: PaneId,
    row: usize,
    col: usize,
    /// Command text after the prompt, used to reject stale suggestions.
    line: String,
    cwd: PathBuf,
    options: complete::Options,
    candidates: Vec<complete::Candidate>,
    typed_line: usize,
    typed_token: usize,
    right_limit: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct FooterHits {
    branch: Option<Rect>,
    changes: Option<Rect>,
}

#[derive(Clone, Copy, Debug)]
struct PromptTrim {
    row: usize,
    columns: usize,
}

#[derive(Clone, Debug)]
struct PaneNotification {
    title: Option<String>,
    message: Option<String>,
    at: Instant,
}

#[derive(Clone, Debug)]
struct HookState {
    state: AgentState,
    message: Option<String>,
    at: Instant,
}

#[derive(Clone, Debug)]
struct BranchMenu {
    pane: PaneId,
    cwd: PathBuf,
    branches: Vec<BranchInfo>,
    selected: usize,
    loading: bool,
    error: Option<String>,
    busy: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct BranchMenuLayout {
    rect: Rect,
    list: Rect,
    row_height: f32,
    visible_rows: usize,
    offset: usize,
    message: Option<Rect>,
}

type FrameEntry = (PaneId, Rect, Arc<Frame>, Rgb, Option<Vec<usize>>);
type BuiltFrame = (PaneId, Arc<Frame>, Rgb, Option<Vec<PromptTrim>>);

#[derive(Clone, Debug)]
struct DiffView {
    pane: PaneId,
    cwd: PathBuf,
    generation: u64,
    loading: bool,
    error: Option<String>,
    detail: Option<ReviewDetail>,
    rows: Vec<DiffRow>,
    max_cols: usize,
    scroll_row: usize,
    scroll_col: usize,
}

#[derive(Clone, Debug)]
struct PendingSpawn {
    cwd: PathBuf,
    profile_id: String,
    session_key: u64,
    scrollback: Option<PathBuf>,
    initial_command: Option<String>,
}

struct RestoredWorkspace {
    pending: HashMap<PaneId, PendingSpawn>,
    next_key: u64,
    collapsed: HashSet<TabId>,
}

fn restore_workspace(
    workspace: &mut Workspace,
    shells: &ShellCatalog,
    session: Option<&SessionState>,
    default_dir: &Path,
    restore_scrollback: bool,
) -> RestoredWorkspace {
    let mut pending = HashMap::new();
    let mut next_key = 1u64;
    let mut collapsed = HashSet::new();

    if let Some(session) = session {
        for tab in &session.tabs {
            if tab.panes.is_empty() {
                continue;
            }
            let layout = convert_saved_layout(&tab.layout);
            let Some((_tab_id, pane_ids)) = workspace.restore_terminal(
                &layout,
                tab.panes.len(),
                tab.focused,
                tab.title.clone(),
            ) else {
                continue;
            };

            for (pane_id, saved) in pane_ids.iter().zip(&tab.panes) {
                let profile_id = if shells.get(&saved.profile_id).is_some() {
                    saved.profile_id.clone()
                } else {
                    shells.default_id.clone()
                };
                let cwd = saved
                    .cwd
                    .as_deref()
                    .map(PathBuf::from)
                    .filter(|path| restore_cwd(path))
                    .unwrap_or_else(|| default_dir.to_path_buf());
                let scrollback = restore_scrollback
                    .then(|| session::scrollback_path(saved.key))
                    .flatten()
                    .filter(|path| path.is_file());
                pending.insert(
                    *pane_id,
                    PendingSpawn {
                        cwd,
                        profile_id,
                        session_key: saved.key,
                        scrollback,
                        initial_command: None,
                    },
                );
                next_key = next_key.max(saved.key.saturating_add(1));
            }
        }

        for index in &session.collapsed_tabs {
            if let Some(tab) = workspace.tabs.get(*index) {
                collapsed.insert(tab.id);
            }
        }
        if !workspace.tabs.is_empty() {
            workspace.select_index(session.active_tab.min(workspace.tabs.len() - 1));
        }
    }

    if workspace.tabs.is_empty() {
        let (_tab_id, pane_id) = workspace.create_terminal();
        pending.insert(
            pane_id,
            PendingSpawn {
                cwd: default_dir.to_path_buf(),
                profile_id: shells.default_id.clone(),
                session_key: next_key,
                scrollback: None,
                initial_command: None,
            },
        );
        next_key += 1;
    }

    RestoredWorkspace {
        pending,
        next_key,
        collapsed,
    }
}

fn read_scrollbacks(paths: Vec<(PaneId, PathBuf)>) -> HashMap<PaneId, Vec<u8>> {
    if paths.len() == 1 {
        let (pane, path) = paths.into_iter().next().unwrap();
        return session::read_scrollback(&path)
            .map(|bytes| (pane, bytes))
            .into_iter()
            .collect();
    }

    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .into_iter()
            .map(|(pane, path)| scope.spawn(move || (pane, session::read_scrollback(&path))))
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| {
                let (pane, bytes) = handle.join().ok()?;
                bytes.map(|bytes| (pane, bytes))
            })
            .collect()
    })
}

fn convert_saved_layout(layout: &SavedLayout) -> crate::workspace::LayoutSpec {
    use crate::workspace::LayoutSpec;

    match layout {
        SavedLayout::Leaf { index } => LayoutSpec::Leaf(*index),
        SavedLayout::Split {
            direction,
            ratio,
            first,
            second,
        } => LayoutSpec::Split {
            direction: match direction {
                SavedDirection::Right => SplitDirection::Right,
                SavedDirection::Down => SplitDirection::Down,
            },
            ratio: *ratio,
            first: Box::new(convert_saved_layout(first)),
            second: Box::new(convert_saved_layout(second)),
        },
    }
}

fn saved_layout(
    layout: &crate::workspace::LayoutNode,
    indices: &HashMap<PaneId, usize>,
) -> Option<SavedLayout> {
    use crate::workspace::LayoutNode;

    match layout {
        LayoutNode::Leaf(pane) => Some(SavedLayout::Leaf {
            index: *indices.get(pane)?,
        }),
        LayoutNode::Split(split) => Some(SavedLayout::Split {
            direction: match split.direction {
                SplitDirection::Right => SavedDirection::Right,
                SplitDirection::Down => SavedDirection::Down,
            },
            ratio: split.ratio,
            first: Box::new(saved_layout(&split.first, indices)?),
            second: Box::new(saved_layout(&split.second, indices)?),
        }),
    }
}

fn restore_cwd(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with(r"\\wsl$\") || text.starts_with(r"\\wsl.localhost\") || path.is_dir()
}

fn position_is_visible(event_loop: &ActiveEventLoop, x: i32, y: i32) -> bool {
    event_loop.available_monitors().any(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        let right = position.x.saturating_add(size.width as i32);
        let bottom = position.y.saturating_add(size.height as i32);
        x >= position.x && x < right && y >= position.y && y < bottom
    })
}

impl WindowState {
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    fn shared_render_resources(&self) -> SharedRenderResources {
        self.renderer.shared_resources()
    }

    fn new(
        event_loop: &ActiveEventLoop,
        config: &Config,
        proxy: EventLoopProxy<UserEvent>,
    ) -> anyhow::Result<Self> {
        let startup = Instant::now();
        let saved_session = config.restore_session.then(session::load).flatten();
        let saved_window = saved_session
            .as_ref()
            .and_then(|session| session.window.as_ref());

        let mut attributes = Window::default_attributes()
            .with_title("F-Terminal")
            .with_inner_size(LogicalSize::new(1100.0, 700.0))
            .with_min_inner_size(LogicalSize::new(520.0, 260.0))
            .with_theme(Some(WindowTheme::Dark))
            .with_visible(false);
        if let Some(saved) = saved_window {
            if saved.width >= 520.0 && saved.height >= 260.0 {
                attributes =
                    attributes.with_inner_size(LogicalSize::new(saved.width, saved.height));
            }
            if let (Some(x), Some(y)) = (saved.x, saved.y)
                && position_is_visible(event_loop, x, y)
            {
                attributes = attributes.with_position(PhysicalPosition::new(x, y));
            }
        }
        let window = Arc::new(event_loop.create_window(attributes)?);
        window.set_ime_allowed(true);
        if saved_window.is_some_and(|saved| saved.maximized) {
            window.set_maximized(true);
        }

        let renderer = pollster::block_on(Renderer::new(window.clone(), event_loop, config, None))?;
        log::info!("renderer ready in {:?}", startup.elapsed());

        let working_directory = default_directory();
        log::info!("working directory: {}", working_directory.display());
        let shells = ShellCatalog::detect(config.shell.as_deref());
        log::info!(
            "detected {} shell profile(s), default: {}",
            shells.profiles.len(),
            shells.default_profile().name
        );
        log::debug!(
            "shell profiles: {}",
            shells
                .profiles
                .iter()
                .map(|profile| profile.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let mut workspace = Workspace::new();
        let restored = restore_workspace(
            &mut workspace,
            &shells,
            saved_session.as_ref(),
            &working_directory,
            config.restore_scrollback,
        );
        let pending_spawns = restored.pending;
        let next_session_key = restored.next_key;
        let collapsed_tabs = restored.collapsed;
        let explorer_root = workspace
            .focused_pane()
            .and_then(|id| pending_spawns.get(&id))
            .map(|spawn| spawn.cwd.clone())
            .unwrap_or_else(|| working_directory.clone());

        let highlight_tx = highlight::spawn_worker(proxy.clone());
        let agent_watch_tx = if config.agents.enabled {
            agents::spawn_watcher(proxy.clone())
        } else {
            None
        };
        let completion = complete::Engine::new(false);
        let session_writer = if config.restore_session {
            SessionWriter::new()
        } else {
            None
        };

        let now = Instant::now();
        let mut state = Self {
            window,
            renderer,
            workspace,
            proxy,
            clipboard: None,
            sidebar: Sidebar::default(),
            sidebar_drag: None,
            search: None,
            hovered_link: None,
            view: ViewLayout::empty(),
            modifiers: ModifiersState::empty(),
            window_focused: true,
            cursor_position: PhysicalPosition::new(0.0, 0.0),
            scroll_accumulators: HashMap::new(),
            drag: None,
            selecting: None,
            last_click: None,
            processes: System::new().ok(),
            agent_watch_tx,
            agent_watch_generation: 0,
            agent_watch_pending: false,
            last_agent_poll: Instant::now()
                .checked_sub(Duration::from_millis(config.agents.poll_interval_ms))
                .unwrap_or_else(Instant::now),
            detected_agents: Vec::new(),
            external_agents: Vec::new(),
            agent_snapshot: Arc::new(AgentSnapshot::default()),
            agent_times: HashMap::new(),
            seen_agents: HashSet::new(),
            agent_notifications: HashMap::new(),
            hook_log: agents::HookLog::new(),
            hook_states: HashMap::new(),
            agents_dirty: true,
            agent_preview_dirty: false,
            last_agent_preview_sync: Instant::now(),
            explorer: FileExplorer::empty(explorer_root),
            explorer_scan_id: 0,
            explorer_pinned: false,
            explorer_pane: None,
            editor: None,
            highlight_tx,
            highlight_generation: 0,
            highlight_sent_generation: 0,
            highlight_revision_seen: 0,
            highlight_due: None,
            dirty_panes: HashSet::new(),
            frame_cache: HashMap::new(),
            frame_dominant: HashMap::new(),
            editor_frame_cache: None,
            diff_frame_cache: None,
            quad_scratch: Vec::new(),
            label_scratch: Vec::new(),
            editor_dirty: false,
            last_cwd_poll: Instant::now(),
            last_git_poll: Instant::now(),
            git_in_flight: HashSet::new(),
            footer_hits: HashMap::new(),
            prompt_trims: HashMap::new(),
            branch_menu: None,
            diff_view: None,
            diff_generation: 0,
            editor_ratio: 0.42,
            pending_spawns,
            shells,
            focus: Focus::Terminal,
            completion,
            completion_view: None,
            completion_open: false,
            completion_selected: 0,
            popup_occlusion: None,
            session_writer,
            next_session_key,
            last_session_save: now,
            last_scrollback_save: now,
            scrollback_queue: Vec::new(),
            last_saved_session: None,
            saved_scrollback: HashMap::new(),
        };
        state.sidebar.collapsed_tabs = collapsed_tabs;

        if let Err(error) = state.sync_layout(config) {
            log::warn!("could not restore the previous session: {error:#}; starting fresh");
            state.workspace = Workspace::new();
            state.pending_spawns.clear();
            state.saved_scrollback.clear();
            state.sidebar.collapsed_tabs.clear();
            state.explorer = FileExplorer::empty(working_directory.clone());
            let (_tab_id, pane_id) = state.workspace.create_terminal();
            let session_key = state.alloc_session_key();
            state.pending_spawns.insert(
                pane_id,
                PendingSpawn {
                    cwd: working_directory,
                    profile_id: state.shells.default_id.clone(),
                    session_key,
                    scrollback: None,
                    initial_command: None,
                },
            );
            state.sync_layout(config)?;
        }
        state.update_window_title();

        // Paint once while the window is still hidden so Windows never shows
        // an unpainted white frame. Rendering a second time after showing
        // covers platforms that ignore presents on hidden surfaces.
        state.on_redraw(config);
        state.window.set_visible(true);
        state.on_redraw(config);
        state.window.request_redraw();
        state.start_deferred_workers(config);

        log::info!("first frame in {:?}", startup.elapsed());
        log::info!(
            "started with {}x{} grid, cell {:.2}x{:.2}px, scale {:.1}",
            state
                .workspace
                .focused_pane()
                .and_then(|id| state.workspace.pane(id))
                .map(|pane| pane.grid.cols)
                .unwrap_or(0),
            state
                .workspace
                .focused_pane()
                .and_then(|id| state.workspace.pane(id))
                .map(|pane| pane.grid.lines)
                .unwrap_or(0),
            state.renderer.cell_metrics().width,
            state.renderer.cell_metrics().height,
            state.window.scale_factor()
        );

        Ok(state)
    }

    /// Start work that is not needed for the first frame. These workers only
    /// touch the UI through `UserEvent`s, so startup stays responsive.
    fn start_deferred_workers(&mut self, config: &Config) {
        if config.completions && config.completion_history {
            let proxy = self.proxy.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("completion-history".into())
                .spawn(move || {
                    let history = History::load_standard();
                    let _ = proxy.send_event(UserEvent::CompletionHistory(history));
                })
            {
                log::warn!("failed to start the completion history worker: {error}");
            }
        }

        if config.completions && config.completion_commands {
            let proxy = self.proxy.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("completion-index".into())
                .spawn(move || {
                    let commands = complete::commands::index_path();
                    let _ = proxy.send_event(UserEvent::CompletionCommands(commands));
                })
            {
                log::warn!("failed to start the completion index worker: {error}");
            }
        }

        let root = self.explorer.root.clone();
        let proxy = self.proxy.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("explorer-scan".into())
            .spawn(move || {
                let (rows, error) = FileExplorer::scan(&root, &HashSet::new());
                let _ = proxy.send_event(UserEvent::ExplorerLoaded {
                    root,
                    scan_id: 0,
                    rows,
                    error,
                });
            })
        {
            log::warn!("failed to start the explorer scan worker: {error}");
            self.explorer.rebuild();
        }

        #[cfg(windows)]
        {
            let proxy = self.proxy.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("wsl-profiles".into())
                .spawn(move || {
                    let profiles = crate::shell::detect_extra_profiles();
                    let _ = proxy.send_event(UserEvent::ShellProfiles(profiles));
                })
            {
                log::warn!("failed to start the WSL profile worker: {error}");
            }
        }
    }

    fn on_window_event(
        &mut self,
        event: WindowEvent,
        event_loop: &ActiveEventLoop,
        config: &Config,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                self.renderer.resize(size);
                self.relayout(config);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.renderer.set_scale_factor(scale_factor, config);
                self.relayout(config);
            }
            WindowEvent::Occluded(false) => self.window.request_redraw(),
            WindowEvent::CursorLeft { .. } => self.clear_link_hover(),
            WindowEvent::RedrawRequested => self.on_redraw(config),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.on_key(event, event_loop, config);
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if self.search.is_some() {
                    self.search_push(&text);
                    self.window.request_redraw();
                } else if self.focus == Focus::Editor && self.editor.is_some() {
                    if let Some(editor) = &mut self.editor {
                        editor.insert_str(&text);
                    }
                    self.editor_dirty = true;
                    self.ensure_editor_visible();
                    self.window.request_redraw();
                } else if self.focus == Focus::Terminal
                    && let Some(pane) = self.focused_pane_mut()
                {
                    pane.session.write(text.as_bytes());
                    pane.suggestion_armed = true;
                }
            }
            WindowEvent::Ime(Ime::Preedit(_text, _cursor)) => {
                // TODO: draw the preedit overlay at the cursor position.
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = position;
                self.on_cursor_moved(position, config);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse_input(state, button, event_loop, config);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.on_mouse_wheel(delta, config);
            }
            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                if focused {
                    let now = Instant::now();
                    self.last_cwd_poll = now.checked_sub(Duration::from_millis(750)).unwrap_or(now);
                    self.last_git_poll = now.checked_sub(GIT_STATUS_POLL_INTERVAL).unwrap_or(now);
                }

                let send_focus = self
                    .focused_pane()
                    .is_some_and(|pane| pane.terminal.mode().contains(TermMode::FOCUS_IN_OUT));
                if send_focus && let Some(pane) = self.focused_pane_mut() {
                    pane.session
                        .write(if focused { b"\x1b[I" } else { b"\x1b[O" });
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // Layout
    // ------------------------------------------------------------------

    fn sync_layout(&mut self, config: &Config) -> anyhow::Result<()> {
        let size = self.window.inner_size();
        let width = size.width.max(1) as f32;
        let height = size.height.max(1) as f32;
        let scale = self.renderer.scale_factor() as f32;
        let metrics = self.renderer.cell_metrics();

        let active_id = self.workspace.active_tab_id();
        let summaries: Vec<TabSummary> = self
            .workspace
            .tabs
            .iter()
            .map(|tab| TabSummary {
                id: tab.id,
                focused: tab.focused,
                panes: tab.leaves(),
            })
            .collect();
        self.sidebar
            .collapsed_tabs
            .retain(|id| self.workspace.tabs.iter().any(|tab| tab.id == *id));
        let sidebar_layout = sidebar::layout(
            &self.sidebar,
            &summaries,
            active_id,
            LayoutSpec {
                window_width: width,
                window_height: height,
                scale,
                width_logical: config.sidebar_width,
                file_count: self.explorer.rows.len(),
                profile_count: self.shells.profiles.len(),
            },
        );
        log::debug!(
            "sidebar tree: {} sessions, {} rows",
            summaries.len(),
            sidebar_layout.rows.len()
        );

        let clamped_scroll = self.sidebar.scroll.clamp(0.0, sidebar_layout.max_scroll);
        if (clamped_scroll - self.sidebar.scroll).abs() > 0.01 {
            self.sidebar.scroll = clamped_scroll;
        }

        let content_area = Rect::new(
            sidebar_layout.content_x,
            0.0,
            (width - sidebar_layout.content_x).max(1.0),
            height,
        );

        let Some(tab) = self.workspace.active_tab() else {
            self.view = ViewLayout::empty();
            return Ok(());
        };

        let gap = (2.0 * scale).round().max(1.0);
        let padding = self.renderer.physical(config.padding);
        let padding_left = self.renderer.physical(config.padding_left);

        let mut terminal_area = content_area;
        let mut editor_layout = None;

        if self.editor.is_some() || self.diff_view.is_some() {
            // The right-side panel keeps a small inset even when the terminal
            // is edge-to-edge, so text never collides with its frame.
            let editor_padding = padding.max(self.renderer.physical(10.0));
            let minimum_editor_width = (240.0 * scale).min(content_area.width);
            let maximum_editor_width =
                (content_area.width - 240.0 * scale).max(minimum_editor_width);
            let editor_width = (content_area.width * self.editor_ratio)
                .clamp(minimum_editor_width, maximum_editor_width);
            let terminal_width = (content_area.width - editor_width - gap).max(1.0);

            terminal_area = Rect::new(content_area.x, 0.0, terminal_width, height);

            let divider = Rect::new(content_area.x + terminal_width, 0.0, gap, height);
            let panel = Rect::new(
                content_area.x + terminal_width + gap,
                0.0,
                editor_width,
                height,
            );
            let header_height = (28.0 * scale).round();
            let header = Rect::new(panel.x, panel.y, panel.width, header_height);
            let close_size = (18.0 * scale).round();
            let close = Rect::new(
                panel.right() - close_size - editor_padding,
                panel.y + ((header_height - close_size) / 2.0).max(0.0),
                close_size,
                close_size,
            );
            let text = Rect::new(
                panel.x + editor_padding,
                header.bottom() + editor_padding,
                (panel.width - editor_padding * 2.0).max(1.0),
                (panel.height - header_height - editor_padding * 2.0).max(1.0),
            );

            if let Some(editor) = &mut self.editor {
                let rows = (text.height / metrics.height).floor().max(1.0) as usize;
                let cols = (text.width / metrics.width).floor().max(1.0) as usize;
                editor.ensure_visible(rows, cols);
            }
            self.editor_dirty = true;

            editor_layout = Some(EditorLayout {
                panel,
                header,
                text,
                close,
                divider,
                split_area: content_area,
                padding: editor_padding,
            });
        }

        let layout = compute_layout(
            &tab.layout,
            terminal_area,
            gap,
            padding,
            padding_left,
            // A compact but readable line for the Git bubbles.
            (26.0 * scale).round().max(18.0),
        );
        let pane_specs: Vec<(PaneId, Rect)> = layout
            .panes
            .iter()
            .map(|pane| (pane.id, pane.content))
            .collect();

        // Read the saved history for the panes that are about to spawn in
        // parallel. Inactive tabs are not touched until they are selected.
        let mut scrollbacks = HashMap::new();
        let mut pending_scrollbacks = Vec::new();
        for (pane_id, _) in &pane_specs {
            if self.workspace.pane(*pane_id).is_some() {
                continue;
            }
            if let Some(spawn) = self.pending_spawns.get(pane_id)
                && let Some(path) = &spawn.scrollback
            {
                pending_scrollbacks.push((*pane_id, path.clone()));
            }
        }
        if !pending_scrollbacks.is_empty() {
            scrollbacks = read_scrollbacks(pending_scrollbacks);
        }

        let mut spawned_panes = Vec::new();
        for (pane_id, content) in &pane_specs {
            let grid = grid_size_for(content.width, content.height, metrics);
            if let Some(pane) = self.workspace.pane_mut(*pane_id) {
                if pane.resize(grid) {
                    self.dirty_panes.insert(*pane_id);
                    if self
                        .search
                        .as_ref()
                        .is_some_and(|search| search.pane == *pane_id)
                        && let Some(search) = &mut self.search
                    {
                        search.dirty = true;
                    }
                }
            } else {
                let spawn = match self.pending_spawns.remove(pane_id) {
                    Some(spawn) => spawn,
                    None => {
                        let session_key = self.alloc_session_key();
                        PendingSpawn {
                            cwd: default_directory(),
                            profile_id: self.shells.default_id.clone(),
                            session_key,
                            scrollback: None,
                            initial_command: None,
                        }
                    }
                };
                let profile = self
                    .shells
                    .get(&spawn.profile_id)
                    .cloned()
                    .unwrap_or_else(|| self.shells.default_profile().clone());
                let restore = crate::workspace::PaneRestore {
                    session_key: spawn.session_key,
                    scrollback: scrollbacks.get(pane_id).map(Vec::as_slice),
                };
                // Reuse the status of a sibling in the same directory so the
                // bubbles appear immediately; a fresh request runs below.
                let cached_status = self
                    .workspace
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.panes.values())
                    .find(|pane| pane.cwd.as_deref() == Some(spawn.cwd.as_path()))
                    .and_then(|pane| pane.git_status.clone());
                let mut pane = match Pane::new(
                    *pane_id,
                    grid,
                    self.proxy.clone(),
                    &profile,
                    spawn.cwd.clone(),
                    restore,
                    config.scrollback_lines,
                ) {
                    Ok(pane) => pane,
                    Err(error) => {
                        log::warn!(
                            "failed to spawn pane {} with {} in {}: {error:#}; retrying with defaults",
                            pane_id.0,
                            profile.name,
                            spawn.cwd.display()
                        );
                        let fallback = self.shells.default_profile().clone();
                        Pane::new(
                            *pane_id,
                            grid,
                            self.proxy.clone(),
                            &fallback,
                            default_directory(),
                            restore,
                            config.scrollback_lines,
                        )
                        .with_context(|| {
                            format!("failed to spawn fallback shell for pane {}", pane_id.0)
                        })?
                    }
                };
                if pane.cwd.as_deref() == Some(spawn.cwd.as_path()) {
                    pane.git_status = cached_status;
                }
                if let Some(command) = spawn.initial_command {
                    pane.session.write(format!("{command}\r").as_bytes());
                }
                self.workspace.insert_pane(pane);
                spawned_panes.push(*pane_id);
            }
        }

        self.view = ViewLayout {
            sidebar: sidebar_layout,
            panes: layout.panes,
            dividers: layout.dividers,
            editor: editor_layout,
        };

        for pane_id in spawned_panes {
            self.request_git_status(pane_id);
        }

        self.apply_scrollback_policy(config);

        Ok(())
    }

    fn apply_scrollback_policy(&mut self, config: &Config) {
        let active = self.workspace.active_tab_id();
        let focused = self.workspace.focused_pane();

        let mut panes = Vec::new();
        for tab in &self.workspace.tabs {
            let is_active = Some(tab.id) == active;
            for pane in tab.panes.values() {
                panes.push(ScrollbackPane {
                    id: pane.id,
                    cols: pane.grid.cols,
                    active: is_active,
                    focused: Some(pane.id) == focused,
                });
            }
        }
        panes.sort_by_key(|pane| pane.id.0);

        let limits = plan_scrollback_limits(&panes, config);

        if config.scrollback_budget_mb > 0 {
            let budget = (config.scrollback_budget_mb as usize).saturating_mul(1024 * 1024);
            let estimated = panes.iter().fold(0usize, |total, pane| {
                let limit = limits.get(&pane.id).copied().unwrap_or(0);
                total.saturating_add(estimate_scrollback_bytes(pane.cols, limit))
            });
            log::debug!(
                "scrollback estimated at {:.1} MB with a {} MB budget",
                estimated as f64 / 1_048_576.0,
                config.scrollback_budget_mb
            );

            if estimated > budget {
                log::warn!(
                    "scrollback still above budget after trimming: {:.1} MB > {} MB",
                    estimated as f64 / 1_048_576.0,
                    config.scrollback_budget_mb
                );
            }
        }

        for (pane_id, limit) in limits {
            if let Some(pane) = self.workspace.pane_mut(pane_id) {
                pane.apply_scrollback_limit(limit);
            }
        }
    }

    fn apply_font_size(&mut self, config: &Config) {
        self.renderer.refresh_font_metrics(config);
        self.relayout(config);
    }

    fn relayout(&mut self, config: &Config) {
        // Pane frames do not depend on the viewport position; terminal content
        // changes are invalidated through `dirty_panes`/`Term::damage`.
        self.editor_frame_cache = None;
        self.diff_frame_cache = None;
        if self
            .diff_view
            .as_ref()
            .is_some_and(|diff| self.workspace.pane(diff.pane).is_none())
        {
            self.diff_view = None;
            self.focus = if self.editor.is_some() {
                Focus::Editor
            } else {
                Focus::Terminal
            };
        }
        self.branch_menu = None;
        self.sync_explorer_to_focused();
        if let Err(error) = self.sync_layout(config) {
            log::error!("layout failed: {error:#}");
        }
        self.update_window_title();
        self.window.request_redraw();
    }

    fn update_window_title(&self) {
        let title = self
            .workspace
            .active_tab()
            .map(|tab| tab.title.as_str())
            .unwrap_or("F-Terminal");
        self.window.set_title(&format!("{title} — F-Terminal"));
    }

    // ------------------------------------------------------------------
    // Actions
    // ------------------------------------------------------------------

    fn alloc_session_key(&mut self) -> u64 {
        let key = self.next_session_key;
        self.next_session_key = self.next_session_key.wrapping_add(1).max(1);
        key
    }

    fn new_terminal(&mut self, config: &Config) {
        let profile_id = self.shells.default_id.clone();
        self.new_terminal_with(&profile_id, config);
    }

    fn new_terminal_with(&mut self, profile_id: &str, config: &Config) {
        self.close_search();
        self.clear_link_hover();
        self.refresh_cwds(config);
        let cwd = self.focused_cwd();
        let (tab_id, pane_id) = self.workspace.create_terminal();
        let session_key = self.alloc_session_key();
        self.pending_spawns.insert(
            pane_id,
            PendingSpawn {
                cwd,
                profile_id: profile_id.to_owned(),
                session_key,
                scrollback: None,
                initial_command: None,
            },
        );
        self.sidebar.profile_menu = false;
        self.sidebar.hovered_profile = None;
        self.focus = Focus::Terminal;
        self.branch_menu = None;
        // New terminals are appended to the sidebar; reveal the newest one.
        self.sidebar.scroll = f32::MAX;

        if let Err(error) = self.sync_layout(config) {
            log::error!("failed to create terminal: {error:#}");
            self.pending_spawns.remove(&pane_id);
            self.workspace.close_tab(tab_id);
            let _ = self.sync_layout(config);
        }
        self.save_session(config, false);

        self.update_window_title();
        self.window.request_redraw();
    }

    fn split_focused(&mut self, direction: SplitDirection, config: &Config) {
        self.close_search();
        self.clear_link_hover();
        self.refresh_cwds(config);
        let cwd = self.focused_cwd();
        let profile_id = self
            .workspace
            .focused_pane()
            .and_then(|id| self.workspace.pane(id))
            .map(|pane| pane.profile_id.clone())
            .unwrap_or_else(|| self.shells.default_id.clone());
        let Some(new_pane) = self.workspace.split_focused(direction) else {
            return;
        };
        if let Some(tab) = self.workspace.active_tab_id() {
            self.sidebar.collapsed_tabs.remove(&tab);
        }
        let session_key = self.alloc_session_key();
        self.pending_spawns.insert(
            new_pane,
            PendingSpawn {
                cwd,
                profile_id,
                session_key,
                scrollback: None,
                initial_command: None,
            },
        );
        self.focus = Focus::Terminal;

        if let Err(error) = self.sync_layout(config) {
            log::error!("failed to split pane: {error:#}");
            self.pending_spawns.remove(&new_pane);
            self.workspace.close_pane(new_pane);
            let _ = self.sync_layout(config);
        }
        self.save_session(config, false);

        self.update_window_title();
        self.window.request_redraw();
    }

    fn close_focused_pane(&mut self, event_loop: &ActiveEventLoop, config: &Config) {
        if let Some(pane) = self.workspace.focused_pane() {
            self.close_pane(pane, event_loop, config);
        }
    }

    fn close_pane(&mut self, pane: PaneId, event_loop: &ActiveEventLoop, config: &Config) {
        if self
            .search
            .as_ref()
            .is_some_and(|search| search.pane == pane)
        {
            self.close_search();
        }
        self.clear_link_hover();
        match self.workspace.close_pane(pane) {
            CloseOutcome::NotFound => {}
            CloseOutcome::WorkspaceEmpty => {
                self.save_session(config, true);
                event_loop.exit();
            }
            CloseOutcome::PaneClosed | CloseOutcome::TabClosed => {
                self.pending_spawns.remove(&pane);
                self.saved_scrollback.remove(&pane);
                self.scroll_accumulators.remove(&pane);
                self.git_in_flight.remove(&pane);
                self.selecting = None;
                self.sidebar.hovered = None;
                self.agents_dirty = true;
                self.relayout(config);
                self.save_session(config, false);
            }
        }
    }

    fn close_tab(&mut self, tab: TabId, event_loop: &ActiveEventLoop, config: &Config) {
        self.close_search();
        self.clear_link_hover();
        match self.workspace.close_tab(tab) {
            CloseOutcome::NotFound => {}
            CloseOutcome::WorkspaceEmpty => {
                self.save_session(config, true);
                event_loop.exit();
            }
            CloseOutcome::TabClosed | CloseOutcome::PaneClosed => {
                self.selecting = None;
                self.sidebar.hovered = None;
                self.agents_dirty = true;
                self.relayout(config);
                self.save_session(config, false);
            }
        }
    }

    fn toggle_workspace(&mut self, tab: TabId, config: &Config) {
        if !self.sidebar.collapsed_tabs.remove(&tab) {
            self.sidebar.collapsed_tabs.insert(tab);
        }
        self.relayout(config);
    }

    fn select_pane(&mut self, tab: TabId, pane: PaneId, config: &Config) {
        self.close_search();
        self.clear_link_hover();
        self.workspace.select_tab(tab);
        if self.workspace.focus_pane(pane) {
            self.focus = Focus::Terminal;
            self.mark_pane_agents_seen(pane);
            self.agents_dirty = true;
            self.sync_explorer_to_focused();
            self.update_window_title();
            self.relayout(config);
        }
    }

    fn select_tab(&mut self, tab: TabId, config: &Config) {
        self.close_search();
        self.clear_link_hover();
        if self.workspace.select_tab(tab) {
            self.focus = Focus::Terminal;
            self.relayout(config);
        }
    }

    fn select_tab_index(&mut self, index: usize, config: &Config) {
        if self.workspace.select_index(index) {
            self.focus = Focus::Terminal;
            self.relayout(config);
        }
    }

    fn next_terminal(&mut self, delta: i32, config: &Config) {
        if self.workspace.next_terminal(delta) {
            self.focus = Focus::Terminal;
            self.relayout(config);
        }
    }

    fn dispatch_menu(&mut self, action: menu::Action, config: &Config) {
        self.refresh_cwds(config);
        let cwd = self.focused_cwd();
        match action {
            menu::Action::Explorer => menu::open_explorer(&cwd),
            menu::Action::OpenCode => menu::open_in_code(&cwd),
            menu::Action::CodeReview => {
                let _ = self.proxy.send_event(UserEvent::OpenCodeReview { cwd });
            }
            menu::Action::GitManager => {
                let _ = self.proxy.send_event(UserEvent::OpenGitManager { cwd });
            }
            menu::Action::AgentMode => {
                let _ = self.proxy.send_event(UserEvent::OpenAgentMode);
            }
            menu::Action::Settings => self.open_settings(config),
        }
    }

    pub(crate) fn focused_cwd(&self) -> PathBuf {
        self.workspace
            .focused_pane()
            .and_then(|id| self.workspace.pane(id))
            .and_then(|pane| pane.cwd.clone())
            .unwrap_or_else(default_directory)
    }

    fn sync_explorer_to_focused(&mut self) {
        let focused = self.workspace.focused_pane();
        if self.explorer_pane != focused {
            self.explorer_pane = focused;
            self.explorer_pinned = false;
        }
        if self.explorer_pinned {
            return;
        }
        let cwd = self.focused_cwd();
        if self.explorer.prepare_root(cwd) {
            self.request_explorer_scan();
        }
    }

    fn explorer_go_up(&mut self, config: &Config) {
        let Some(parent) = self.explorer.root.parent().map(Path::to_path_buf) else {
            return;
        };
        if self.explorer.prepare_root(parent) {
            self.request_explorer_scan();
        }
        self.explorer_pane = self.workspace.focused_pane();
        self.explorer_pinned = true;
        self.relayout(config);
    }

    /// Poll the focused child process cwd so the file explorer and the menu
    /// actions follow `cd`.
    fn refresh_cwds(&mut self, config: &Config) {
        let mut pids = Vec::new();
        for tab in &self.workspace.tabs {
            for pane in tab.panes.values() {
                if pane.cwd_from_shell {
                    continue;
                }
                if let Some(pid) = pane.session.process_id() {
                    pids.push(Pid::from_u32(pid));
                }
            }
        }
        if pids.is_empty() {
            return;
        }

        let Some(system) = self.processes.as_mut() else {
            return;
        };
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            false,
            ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
        );

        let mut updates = Vec::new();
        for tab in &self.workspace.tabs {
            for pane in tab.panes.values() {
                if pane.cwd_from_shell {
                    continue;
                }
                if let Some(pid) = pane.session.process_id()
                    && let Some(cwd) = system
                        .process(Pid::from_u32(pid))
                        .and_then(|process| process.cwd())
                {
                    updates.push((pane.id, cwd.to_path_buf()));
                }
            }
        }

        let focused = self.workspace.focused_pane();
        let mut focused_cwd = None;
        let mut cwd_changed = Vec::new();
        for (pane_id, cwd) in updates {
            if let Some(pane) = self.workspace.pane_mut(pane_id)
                && pane.cwd.as_ref() != Some(&cwd)
            {
                pane.cwd = Some(cwd.clone());
                cwd_changed.push(pane_id);
                if Some(pane_id) == focused {
                    focused_cwd = Some(cwd);
                }
            }
        }

        if let Some(cwd) = focused_cwd {
            log::debug!("focused cwd: {}", cwd.display());
            self.explorer_pinned = false;
            if self.explorer.prepare_root(cwd) {
                self.request_explorer_scan();
            }
            if self.sidebar.mode == SidebarMode::Files {
                self.relayout(config);
            }
        }

        for pane_id in cwd_changed {
            self.request_git_status(pane_id);
        }
    }

    fn request_git_status(&mut self, pane_id: PaneId) {
        let Some(pane) = self.workspace.pane(pane_id) else {
            return;
        };
        let cwd = pane.cwd.clone().unwrap_or_else(default_directory);
        self.request_git_status_group(vec![pane_id], cwd);
    }

    fn request_git_status_group(&mut self, panes: Vec<PaneId>, cwd: PathBuf) {
        let pending: Vec<PaneId> = panes
            .into_iter()
            .filter(|pane_id| !self.git_in_flight.contains(pane_id))
            .collect();
        if pending.is_empty() {
            return;
        }
        for pane_id in &pending {
            self.git_in_flight.insert(*pane_id);
        }

        let proxy = self.proxy.clone();
        let event_panes = pending.clone();
        let spawned = std::thread::Builder::new()
            .name("git-status".into())
            .spawn(move || {
                let status = git_status::inspect(&cwd);
                let _ = proxy.send_event(UserEvent::GitStatus {
                    panes: event_panes,
                    cwd,
                    status,
                });
            });

        if spawned.is_err() {
            for pane_id in &pending {
                self.git_in_flight.remove(pane_id);
            }
        }
    }

    fn refresh_git_statuses(&mut self) {
        self.git_in_flight
            .retain(|pane_id| self.workspace.pane(*pane_id).is_some());

        // Several splits commonly share a directory. Inspect each distinct cwd
        // once and fan the result out to every pane in that group.
        let mut groups: HashMap<PathBuf, Vec<PaneId>> = HashMap::new();
        for pane_layout in &self.view.panes {
            let Some(pane) = self.workspace.pane(pane_layout.id) else {
                continue;
            };
            let cwd = pane.cwd.clone().unwrap_or_else(default_directory);
            groups.entry(cwd).or_default().push(pane_layout.id);
        }
        for (cwd, panes) in groups {
            self.request_git_status_group(panes, cwd);
        }
    }

    fn on_git_status(&mut self, panes: Vec<PaneId>, cwd: PathBuf, status: Option<GitStatus>) {
        let mut changed_visible = false;
        let mut retry = Vec::new();

        for pane_id in &panes {
            self.git_in_flight.remove(pane_id);
            let Some(pane) = self.workspace.pane(*pane_id) else {
                continue;
            };
            if pane.cwd.as_ref() != Some(&cwd) {
                // The shell moved while the request was running; try again
                // with the pane's current directory.
                retry.push(*pane_id);
                continue;
            }

            let changed = pane.git_status != status;
            if changed && self.view.panes.iter().any(|view| view.id == *pane_id) {
                changed_visible = true;
            }
            if changed && self.pane_has_agent(*pane_id) {
                self.agents_dirty = true;
            }
            if let Some(pane) = self.workspace.pane_mut(*pane_id) {
                pane.git_status = status.clone();
            }
        }

        if changed_visible {
            self.window.request_redraw();
        }
        for pane_id in retry {
            self.request_git_status(pane_id);
        }
    }

    fn toggle_branch_menu(&mut self, pane_id: PaneId) {
        if self
            .branch_menu
            .as_ref()
            .is_some_and(|menu| menu.pane == pane_id)
        {
            self.branch_menu = None;
            self.window.request_redraw();
            return;
        }

        let Some(cwd) = self
            .workspace
            .pane(pane_id)
            .and_then(|pane| pane.cwd.clone())
        else {
            return;
        };

        self.clear_completion();
        self.branch_menu = Some(BranchMenu {
            pane: pane_id,
            cwd: cwd.clone(),
            branches: Vec::new(),
            selected: 0,
            loading: true,
            error: None,
            busy: None,
        });
        self.request_branches(pane_id, cwd);
        self.window.request_redraw();
    }

    fn request_branches(&mut self, pane_id: PaneId, cwd: PathBuf) {
        let proxy = self.proxy.clone();
        let thread_cwd = cwd.clone();
        let spawned = std::thread::Builder::new()
            .name("git-branches".into())
            .spawn(move || {
                let result = git_refs::load(&thread_cwd)
                    .map(|refs| refs.local_branches)
                    .map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::BranchesLoaded {
                    pane: pane_id,
                    cwd: thread_cwd,
                    result,
                });
            });

        if spawned.is_err()
            && let Some(menu) = self.branch_menu.as_mut()
            && menu.pane == pane_id
        {
            menu.loading = false;
            menu.error = Some("No se pudo leer las ramas de Git".to_owned());
        }
    }

    fn on_branches_loaded(
        &mut self,
        pane_id: PaneId,
        cwd: PathBuf,
        result: Result<Vec<BranchInfo>, String>,
    ) {
        let Some(menu) = self.branch_menu.as_mut() else {
            return;
        };
        if menu.pane != pane_id || menu.cwd != cwd {
            return;
        }

        menu.loading = false;
        match result {
            Ok(branches) => {
                menu.selected = branches
                    .iter()
                    .position(|branch| branch.current)
                    .unwrap_or(0);
                menu.branches = branches;
            }
            Err(error) => menu.error = Some(error),
        }
        self.window.request_redraw();
    }

    fn checkout_selected_branch(&mut self) {
        let Some(menu) = self.branch_menu.as_mut() else {
            return;
        };
        let Some(branch) = menu.branches.get(menu.selected) else {
            return;
        };
        if branch.current {
            self.branch_menu = None;
            self.window.request_redraw();
            return;
        }

        let name = branch.name.clone();
        menu.busy = Some(format!("Cambiando a {name}…"));
        menu.error = None;
        let pane_id = menu.pane;
        let cwd = menu.cwd.clone();

        let proxy = self.proxy.clone();
        let action = GitOpsAction::Checkout {
            branch: name.clone(),
            track: false,
            local: name.clone(),
        };
        let thread_cwd = cwd.clone();
        let thread_branch = name.clone();
        let spawned = std::thread::Builder::new()
            .name("git-checkout".into())
            .spawn(move || {
                let result =
                    git_ops::apply(&thread_cwd, &action).map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::BranchCheckedOut {
                    pane: pane_id,
                    cwd: thread_cwd,
                    branch: thread_branch,
                    result,
                });
            });

        if spawned.is_err()
            && let Some(menu) = self.branch_menu.as_mut()
            && menu.pane == pane_id
        {
            menu.busy = None;
            menu.error = Some("No se pudo iniciar el checkout de Git".to_owned());
        }
        self.window.request_redraw();
    }

    fn on_branch_checked_out(
        &mut self,
        pane_id: PaneId,
        cwd: PathBuf,
        branch: String,
        result: Result<String, String>,
    ) {
        match result {
            Ok(_) => {
                if self
                    .branch_menu
                    .as_ref()
                    .is_some_and(|menu| menu.pane == pane_id)
                {
                    self.branch_menu = None;
                }
                self.git_in_flight.remove(&pane_id);
                self.request_git_status(pane_id);
                if self
                    .diff_view
                    .as_ref()
                    .is_some_and(|diff| diff.pane == pane_id && diff.cwd == cwd)
                {
                    self.reload_diff_view();
                }
                self.window.request_redraw();
            }
            Err(error) => {
                log::warn!("checkout of {branch} failed: {error}");
                if let Some(menu) = self.branch_menu.as_mut()
                    && menu.pane == pane_id
                    && menu.cwd == cwd
                {
                    menu.busy = None;
                    menu.error = Some(error);
                }
                self.window.request_redraw();
            }
        }
    }

    fn toggle_diff_view(&mut self, pane_id: PaneId, config: &Config) {
        self.close_search();
        self.clear_link_hover();
        let Some(cwd) = self
            .workspace
            .pane(pane_id)
            .and_then(|pane| pane.cwd.clone())
        else {
            return;
        };

        if self
            .diff_view
            .as_ref()
            .is_some_and(|diff| diff.pane == pane_id && diff.cwd == cwd)
        {
            self.close_diff_view(config);
            return;
        }

        self.branch_menu = None;
        self.clear_completion();
        self.diff_generation = self.diff_generation.wrapping_add(1);
        let generation = self.diff_generation;
        log::debug!(
            "opening working diff for pane {} at {}",
            pane_id.0,
            cwd.display()
        );
        self.diff_view = Some(DiffView {
            pane: pane_id,
            cwd: cwd.clone(),
            generation,
            loading: true,
            error: None,
            detail: None,
            rows: Vec::new(),
            max_cols: 0,
            scroll_row: 0,
            scroll_col: 0,
        });
        self.focus = Focus::Diff;
        self.editor_dirty = true;
        if self.editor.is_none() {
            self.editor_ratio = 0.5;
        }
        self.relayout(config);
        self.request_working_diff(pane_id, cwd, generation);
    }

    fn request_working_diff(&mut self, pane_id: PaneId, cwd: PathBuf, generation: u64) {
        let proxy = self.proxy.clone();
        let thread_cwd = cwd.clone();
        let summary = ReviewSummary {
            title: "Cambios locales".to_owned(),
            subtitle: String::new(),
            kind: ReviewKind::Working,
        };
        let spawned = std::thread::Builder::new()
            .name("working-diff".into())
            .spawn(move || {
                let result = crate::git::repository_root(&thread_cwd)
                    .and_then(|root| review::load_detail(&root, &summary, &Default::default()))
                    .map(|detail| {
                        let rows = build_diff_rows(&detail);
                        let max_cols = rows
                            .iter()
                            .map(|row| row.text.chars().count())
                            .max()
                            .unwrap_or(0);
                        crate::event::WorkingDiffPayload {
                            detail,
                            rows,
                            max_cols,
                        }
                    })
                    .map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::WorkingDiffLoaded {
                    pane: pane_id,
                    cwd: thread_cwd,
                    generation,
                    result,
                });
            });

        if spawned.is_err()
            && let Some(diff) = self.diff_view.as_mut()
            && diff.generation == generation
        {
            diff.loading = false;
            diff.error = Some("No se pudo iniciar la lectura del diff".to_owned());
        }
    }

    fn on_working_diff_loaded(
        &mut self,
        pane_id: PaneId,
        cwd: PathBuf,
        generation: u64,
        result: Result<crate::event::WorkingDiffPayload, String>,
    ) {
        let Some(diff) = self.diff_view.as_mut() else {
            return;
        };
        if diff.pane != pane_id || diff.cwd != cwd || diff.generation != generation {
            return;
        }

        diff.loading = false;
        match result {
            Ok(payload) => {
                diff.rows = payload.rows;
                diff.max_cols = payload.max_cols;
                diff.scroll_row = 0;
                diff.scroll_col = 0;
                log::debug!(
                    "working diff loaded: {} file(s), {} row(s)",
                    payload.detail.files.len(),
                    diff.rows.len()
                );
                diff.detail = Some(payload.detail);
                diff.error = None;
            }
            Err(error) => {
                diff.rows.clear();
                diff.max_cols = 0;
                diff.detail = None;
                log::warn!("failed to load working diff: {error}");
                diff.error = Some(error);
            }
        }
        self.editor_dirty = true;
        self.window.request_redraw();
    }

    fn close_diff_view(&mut self, config: &Config) {
        if self.diff_view.take().is_some() {
            self.focus = if self.editor.is_some() {
                Focus::Editor
            } else {
                Focus::Terminal
            };
            self.editor_dirty = true;
            self.relayout(config);
        }
    }

    fn scroll_diff(&mut self, rows: f32, columns: f32) {
        let Some(diff) = self.diff_view.as_mut() else {
            return;
        };
        let metrics = self.renderer.cell_metrics();
        let visible_rows = self
            .view
            .editor
            .map(|layout| (layout.text.height / metrics.height).floor().max(1.0) as usize)
            .unwrap_or(1);
        let visible_cols = self
            .view
            .editor
            .map(|layout| (layout.text.width / metrics.width).floor().max(1.0) as usize)
            .unwrap_or(1);
        let max_row = diff.rows.len().saturating_sub(visible_rows);
        let max_col = diff.max_cols.saturating_sub(visible_cols);

        if rows != 0.0 {
            let target = (diff.scroll_row as f32 - rows).round();
            diff.scroll_row = target.clamp(0.0, max_row as f32) as usize;
        }
        if columns != 0.0 {
            let target = (diff.scroll_col as f32 - columns).round();
            diff.scroll_col = target.clamp(0.0, max_col as f32) as usize;
        }
        self.editor_dirty = true;
        self.window.request_redraw();
    }

    fn reload_diff_view(&mut self) {
        let Some(diff) = self.diff_view.as_ref() else {
            return;
        };
        let pane = diff.pane;
        let cwd = diff.cwd.clone();
        self.diff_generation = self.diff_generation.wrapping_add(1);
        let generation = self.diff_generation;
        if let Some(diff) = self.diff_view.as_mut() {
            diff.generation = generation;
            diff.loading = true;
            diff.error = None;
        }
        self.editor_dirty = true;
        self.request_working_diff(pane, cwd, generation);
        self.window.request_redraw();
    }

    fn handle_diff_key(&mut self, event: &winit::event::KeyEvent, config: &Config) -> bool {
        if event.state != ElementState::Pressed || self.diff_view.is_none() {
            return false;
        }

        let ctrl = self.modifiers.control_key();
        let super_key = self.modifiers.super_key();
        let command = ctrl || super_key;

        if command {
            if let PhysicalKey::Code(KeyCode::KeyW) = event.physical_key {
                self.close_diff_view(config);
                return true;
            }
            if let PhysicalKey::Code(KeyCode::KeyC) | PhysicalKey::Code(KeyCode::KeyA) =
                event.physical_key
            {
                return true;
            }
            return false;
        }

        let page = self
            .view
            .editor
            .map(|layout| {
                let metrics = self.renderer.cell_metrics();
                (layout.text.height / metrics.height).floor().max(1.0)
            })
            .unwrap_or(10.0);

        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.close_diff_view(config);
                return true;
            }
            Key::Named(NamedKey::ArrowUp) => self.scroll_diff(1.0, 0.0),
            Key::Named(NamedKey::ArrowDown) => self.scroll_diff(-1.0, 0.0),
            Key::Named(NamedKey::PageUp) => self.scroll_diff(page, 0.0),
            Key::Named(NamedKey::PageDown) => self.scroll_diff(-page, 0.0),
            Key::Named(NamedKey::Home) => self.scroll_diff(f32::MAX, 0.0),
            Key::Named(NamedKey::End) => self.scroll_diff(f32::MIN, 0.0),
            Key::Named(NamedKey::ArrowLeft) => self.scroll_diff(0.0, 1.0),
            Key::Named(NamedKey::ArrowRight) => self.scroll_diff(0.0, -1.0),
            Key::Character(character) if character.eq_ignore_ascii_case("r") => {
                self.reload_diff_view();
            }
            _ => {}
        }
        true
    }

    fn branch_menu_layout(&self, config: &Config) -> Option<BranchMenuLayout> {
        let menu = self.branch_menu.as_ref()?;
        let anchor = self.footer_hits.get(&menu.pane)?.branch?;
        let outer = self
            .view
            .panes
            .iter()
            .find(|pane| pane.id == menu.pane)?
            .outer;
        let scale = self.renderer.scale_factor() as f32;
        let ui_font_size = self.renderer.physical(config.ui_font_size);
        Some(branch_menu_layout(menu, anchor, outer, scale, ui_font_size))
    }

    fn handle_branch_menu_click(
        &mut self,
        state: ElementState,
        button: MouseButton,
        x: f32,
        y: f32,
        config: &Config,
    ) -> bool {
        let Some(layout) = self.branch_menu_layout(config) else {
            self.branch_menu = None;
            return false;
        };

        if layout.rect.contains(x, y) {
            let mut checkout = false;
            if state == ElementState::Pressed
                && button == MouseButton::Left
                && let Some(menu) = self.branch_menu.as_mut()
                && !menu.loading
                && menu.busy.is_none()
                && layout.list.contains(x, y)
            {
                let row = ((y - layout.list.y) / layout.row_height).floor().max(0.0) as usize;
                if row < layout.visible_rows {
                    let index = layout.offset + row;
                    if index < menu.branches.len() {
                        menu.selected = index;
                        checkout = true;
                    }
                }
            }
            if checkout {
                self.checkout_selected_branch();
            }
            return true;
        }

        // Clicking the branch pill again closes the popup. Release events are
        // consumed but must not close the menu that was just opened.
        let on_anchor = self
            .branch_menu
            .as_ref()
            .and_then(|menu| self.footer_hits.get(&menu.pane))
            .and_then(|hits| hits.branch)
            .is_some_and(|rect| rect.contains(x, y));
        if state == ElementState::Pressed {
            self.branch_menu = None;
            self.window.request_redraw();
        }
        on_anchor
    }

    fn handle_branch_menu_wheel(&mut self, delta: MouseScrollDelta, config: &Config) -> bool {
        let Some(layout) = self.branch_menu_layout(config) else {
            self.branch_menu = None;
            return false;
        };
        let (x, y) = (self.cursor_position.x as f32, self.cursor_position.y as f32);
        if !layout.rect.contains(x, y) {
            self.branch_menu = None;
            self.window.request_redraw();
            return false;
        }

        let rows = match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(position) => position.y as f32 / layout.row_height,
        };
        if rows != 0.0
            && let Some(menu) = self.branch_menu.as_mut()
            && !menu.branches.is_empty()
        {
            let selected = (menu.selected as f32 - rows.round())
                .clamp(0.0, (menu.branches.len().saturating_sub(1)) as f32);
            menu.selected = selected as usize;
            self.window.request_redraw();
        }
        true
    }

    fn toggle_sidebar(&mut self, config: &Config) {
        self.sidebar.visible = !self.sidebar.visible;
        self.sidebar.hovered = None;
        self.sidebar.hovered_file = None;
        self.relayout(config);
    }

    fn toggle_sidebar_mode(&mut self, config: &Config) {
        self.refresh_cwds(config);
        self.sidebar.mode = match self.sidebar.mode {
            SidebarMode::Terminals => SidebarMode::Files,
            SidebarMode::Files => SidebarMode::Terminals,
        };
        self.sidebar.scroll = 0.0;
        self.sidebar.hovered = None;
        self.sidebar.hovered_file = None;
        self.sidebar.profile_menu = false;
        self.sidebar.hovered_profile = None;

        if self.sidebar.mode == SidebarMode::Files {
            self.explorer_pinned = false;
            self.explorer.prepare_root(self.focused_cwd());
            self.request_explorer_scan();
        }

        self.relayout(config);
    }

    fn toggle_profile_menu(&mut self, config: &Config) {
        if self.sidebar.mode != SidebarMode::Terminals {
            self.sidebar.mode = SidebarMode::Terminals;
        }
        self.sidebar.profile_menu = !self.sidebar.profile_menu;
        self.sidebar.profile_selected = self.shells.index_of(&self.shells.default_id).unwrap_or(0);
        self.sidebar.hovered = None;
        self.sidebar.hovered_file = None;
        self.sidebar.hovered_profile = None;
        self.sidebar.scroll = 0.0;
        self.relayout(config);
    }

    fn select_profile(&mut self, index: usize, config: &Config) {
        let Some(profile) = self.shells.profiles.get(index) else {
            return;
        };
        let id = profile.id.clone();
        self.sidebar.profile_menu = false;
        self.sidebar.hovered_profile = None;
        self.new_terminal_with(&id, config);
    }

    fn refresh_files(&mut self, config: &Config) {
        self.refresh_cwds(config);
        if !self.explorer_pinned {
            self.explorer.prepare_root(self.focused_cwd());
        }
        self.request_explorer_scan();
        self.relayout(config);
    }

    fn activate_file_row(&mut self, row_index: usize, config: &Config) {
        let Some(row) = self.explorer.rows.get(row_index).cloned() else {
            return;
        };

        if row.is_dir {
            self.explorer.toggle_expanded(&row.path);
            self.request_explorer_scan();
            self.relayout(config);
        } else {
            self.open_file(row.path, config);
        }
    }

    fn open_settings(&mut self, config: &Config) {
        let Some(path) = config_path() else {
            log::warn!("could not resolve the config path");
            return;
        };
        log::info!("opening settings at {}", path.display());
        self.open_file(path, config);
    }

    fn open_file(&mut self, path: PathBuf, config: &Config) {
        self.close_search();
        self.clear_link_hover();
        match Editor::open(&path) {
            Ok(editor) => {
                log::info!("editing {}", path.display());
                self.highlight_revision_seen = editor.revision;
                self.highlight_generation = self.highlight_generation.wrapping_add(1);
                self.highlight_due = Some(Instant::now());
                self.editor_dirty = true;
                self.diff_view = None;
                self.editor = Some(editor);
                self.focus = Focus::Editor;
                self.relayout(config);
            }
            Err(error) => log::warn!("failed to open {}: {error:#}", path.display()),
        }
    }

    fn poll_hook_events(&mut self) {
        for event in self.hook_log.poll() {
            if self.workspace.pane(event.pane).is_some() {
                if event.state == AgentState::Detected {
                    self.hook_states.remove(&event.pane);
                } else {
                    self.hook_states.insert(
                        event.pane,
                        HookState {
                            state: event.state,
                            message: event.message,
                            at: Instant::now(),
                        },
                    );
                }
                self.agents_dirty = true;
            }
        }
    }

    fn poll_agent_watcher(&mut self, config: &Config, force: bool) {
        if !config.agents.enabled {
            return;
        }
        self.poll_hook_events();
        if !force && !self.window_focused && self.agent_snapshot.generated.is_some() {
            return;
        }
        if self.window_focused
            && let Some(pane) = self.workspace.focused_pane()
        {
            self.mark_pane_agents_seen(pane);
        }
        if self.agent_watch_pending {
            if self.last_agent_poll.elapsed() <= Duration::from_secs(8) {
                return;
            }
            log::warn!("agent watcher did not answer in time; restarting it");
            self.agent_watch_pending = false;
            self.agent_watch_tx = agents::spawn_watcher(self.proxy.clone());
        }
        if self.agent_watch_tx.is_none() {
            self.agent_watch_tx = agents::spawn_watcher(self.proxy.clone());
        }

        let now = Instant::now();
        let interval = Duration::from_millis(config.agents.poll_interval_ms);
        if now.duration_since(self.last_agent_poll) < interval {
            return;
        }
        let Some(sender) = &self.agent_watch_tx else {
            return;
        };

        let roots = self
            .workspace
            .tabs
            .iter()
            .flat_map(|tab| tab.panes.values())
            .filter_map(|pane| pane.session.process_id().map(|pid| (pane.id, pid)))
            .collect::<Vec<_>>();

        self.agent_watch_generation = self.agent_watch_generation.wrapping_add(1);
        let request = AgentWatchRequest {
            generation: self.agent_watch_generation,
            roots,
            include_external: config.agents.show_external,
        };
        match sender.send(request) {
            Ok(()) => {
                self.last_agent_poll = now;
                self.agent_watch_pending = true;
            }
            Err(error) => {
                log::warn!("agent watcher stopped: {error}");
                self.agent_watch_tx = None;
            }
        }
    }

    /// Ignore the scan interval and ask for a fresh process tree right now.
    /// Any request still in flight is abandoned; the generation check drops
    /// its stale response.
    fn force_agent_refresh(&mut self, config: &Config) {
        if !config.agents.enabled {
            return;
        }
        self.poll_hook_events();
        self.agent_watch_pending = false;
        self.last_agent_poll = Instant::now()
            .checked_sub(Duration::from_millis(config.agents.poll_interval_ms))
            .unwrap_or_else(Instant::now);
        // Show the current state immediately even while the new scan runs.
        self.agents_dirty = true;
        self.rebuild_agent_snapshot();
        self.poll_agent_watcher(config, true);
    }

    fn on_agents_detected(
        &mut self,
        generation: u64,
        panes: Vec<DetectedAgent>,
        external: Vec<DetectedAgent>,
    ) {
        if generation != self.agent_watch_generation {
            return;
        }
        self.agent_watch_pending = false;

        let previous: HashSet<PaneId> = self
            .detected_agents
            .iter()
            .filter_map(|agent| agent.pane)
            .collect();
        let current: HashSet<PaneId> = panes.iter().filter_map(|agent| agent.pane).collect();
        self.detected_agents = panes;
        self.external_agents = external;

        // When an agent exits back to the shell, drop its transient state.
        for pane in previous.difference(&current) {
            if let Some(pane) = self.workspace.pane_mut(*pane) {
                pane.terminal.clear_active_program_status();
                pane.program_status_revision = pane.terminal.program_status_revision();
            }
            if self
                .hook_states
                .get(pane)
                .is_some_and(|state| !state.state.is_finished())
            {
                self.hook_states.remove(pane);
            }
        }

        self.agents_dirty = true;
        self.rebuild_agent_snapshot();
        log::debug!(
            "agent watcher: {} pane agent(s), {} external",
            self.detected_agents.len(),
            self.external_agents.len()
        );
    }

    fn mark_pane_agents_seen(&mut self, pane: PaneId) {
        let mut changed = false;
        for row in &self.agent_snapshot.rows {
            if row.pane == Some(pane) && row.state.is_finished() {
                changed |= self.seen_agents.insert(row.id.clone());
            }
        }
        if changed {
            self.agents_dirty = true;
        }
    }

    fn focus_agent_pane(&mut self, pane: PaneId, config: &Config) -> bool {
        self.close_search();
        self.clear_link_hover();
        if !self.workspace.focus_pane(pane) {
            return false;
        }
        self.focus = Focus::Terminal;
        self.mark_pane_agents_seen(pane);
        self.rebuild_agent_snapshot();
        self.sync_explorer_to_focused();
        self.update_window_title();
        self.relayout(config);
        self.window.request_redraw();
        true
    }

    fn send_agent_input(&mut self, pane_id: PaneId, bytes: &[u8]) -> bool {
        let Some(pane) = self.workspace.pane_mut(pane_id) else {
            return false;
        };
        pane.session.write(bytes);
        self.agents_dirty = true;
        self.agent_preview_dirty = true;
        true
    }

    fn launch_agent(
        &mut self,
        kind: &AgentKind,
        cwd: Option<PathBuf>,
        config: &Config,
    ) -> Option<PaneId> {
        let command = kind.command_name()?.to_owned();
        self.refresh_cwds(config);
        let cwd = cwd
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| self.focused_cwd());
        let profile_id = self.shells.default_id.clone();
        let (tab_id, pane_id) = self.workspace.create_terminal();
        let session_key = self.alloc_session_key();
        self.pending_spawns.insert(
            pane_id,
            PendingSpawn {
                cwd,
                profile_id,
                session_key,
                scrollback: None,
                initial_command: Some(command),
            },
        );
        self.sidebar.profile_menu = false;
        self.sidebar.hovered_profile = None;
        self.focus = Focus::Terminal;
        self.branch_menu = None;
        self.sidebar.scroll = f32::MAX;

        if let Err(error) = self.sync_layout(config) {
            log::error!("failed to launch agent terminal: {error:#}");
            self.pending_spawns.remove(&pane_id);
            self.workspace.close_tab(tab_id);
            let _ = self.sync_layout(config);
            return None;
        }
        self.save_session(config, false);
        self.update_window_title();
        self.window.request_redraw();
        Some(pane_id)
    }

    fn mark_agent_seen(&mut self, id: &AgentId) {
        if self.seen_agents.insert(id.clone()) {
            self.agents_dirty = true;
            self.rebuild_agent_snapshot();
        }
    }

    fn mark_all_agents_seen(&mut self) -> usize {
        let mut count = 0usize;
        for row in &self.agent_snapshot.rows {
            if row.state.is_finished() && self.seen_agents.insert(row.id.clone()) {
                count += 1;
            }
        }
        if count > 0 {
            self.agents_dirty = true;
            self.rebuild_agent_snapshot();
        }
        count
    }

    fn pane_has_agent(&self, pane: PaneId) -> bool {
        self.detected_agents
            .iter()
            .any(|agent| agent.pane == Some(pane))
            || self
                .workspace
                .pane(pane)
                .is_some_and(|pane| pane.terminal.has_program_status())
    }

    fn pane_agent_state(&self, pane: PaneId) -> Option<AgentState> {
        self.agent_snapshot
            .rows
            .iter()
            .filter(|row| row.pane == Some(pane))
            .filter(|row| !(row.seen && row.state.is_finished()))
            .max_by_key(|row| row.state.priority(row.seen))
            .map(|row| row.state.clone())
    }

    fn tab_agent_state(&self, tab: TabId) -> Option<AgentState> {
        let panes = self
            .workspace
            .tabs
            .iter()
            .find(|candidate| candidate.id == tab)?
            .panes
            .values()
            .map(|pane| pane.id)
            .collect::<Vec<_>>();
        panes
            .into_iter()
            .filter_map(|pane| self.pane_agent_state(pane))
            .max_by_key(|state| state.priority(false))
    }

    fn rebuild_agent_snapshot(&mut self) {
        if !self.agents_dirty && self.agent_snapshot.generated.is_some() {
            return;
        }
        let now = Instant::now();
        let now_epoch = epoch_seconds();
        let mut rows = Vec::new();
        let active_panes: HashSet<PaneId> = self
            .workspace
            .tabs
            .iter()
            .flat_map(|tab| tab.panes.values())
            .map(|pane| pane.id)
            .collect();
        self.hook_states
            .retain(|pane, _| active_panes.contains(pane));
        self.agent_notifications
            .retain(|pane, _| active_panes.contains(pane));
        let detected_by_pane: HashMap<PaneId, Vec<DetectedAgent>> = {
            let mut map: HashMap<PaneId, Vec<DetectedAgent>> = HashMap::new();
            for agent in &self.detected_agents {
                if let Some(pane) = agent.pane {
                    map.entry(pane).or_default().push(agent.clone());
                }
            }
            map
        };

        for tab in &self.workspace.tabs {
            for pane in tab.panes.values() {
                let records = pane.terminal.program_status();
                let detected = detected_by_pane.get(&pane.id).cloned().unwrap_or_default();
                let raw_root = records.iter().find(|record| record.id.is_empty());
                let root_kind = raw_root
                    .and_then(|record| record.app.as_deref())
                    .map(kind_from_protocol_app);
                let stale_protocol = root_kind.as_ref().is_some_and(|kind| {
                    !detected.is_empty() && detected.iter().all(|agent| &agent.kind != kind)
                });
                let root = raw_root.filter(|_| !stale_protocol);
                let children: Vec<&ProgramRecord> = records
                    .iter()
                    .filter(|record| !record.id.is_empty())
                    .collect();
                let mut used_detected: HashSet<u32> = HashSet::new();
                let notification = self
                    .agent_notifications
                    .get(&pane.id)
                    .filter(|notification| {
                        now.duration_since(notification.at) < Duration::from_secs(120)
                            && pane
                                .last_output
                                .checked_duration_since(notification.at)
                                .is_none_or(|elapsed| elapsed <= Duration::from_secs(1))
                    });
                let hook = self
                    .hook_states
                    .get(&pane.id)
                    .filter(|hook| now.duration_since(hook.at) < Duration::from_secs(1800));
                let has_agent_in_pane = root.is_some() || hook.is_some() || !detected.is_empty();
                let preview_lines = if has_agent_in_pane {
                    pane.terminal.tail_lines(60)
                } else {
                    Vec::new()
                };
                let git_branch = pane.git_status.as_ref().map(|status| status.branch.clone());
                let git_diff = pane
                    .git_status
                    .as_ref()
                    .map(|status| (status.additions, status.deletions));

                if let Some(root) = root {
                    let kind = root
                        .app
                        .as_deref()
                        .map(kind_from_protocol_app)
                        .or_else(|| detected.first().map(|agent| agent.kind.clone()))
                        .unwrap_or_else(|| AgentKind::Other("agent".into()));
                    let row_id = AgentId::Terminal {
                        pane: pane.id,
                        record: root.id.clone(),
                    };
                    let detected_match = detected
                        .iter()
                        .find(|agent| agent.kind == kind)
                        .or_else(|| detected.first());
                    let (pid, started, cwd, memory_bytes, cpu_tenths, child_processes, command) =
                        if let Some(agent) = detected_match {
                            used_detected.insert(agent.pid);
                            (
                                Some(agent.pid),
                                if agent.started > 0 {
                                    agent.started
                                } else {
                                    self.agent_times.get(&row_id).copied().unwrap_or(0)
                                },
                                agent.cwd.clone().or_else(|| pane.cwd.clone()),
                                Some(agent.memory_bytes),
                                Some(agent.cpu_tenths),
                                agent.child_processes,
                                agent.command.clone(),
                            )
                        } else {
                            (
                                None,
                                self.agent_times.get(&row_id).copied().unwrap_or(0),
                                pane.cwd.clone(),
                                None,
                                None,
                                0,
                                None,
                            )
                        };

                    let mut state = protocol_agent_state(
                        root.state,
                        root.kind,
                        notification,
                        pane.last_output.elapsed(),
                    );
                    let mut message = root
                        .message
                        .clone()
                        .or_else(|| notification.and_then(notification_message));
                    if matches!(state, AgentState::Working | AgentState::Attention)
                        && message.is_none()
                    {
                        message = notification.and_then(notification_message);
                    }
                    if matches!(state, AgentState::Idle) && notification.is_some() {
                        // Codex reports idle after a completed turn and sends
                        // its completion text through OSC 9.
                        if notification
                            .and_then(notification_message)
                            .as_deref()
                            .is_some_and(looks_finished)
                        {
                            state = AgentState::Done;
                        }
                    }

                    let seen = state.is_finished() && self.seen_agents.contains(&row_id);
                    let mut subagents = Vec::new();
                    for child in children {
                        if child.id == root.id {
                            continue;
                        }
                        let child_kind = child
                            .app
                            .as_deref()
                            .map(kind_from_protocol_app)
                            .unwrap_or_else(|| kind.clone());
                        subagents.push(AgentChild {
                            id: child.id.clone(),
                            kind: child_kind,
                            state: protocol_agent_state(
                                child.state,
                                child.kind,
                                None,
                                pane.last_output.elapsed(),
                            ),
                            message: child.message.clone(),
                            progress: child.progress,
                        });
                    }
                    rows.push(AgentRow {
                        id: row_id,
                        kind,
                        state,
                        message,
                        title: root.title.clone().or_else(|| pane.title.clone()),
                        cwd,
                        pane: Some(pane.id),
                        tab_title: Some(tab.title.clone()),
                        pane_label: pane_label(pane, &self.shells),
                        pid,
                        started,
                        progress: root.progress,
                        source: AgentSource::Protocol,
                        subagents,
                        seen,
                        memory_bytes,
                        cpu_tenths,
                        child_processes,
                        command,
                        git_branch: git_branch.clone(),
                        git_diff,
                        last_output: Some(pane.last_output),
                        preview_lines: preview_lines.clone(),
                    });
                }

                if root.is_none()
                    && let Some(hook) = hook
                {
                    let detected_match = detected.first();
                    if let Some(agent) = detected_match {
                        used_detected.insert(agent.pid);
                    }
                    let kind = detected_match
                        .map(|agent| agent.kind.clone())
                        .unwrap_or(AgentKind::Junie);
                    let row_id = AgentId::Terminal {
                        pane: pane.id,
                        record: "hook".into(),
                    };
                    let started = detected_match
                        .map(|agent| agent.started)
                        .filter(|started| *started > 0)
                        .or_else(|| self.agent_times.get(&row_id).copied())
                        .unwrap_or(0);
                    rows.push(AgentRow {
                        id: row_id.clone(),
                        kind,
                        state: hook.state.clone(),
                        message: hook.message.clone(),
                        title: pane.title.clone(),
                        cwd: detected_match
                            .and_then(|agent| agent.cwd.clone())
                            .or_else(|| pane.cwd.clone()),
                        pane: Some(pane.id),
                        tab_title: Some(tab.title.clone()),
                        pane_label: pane_label(pane, &self.shells),
                        pid: detected_match.map(|agent| agent.pid),
                        started,
                        progress: None,
                        source: AgentSource::Hook,
                        subagents: Vec::new(),
                        seen: hook.state.is_finished() && self.seen_agents.contains(&row_id),
                        memory_bytes: detected_match.map(|agent| agent.memory_bytes),
                        cpu_tenths: detected_match.map(|agent| agent.cpu_tenths),
                        child_processes: detected_match
                            .map(|agent| agent.child_processes)
                            .unwrap_or(0),
                        command: detected_match.and_then(|agent| agent.command.clone()),
                        git_branch: git_branch.clone(),
                        git_diff,
                        last_output: Some(pane.last_output),
                        preview_lines: preview_lines.clone(),
                    });
                }

                for agent in detected
                    .iter()
                    .filter(|agent| !used_detected.contains(&agent.pid))
                {
                    let id = AgentId::Terminal {
                        pane: pane.id,
                        record: format!("process:{}", agent.pid),
                    };
                    let started = if agent.started > 0 {
                        agent.started
                    } else {
                        self.agent_times.get(&id).copied().unwrap_or(0)
                    };
                    let mut row = process_agent_row(
                        pane,
                        &tab.title,
                        pane_label(pane, &self.shells),
                        agent,
                        notification,
                        started,
                        preview_lines.clone(),
                    );
                    row.seen = row.state.is_finished() && self.seen_agents.contains(&row.id);
                    rows.push(row);
                }
            }
        }

        for agent in &self.external_agents {
            let id = AgentId::External { pid: agent.pid };
            let started = if agent.started > 0 {
                agent.started
            } else {
                self.agent_times.get(&id).copied().unwrap_or(0)
            };
            rows.push(AgentRow {
                id,
                kind: agent.kind.clone(),
                state: AgentState::Detected,
                message: None,
                title: None,
                cwd: agent.cwd.clone(),
                pane: None,
                tab_title: None,
                pane_label: None,
                pid: Some(agent.pid),
                started,
                progress: None,
                source: AgentSource::External,
                subagents: Vec::new(),
                seen: false,
                memory_bytes: Some(agent.memory_bytes),
                cpu_tenths: Some(agent.cpu_tenths),
                child_processes: agent.child_processes,
                command: agent.command.clone(),
                git_branch: None,
                git_diff: None,
                last_output: None,
                preview_lines: Vec::new(),
            });
        }

        let active: HashSet<AgentId> = rows.iter().map(|row| row.id.clone()).collect();
        self.agent_times.retain(|id, _| active.contains(id));
        self.seen_agents
            .retain(|id| rows.iter().any(|row| &row.id == id && row.seen));
        for row in &mut rows {
            if row.started == 0 {
                row.started = now_epoch;
                self.agent_times.insert(row.id.clone(), now_epoch);
            }
        }

        rows.sort_by(|a, b| {
            b.state
                .priority(b.seen)
                .cmp(&a.state.priority(a.seen))
                .then_with(|| a.pane.is_none().cmp(&b.pane.is_none()))
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.pid.cmp(&b.pid))
        });

        self.agent_snapshot = Arc::new(AgentSnapshot {
            rows,
            generated: Some(now),
        });
        self.agents_dirty = false;
        log::debug!(
            "agent snapshot: {} row(s), {} need attention",
            self.agent_snapshot.rows.len(),
            self.agent_snapshot.attention_count()
        );
    }

    fn poll_background(&mut self, config: &Config, agent_window_open: bool) {
        let now = Instant::now();

        // Background polling is only useful while the user can see the
        // terminal or the Agent Mode window. Sleeping here also stops git
        // processes while another application owns the focus.
        if (!self.window_focused && !agent_window_open)
            || self.window.is_minimized().unwrap_or(false)
        {
            return;
        }

        if now.duration_since(self.last_cwd_poll) >= Duration::from_millis(750) {
            self.last_cwd_poll = now;
            self.refresh_cwds(config);
        }

        if now.duration_since(self.last_git_poll) >= GIT_STATUS_POLL_INTERVAL {
            self.last_git_poll = now;
            self.refresh_git_statuses();
        }

        if let Some(editor) = &self.editor
            && editor.revision != self.highlight_revision_seen
        {
            self.highlight_revision_seen = editor.revision;
            self.highlight_generation = self.highlight_generation.wrapping_add(1);
            self.highlight_due = Some(now + Duration::from_millis(120));
        }

        let Some(due) = self.highlight_due else {
            return;
        };
        if now < due || self.highlight_generation <= self.highlight_sent_generation {
            return;
        }

        let Some(editor) = &self.editor else {
            self.highlight_due = None;
            return;
        };
        if !editor.highlight_dirty {
            self.highlight_due = None;
            return;
        }

        let bytes: usize = editor.lines.iter().map(|line| line.len() + 1).sum();
        if bytes > HIGHLIGHT_LIMIT {
            self.highlight_due = None;
            self.highlight_sent_generation = self.highlight_generation;
            if let Some(editor) = &mut self.editor {
                editor.highlight.clear();
                editor.highlight_dirty = false;
            }
            self.editor_dirty = true;
            return;
        }

        let Some(sender) = &self.highlight_tx else {
            self.highlight_due = None;
            if let Some(editor) = &mut self.editor {
                editor.highlight_dirty = false;
            }
            return;
        };

        let request = HighlightRequest {
            generation: self.highlight_generation,
            revision: editor.revision,
            path: editor.path.clone(),
            lines: editor.lines.clone(),
        };

        if sender.send(request).is_ok() {
            self.highlight_sent_generation = self.highlight_generation;
            self.highlight_due = None;
        }
    }

    fn next_wake(&self) -> Duration {
        if !self.window_focused || self.window.is_minimized().unwrap_or(false) {
            return Duration::from_secs(1);
        }

        let now = Instant::now();
        let cwd_wait =
            Duration::from_millis(750).saturating_sub(now.duration_since(self.last_cwd_poll));
        let highlight_wait = self
            .highlight_due
            .map(|due| due.saturating_duration_since(now))
            .unwrap_or(cwd_wait)
            .min(cwd_wait);

        highlight_wait.max(Duration::from_millis(16))
    }

    fn on_highlight(&mut self, generation: u64, revision: u64, spans: Vec<Vec<StyledSpan>>) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        if generation != self.highlight_generation || revision != editor.revision {
            return;
        }

        editor.highlight = spans;
        editor.highlight_dirty = false;
        self.editor_dirty = true;
        log::debug!(
            "applied syntax highlighting for {} lines",
            editor.highlight.len()
        );
        self.window.request_redraw();
    }

    fn close_editor(&mut self, config: &Config) {
        if self.editor.take().is_some() {
            self.highlight_generation = self.highlight_generation.wrapping_add(1);
            self.highlight_due = None;
            self.editor_dirty = false;
            self.focus = Focus::Terminal;
            self.relayout(config);
        }
    }

    fn save_editor(&mut self) {
        if let Some(editor) = &mut self.editor {
            match editor.save() {
                Ok(()) => log::info!("saved {}", editor.path.display()),
                Err(error) => log::error!("failed to save {}: {error:#}", editor.path.display()),
            }
        }
        self.window.request_redraw();
    }

    fn ensure_editor_visible(&mut self) {
        let Some(layout) = self.view.editor else {
            return;
        };
        let metrics = self.renderer.cell_metrics();
        let rows = (layout.text.height / metrics.height).floor().max(1.0) as usize;
        let cols = (layout.text.width / metrics.width).floor().max(1.0) as usize;
        if let Some(editor) = &mut self.editor {
            editor.ensure_visible(rows, cols);
        }
    }

    fn handle_editor_key(&mut self, event: &winit::event::KeyEvent, config: &Config) -> bool {
        let ctrl = self.modifiers.control_key();
        let alt = self.modifiers.alt_key();
        let super_key = self.modifiers.super_key();
        let command = ctrl || super_key;

        if event.state != ElementState::Pressed || self.editor.is_none() {
            return false;
        }

        // AltGr must type composed characters, not act as a shortcut.
        if ctrl && alt && !super_key {
            if let Some(text) = &event.text
                && !text.is_empty()
            {
                if let Some(editor) = &mut self.editor {
                    editor.insert_str(text);
                }
                self.ensure_editor_visible();
                self.window.request_redraw();
            }
            return true;
        }

        let PhysicalKey::Code(code) = event.physical_key else {
            return false;
        };

        if command {
            match code {
                KeyCode::KeyS if self.modifiers.shift_key() => return false,
                KeyCode::KeyS => {
                    self.save_editor();
                    return true;
                }
                KeyCode::KeyV => {
                    self.paste_into_editor();
                    return true;
                }
                KeyCode::KeyW => {
                    self.close_editor(config);
                    return true;
                }
                // No selection/undo yet; keep these away from the terminal.
                KeyCode::KeyC
                | KeyCode::KeyA
                | KeyCode::KeyK
                | KeyCode::Backspace
                | KeyCode::ArrowLeft
                | KeyCode::ArrowRight
                | KeyCode::ArrowUp
                | KeyCode::ArrowDown => return true,
                // Let global shortcuts (new tab, split, sidebar, tabs, quit) run.
                KeyCode::KeyT
                | KeyCode::KeyB
                | KeyCode::KeyD
                | KeyCode::KeyE
                | KeyCode::KeyF
                | KeyCode::KeyO
                | KeyCode::KeyQ
                | KeyCode::BracketLeft
                | KeyCode::BracketRight
                | KeyCode::Digit1
                | KeyCode::Digit2
                | KeyCode::Digit3
                | KeyCode::Digit4
                | KeyCode::Digit5
                | KeyCode::Digit6
                | KeyCode::Digit7
                | KeyCode::Digit8
                | KeyCode::Digit9
                | KeyCode::Digit0
                | KeyCode::Equal
                | KeyCode::Minus => return false,
                _ => return true,
            }
        }

        if alt {
            // Avoid sending Alt-composed text to the shell while editing.
            if let Some(text) = &event.text
                && !text.is_empty()
                && let Some(editor) = &mut self.editor
            {
                editor.insert_str(text);
                self.ensure_editor_visible();
                self.window.request_redraw();
            }
            return true;
        }

        if let Key::Named(named) = &event.logical_key {
            let page_rows = self
                .view
                .editor
                .map(|layout| {
                    let metrics = self.renderer.cell_metrics();
                    (layout.text.height / metrics.height).floor().max(1.0) as usize
                })
                .unwrap_or(10);

            match named {
                NamedKey::Escape => {
                    self.close_editor(config);
                }
                NamedKey::Enter => {
                    if let Some(editor) = &mut self.editor {
                        editor.insert_char('\n');
                    }
                }
                NamedKey::Tab => {
                    if let Some(editor) = &mut self.editor {
                        editor.insert_str("    ");
                    }
                }
                NamedKey::Space => {
                    if let Some(editor) = &mut self.editor {
                        editor.insert_char(' ');
                    }
                }
                NamedKey::Backspace => {
                    if let Some(editor) = &mut self.editor {
                        editor.backspace();
                    }
                }
                NamedKey::Delete => {
                    if let Some(editor) = &mut self.editor {
                        editor.delete();
                    }
                }
                NamedKey::ArrowLeft => {
                    if let Some(editor) = &mut self.editor {
                        editor.move_left();
                    }
                }
                NamedKey::ArrowRight => {
                    if let Some(editor) = &mut self.editor {
                        editor.move_right();
                    }
                }
                NamedKey::ArrowUp => {
                    if let Some(editor) = &mut self.editor {
                        editor.move_up();
                    }
                }
                NamedKey::ArrowDown => {
                    if let Some(editor) = &mut self.editor {
                        editor.move_down();
                    }
                }
                NamedKey::Home => {
                    if let Some(editor) = &mut self.editor {
                        editor.move_home();
                    }
                }
                NamedKey::End => {
                    if let Some(editor) = &mut self.editor {
                        editor.move_end();
                    }
                }
                NamedKey::PageUp => {
                    if let Some(editor) = &mut self.editor {
                        editor.page_up(page_rows);
                    }
                }
                NamedKey::PageDown => {
                    if let Some(editor) = &mut self.editor {
                        editor.page_down(page_rows);
                    }
                }
                _ => {}
            }

            self.ensure_editor_visible();
            self.window.request_redraw();
            return true;
        }

        if let Some(text) = &event.text
            && !text.is_empty()
        {
            if let Some(editor) = &mut self.editor {
                editor.insert_str(text);
            }
            self.ensure_editor_visible();
            self.window.request_redraw();
            return true;
        }

        // Swallow other keys while the editor is focused.
        true
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }

    fn paste_into_editor(&mut self) {
        let text = self
            .clipboard()
            .and_then(|clipboard| clipboard.get_text().ok());
        if let Some(text) = text
            && let Some(editor) = &mut self.editor
        {
            editor.insert_str(&text);
            self.editor_dirty = true;
            self.ensure_editor_visible();
            self.window.request_redraw();
        }
    }

    fn set_editor_cursor_from_point(&mut self, position: PhysicalPosition<f64>) {
        let Some(layout) = self.view.editor else {
            return;
        };
        if !layout.text.contains(position.x as f32, position.y as f32) {
            return;
        }

        let metrics = self.renderer.cell_metrics();
        let column = ((position.x as f32 - layout.text.x) / metrics.width).floor() as usize;
        let row = ((position.y as f32 - layout.text.y) / metrics.height).floor() as usize;

        if let Some(editor) = &mut self.editor {
            let row = editor.scroll_row + row;
            let col = editor.scroll_col + column;
            editor.set_cursor(row, col);
            self.ensure_editor_visible();
            self.window.request_redraw();
        }
    }

    fn clear_focused(&mut self) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.session.write(b"\x0c");
        }
    }

    fn copy(&mut self) {
        let Some(pane) = self.focused_pane() else {
            return;
        };
        let Some(text) = pane.terminal.selection_to_string() else {
            return;
        };
        if text.is_empty() {
            return;
        }

        match self.clipboard() {
            Some(clipboard) => {
                if let Err(error) = clipboard.set_text(text) {
                    log::warn!("clipboard copy failed: {error}");
                }
            }
            None => log::warn!("clipboard is unavailable"),
        }
    }

    fn paste(&mut self) {
        let Some(clipboard) = self.clipboard() else {
            log::warn!("clipboard is unavailable");
            return;
        };

        let text = match clipboard.get_text() {
            Ok(text) => text,
            Err(error) => {
                log::warn!("clipboard paste failed: {error}");
                return;
            }
        };
        if text.is_empty() {
            return;
        }

        let text = text.replace("\x1b[201~", "");
        let Some(pane) = self.focused_pane_mut() else {
            return;
        };

        if pane.terminal.mode().contains(TermMode::BRACKETED_PASTE) {
            let mut bytes = Vec::with_capacity(text.len() + 12);
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
            pane.session.write(&bytes);
        } else {
            pane.session.write(text.as_bytes());
        }
        pane.suggestion_armed = true;
    }

    fn focus_direction(&mut self, direction: FocusDirection) {
        self.close_search();
        self.clear_link_hover();
        let Some(current) = self.workspace.focused_pane() else {
            return;
        };
        let Some(current_rect) = self
            .view
            .panes
            .iter()
            .find(|pane| pane.id == current)
            .map(|pane| pane.outer)
        else {
            return;
        };

        let (cx, cy) = current_rect.center();
        let mut best: Option<(f32, PaneId)> = None;

        for pane in &self.view.panes {
            if pane.id == current {
                continue;
            }
            let (px, py) = pane.outer.center();
            let (dx, dy) = (px - cx, py - cy);

            let (forward, primary, secondary) = match direction {
                FocusDirection::Left => (-dx, -dx, dy.abs()),
                FocusDirection::Right => (dx, dx, dy.abs()),
                FocusDirection::Up => (-dy, -dy, dx.abs()),
                FocusDirection::Down => (dy, dy, dx.abs()),
            };

            if forward <= 1.0 {
                continue;
            }

            let score = primary + secondary * 2.0;
            if best.is_none_or(|(best_score, _)| score < best_score) {
                best = Some((score, pane.id));
            }
        }

        if let Some((_, pane)) = best {
            self.workspace.focus_pane(pane);
            self.update_window_title();
            self.window.request_redraw();
        }
    }

    fn focused_pane(&self) -> Option<&Pane> {
        self.workspace
            .focused_pane()
            .and_then(|pane| self.workspace.pane(pane))
    }

    fn focused_pane_mut(&mut self) -> Option<&mut Pane> {
        self.workspace
            .focused_pane()
            .and_then(|pane| self.workspace.pane_mut(pane))
    }

    // ------------------------------------------------------------------
    // Completion
    // ------------------------------------------------------------------

    fn on_completion_commands(&mut self, commands: Vec<String>, config: &Config) {
        self.completion.set_commands(commands);
        self.refresh_completion(config);
        self.window.request_redraw();
    }

    fn on_completion_history(&mut self, history: History) {
        self.completion.set_history(history);
    }

    fn on_shell_profiles(&mut self, profiles: Vec<TerminalProfile>, config: &Config) {
        let added = self.shells.merge_profiles(profiles);
        if added > 0 {
            log::info!("discovered {added} additional shell profile(s)");
            self.relayout(config);
        }
    }

    fn on_explorer_loaded(
        &mut self,
        root: PathBuf,
        scan_id: u64,
        rows: Vec<FileRow>,
        error: Option<String>,
        config: &Config,
    ) {
        if scan_id != self.explorer_scan_id || self.explorer.root != root {
            return;
        }

        self.explorer.apply_scan(rows, error);
        if self.sidebar.mode == SidebarMode::Files {
            self.relayout(config);
        }
    }

    fn request_explorer_scan(&mut self) {
        self.explorer_scan_id = self.explorer_scan_id.wrapping_add(1);
        let scan_id = self.explorer_scan_id;
        let root = self.explorer.root.clone();
        let expanded = self.explorer.expanded().clone();
        let proxy = self.proxy.clone();
        let spawned = std::thread::Builder::new()
            .name("explorer-scan".into())
            .spawn(move || {
                let (rows, error) = FileExplorer::scan(&root, &expanded);
                let _ = proxy.send_event(UserEvent::ExplorerLoaded {
                    root,
                    scan_id,
                    rows,
                    error,
                });
            });
        if spawned.is_err() {
            // Keep the UI usable if the worker cannot start.
            self.explorer.rebuild();
        }
    }

    fn autosave_session(&mut self, config: &Config) {
        if !config.restore_session || self.session_writer.is_none() {
            return;
        }
        let now = Instant::now();
        if now.duration_since(self.last_session_save) >= Duration::from_secs(2) {
            self.save_session(config, false);
        }
        if self.scrollback_queue.is_empty()
            && now.duration_since(self.last_scrollback_save) >= Duration::from_secs(30)
        {
            self.scrollback_queue = self.collect_scrollback_candidates();
            if self.scrollback_queue.is_empty() {
                self.last_scrollback_save = now;
            }
        }
        if !self.scrollback_queue.is_empty() && self.save_scrollback_step(config) {
            self.last_scrollback_save = Instant::now();
        }
    }

    fn collect_scrollback_candidates(&self) -> Vec<PaneId> {
        let mut candidates = Vec::new();
        for tab in &self.workspace.tabs {
            for pane in tab.panes.values() {
                if pane.terminal.in_alt_screen() {
                    continue;
                }
                if self.saved_scrollback.get(&pane.id) == Some(&pane.content_revision) {
                    continue;
                }
                candidates.push(pane.id);
            }
        }
        candidates
    }

    /// Export at most one queued pane per tick so a full scrollback sweep never
    /// clones every grid on the same frame. The queue is fixed when the sweep
    /// starts, so continuous output cannot keep the sweep alive forever.
    fn save_scrollback_step(&mut self, config: &Config) -> bool {
        if !config.restore_scrollback {
            self.scrollback_queue.clear();
            return true;
        }
        if self.scrollback_queue.is_empty() {
            return true;
        }
        let pane_id = self.scrollback_queue.remove(0);
        let Some(state) = self.session_state() else {
            self.scrollback_queue.clear();
            return true;
        };
        let mut keys = Vec::new();
        for tab in &state.tabs {
            keys.extend(tab.panes.iter().map(|pane| pane.key));
        }
        self.saved_scrollback
            .retain(|pane, _| self.workspace.pane(*pane).is_some());

        let Some(pane) = self.workspace.pane(pane_id) else {
            return self.scrollback_queue.is_empty();
        };
        if pane.terminal.in_alt_screen() {
            return self.scrollback_queue.is_empty();
        }

        let source = ScrollbackSource {
            key: pane.session_key,
            grid: pane.terminal.clone_grid(),
            size: pane.grid,
            theme: config.theme,
        };
        let revision = pane.content_revision;
        if let Some(writer) = &self.session_writer {
            writer.request(session::SaveJob {
                state: state.clone(),
                keys,
                scrollbacks: vec![source],
            });
        }
        self.last_saved_session = Some(state);
        self.last_session_save = Instant::now();
        self.saved_scrollback.insert(pane_id, revision);
        self.scrollback_queue.is_empty()
    }

    fn save_session(&mut self, config: &Config, with_scrollback: bool) {
        if !config.restore_session {
            return;
        }
        let Some(state) = self.session_state() else {
            return;
        };
        let mut keys = Vec::new();
        for tab in &state.tabs {
            keys.extend(tab.panes.iter().map(|pane| pane.key));
        }

        let mut scrollbacks = Vec::new();
        let mut revisions = Vec::new();
        if with_scrollback && config.restore_scrollback {
            self.saved_scrollback
                .retain(|pane, _| self.workspace.pane(*pane).is_some());
            for tab in &self.workspace.tabs {
                for pane in tab.panes.values() {
                    if pane.terminal.in_alt_screen() {
                        continue;
                    }
                    if self.saved_scrollback.get(&pane.id) == Some(&pane.content_revision) {
                        continue;
                    }
                    scrollbacks.push(ScrollbackSource {
                        key: pane.session_key,
                        grid: pane.terminal.clone_grid(),
                        size: pane.grid,
                        theme: config.theme,
                    });
                    revisions.push((pane.id, pane.content_revision));
                }
            }
        }

        let now = Instant::now();
        let changed = self.last_saved_session.as_ref() != Some(&state);
        if !changed && scrollbacks.is_empty() {
            self.last_session_save = now;
            if with_scrollback {
                self.last_scrollback_save = now;
            }
            return;
        }

        if let Some(writer) = &self.session_writer {
            writer.request(session::SaveJob {
                state: state.clone(),
                keys,
                scrollbacks,
            });
        }

        self.last_saved_session = Some(state);
        self.last_session_save = now;
        if with_scrollback {
            self.last_scrollback_save = now;
            for (pane, revision) in revisions {
                self.saved_scrollback.insert(pane, revision);
            }
        }
    }

    fn session_state(&self) -> Option<SessionState> {
        let size = self.window.inner_size();
        let scale = self.window.scale_factor();
        let logical = size.to_logical::<f32>(scale);
        let position = self.window.outer_position().ok();
        let (x, y) = match position {
            Some(position) => (Some(position.x), Some(position.y)),
            None => (None, None),
        };
        let window = SavedWindow {
            width: logical.width,
            height: logical.height,
            x,
            y,
            maximized: self.window.is_maximized(),
        };

        let mut tabs = Vec::with_capacity(self.workspace.tabs.len());
        for tab in &self.workspace.tabs {
            let leaves = tab.leaves();
            let mut indices = HashMap::with_capacity(leaves.len());
            for (index, pane) in leaves.iter().enumerate() {
                indices.insert(*pane, index);
            }
            let layout = saved_layout(&tab.layout, &indices)?;
            let focused = leaves
                .iter()
                .position(|pane| *pane == tab.focused)
                .unwrap_or(0);

            let mut panes = Vec::with_capacity(leaves.len());
            for pane_id in &leaves {
                if let Some(pane) = tab.panes.get(pane_id) {
                    panes.push(SavedPane {
                        key: pane.session_key,
                        profile_id: pane.profile_id.clone(),
                        cwd: pane
                            .cwd
                            .as_ref()
                            .map(|cwd| cwd.to_string_lossy().into_owned()),
                    });
                } else {
                    let spawn = self.pending_spawns.get(pane_id)?;
                    panes.push(SavedPane {
                        key: spawn.session_key,
                        profile_id: spawn.profile_id.clone(),
                        cwd: Some(spawn.cwd.to_string_lossy().into_owned()),
                    });
                }
            }

            tabs.push(SavedTab {
                title: Some(tab.title.clone()),
                focused,
                layout,
                panes,
            });
        }

        let collapsed_tabs = self
            .sidebar
            .collapsed_tabs
            .iter()
            .filter_map(|id| self.workspace.tabs.iter().position(|tab| tab.id == *id))
            .collect();

        Some(SessionState {
            version: session::SESSION_VERSION,
            window: Some(window),
            active_tab: self.workspace.active_index(),
            tabs,
            collapsed_tabs,
        })
    }

    fn apply_config(&mut self, old: &Config, config: &Config) {
        if old.font_family != config.font_family {
            log::warn!("font_family changes require a restart");
        }
        if old.render_mode != config.render_mode {
            log::warn!("render.mode changes require a restart");
        }
        if old.shell != config.shell {
            let extras = self.shells.profiles.clone();
            let mut shells = ShellCatalog::detect(config.shell.as_deref());
            shells.merge_profiles(extras);
            self.shells = shells;
        }
        if old.agents.enabled != config.agents.enabled {
            if config.agents.enabled {
                if self.agent_watch_tx.is_none() {
                    self.agent_watch_tx = agents::spawn_watcher(self.proxy.clone());
                }
            } else {
                self.agent_watch_tx = None;
                self.detected_agents.clear();
                self.external_agents.clear();
                self.agent_notifications.clear();
                self.agent_snapshot = Arc::new(AgentSnapshot::default());
                self.agents_dirty = true;
            }
        }

        self.renderer.refresh_font_metrics(config);
        self.frame_cache.clear();
        self.frame_dominant.clear();
        self.prompt_trims.clear();
        self.editor_frame_cache = None;
        self.diff_frame_cache = None;
        self.clear_link_hover();
        if let Some(search) = &mut self.search {
            search.dirty = true;
        }
        let panes: Vec<PaneId> = self
            .workspace
            .tabs
            .iter()
            .flat_map(|tab| tab.panes.values().map(|pane| pane.id))
            .collect();
        for pane_id in &panes {
            if let Some(pane) = self.workspace.pane_mut(*pane_id) {
                pane.terminal.set_scrollback_limit(config.scrollback_lines);
            }
        }
        self.dirty_panes.extend(panes);
        self.relayout(config);
    }

    fn invalidate_pane_frame(&mut self, pane_id: PaneId) {
        self.frame_cache.remove(&pane_id);
        self.frame_dominant.remove(&pane_id);
        self.prompt_trims.remove(&pane_id);
    }

    fn clear_completion(&mut self) {
        self.completion_view = None;
        self.completion_open = false;
        self.completion_selected = 0;
    }

    fn refresh_completion(&mut self, config: &Config) {
        if !config.completions
            || self.focus != Focus::Terminal
            || self.branch_menu.is_some()
            || self.search.is_some()
        {
            self.clear_completion();
            return;
        }

        let Some(pane_id) = self.workspace.focused_pane() else {
            self.clear_completion();
            return;
        };

        let (armed, is_fish) = {
            let Some(pane) = self.workspace.pane(pane_id) else {
                self.clear_completion();
                return;
            };
            let is_fish = self.shells.get(&pane.profile_id).is_some_and(|profile| {
                profile.short.eq_ignore_ascii_case("fish")
                    || profile.name.eq_ignore_ascii_case("fish")
            });
            (pane.suggestion_armed, is_fish)
        };

        // Most redraws (terminal output, hover, footer updates) do not need
        // to inspect the cursor line at all. Keep the expensive grid scan and
        // completion engine for frames where a suggestion is actually armed.
        if !armed && !self.completion_open {
            self.clear_completion();
            return;
        }
        if self.completion_open
            && self
                .completion_view
                .as_ref()
                .is_some_and(|view| view.pane != pane_id)
        {
            self.clear_completion();
            return;
        }

        let Some((input, command, cwd)) = self.workspace.pane(pane_id).and_then(|pane| {
            let input = pane.terminal.input_line()?;
            let prefix = complete::line::extract_command(&input)?;
            let cwd = pane.cwd.clone().unwrap_or_else(default_directory);
            Some((input, prefix.command, cwd))
        }) else {
            self.clear_completion();
            return;
        };

        if command.trim().is_empty() && !self.completion_open {
            self.clear_completion();
            return;
        }

        let options = complete::Options {
            history: config.completion_history,
            commands: config.completion_commands,
            paths: config.completion_paths,
            git: config.completion_git,
            is_fish,
        };

        // A redraw can be caused by mouse movement, output in another pane or
        // a footer update. If the command, cwd and options are unchanged, the
        // candidate list is still valid and only the cursor position needs to
        // be refreshed.
        if let Some(view) = &mut self.completion_view
            && view.pane == pane_id
            && view.line == command
            && view.cwd == cwd
            && view.options == options
        {
            view.row = input.row;
            view.col = input.col;
            view.right_limit = input.right_limit;
            return;
        }

        let completion = self.completion.completion(&command, &cwd, &options);
        if completion.candidates.is_empty() {
            self.clear_completion();
            return;
        }

        log::debug!(
            "completion: {:?} -> {} candidate(s)",
            command,
            completion.candidates.len()
        );
        self.completion_view = Some(CompletionView {
            pane: pane_id,
            row: input.row,
            col: input.col,
            line: command,
            cwd,
            options,
            candidates: completion.candidates,
            typed_line: completion.typed_line,
            typed_token: completion.typed_token,
            right_limit: input.right_limit,
        });
        self.completion_selected = 0;
    }

    fn ghost_candidate(&self, config: &Config) -> Option<&complete::Candidate> {
        if !config.completions || self.focus != Focus::Terminal || self.completion_open {
            return None;
        }
        let view = self.completion_view.as_ref()?;
        if self.workspace.focused_pane() != Some(view.pane) {
            return None;
        }
        let candidate = view.candidates.first()?;
        let typed = if candidate.kind == complete::Kind::History {
            view.typed_line
        } else {
            view.typed_token
        };
        (typed >= config.completion_min_chars).then_some(candidate)
    }

    fn prompt_shift(&self, pane: PaneId, row: usize) -> usize {
        self.prompt_trims
            .get(&pane)
            .and_then(|trims| trims.iter().find(|trim| trim.row == row))
            .map(|trim| trim.columns)
            .unwrap_or(0)
    }

    fn ghost_text(&self, config: &Config) -> Option<GhostText> {
        let view = self.completion_view.as_ref()?;
        let candidate = self.ghost_candidate(config)?;
        let shift = self.prompt_shift(view.pane, view.row);
        let col = view.col.saturating_sub(shift);
        let right_limit = view.right_limit.saturating_sub(shift);
        let free = right_limit.saturating_sub(col);
        if free < 3 {
            return None;
        }
        let mut text = candidate.append.clone();
        if text.chars().count() > free {
            text = text.chars().take(free).collect();
        }
        Some(GhostText {
            row: view.row,
            col,
            text,
            color: config.theme.sidebar_text_dim,
        })
    }

    fn accept_completion(&mut self, config: &Config, index: usize) -> bool {
        let Some(view) = self.completion_view.as_ref() else {
            return false;
        };
        let pane_id = view.pane;
        let expected_line = view.line.clone();
        if self.workspace.focused_pane() != Some(pane_id) {
            return false;
        }
        let Some(candidate) = view.candidates.get(index).cloned() else {
            return false;
        };
        let Some((command, _row, _col)) = self.workspace.pane(pane_id).and_then(|pane| {
            let input = pane.terminal.input_line()?;
            let prefix = complete::line::extract_command(&input)?;
            Some((prefix.command, input.row, input.col))
        }) else {
            self.clear_completion();
            return false;
        };

        if command != expected_line || candidate.append.is_empty() {
            log::debug!(
                "completion accept rejected: expected={:?} current={:?} append={:?}",
                expected_line,
                command,
                candidate.append
            );
            self.refresh_completion(config);
            return false;
        }

        log::debug!(
            "completion accept: {:?} + {:?}",
            expected_line,
            candidate.append
        );
        let Some(pane) = self.workspace.pane_mut(pane_id) else {
            return false;
        };
        pane.session.write(candidate.append.as_bytes());
        pane.suggestion_armed = true;

        self.completion_view = None;
        self.completion_open = false;
        self.completion_selected = 0;
        self.dirty_panes.insert(pane_id);
        self.window.request_redraw();
        true
    }

    fn toggle_completion(&mut self, config: &Config) {
        if self.completion_open {
            self.completion_open = false;
            self.completion_view = None;
            self.completion_selected = 0;
            self.window.request_redraw();
            return;
        }

        self.completion_open = true;
        self.completion_view = None;
        self.refresh_completion(config);
        if self
            .completion_view
            .as_ref()
            .is_none_or(|view| view.candidates.is_empty())
        {
            self.completion_open = false;
        }
        log::debug!(
            "completion popup: open={} candidates={} top={:?}",
            self.completion_open,
            self.completion_view
                .as_ref()
                .map(|view| view.candidates.len())
                .unwrap_or(0),
            self.completion_view
                .as_ref()
                .map(|view| view
                    .candidates
                    .iter()
                    .take(5)
                    .map(|candidate| candidate.label.as_str())
                    .collect::<Vec<_>>())
                .unwrap_or_default()
        );
        self.window.request_redraw();
    }

    fn handle_completion_key(&mut self, event: &winit::event::KeyEvent, config: &Config) -> bool {
        let ctrl = self.modifiers.control_key();
        let alt = self.modifiers.alt_key();
        let shift = self.modifiers.shift_key();
        let super_key = self.modifiers.super_key();

        if !config.completions {
            return false;
        }

        if ctrl && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Space)) {
            self.toggle_completion(config);
            return true;
        }
        if alt
            && !ctrl
            && !super_key
            && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Slash))
        {
            self.toggle_completion(config);
            return true;
        }
        if ctrl
            && !alt
            && !super_key
            && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Period))
        {
            self.toggle_completion(config);
            return true;
        }

        if self.completion_open {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.completion_open = false;
                    self.completion_view = None;
                    self.completion_selected = 0;
                    self.window.request_redraw();
                    return true;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.completion_selected = self.completion_selected.saturating_sub(1);
                    self.window.request_redraw();
                    return true;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    if let Some(view) = &self.completion_view {
                        self.completion_selected = (self.completion_selected + 1)
                            .min(view.candidates.len().saturating_sub(1));
                    }
                    self.window.request_redraw();
                    return true;
                }
                Key::Named(NamedKey::Tab) => {
                    if !self.accept_completion(config, self.completion_selected) {
                        self.refresh_completion(config);
                    }
                    return true;
                }
                Key::Named(NamedKey::Enter) if !shift && !alt => {
                    if !self.accept_completion(config, self.completion_selected) {
                        self.refresh_completion(config);
                    }
                    return true;
                }
                Key::Named(NamedKey::ArrowRight) if !ctrl && !alt && !shift && !super_key => {
                    if !self.accept_completion(config, self.completion_selected) {
                        self.refresh_completion(config);
                    }
                    return true;
                }
                Key::Named(NamedKey::ArrowLeft)
                | Key::Named(NamedKey::Home)
                | Key::Named(NamedKey::End) => {
                    self.clear_completion();
                    self.window.request_redraw();
                    return false;
                }
                _ => {}
            }
            return false;
        }

        if !ctrl
            && !alt
            && !shift
            && !super_key
            && matches!(event.logical_key, Key::Named(NamedKey::ArrowRight))
            && self.ghost_candidate(config).is_some()
        {
            return self.accept_completion(config, 0);
        }

        false
    }

    fn capture_history(&mut self, pane_id: PaneId, config: &Config) {
        if !config.completions || !config.completion_history {
            return;
        }
        if self.workspace.pane(pane_id).is_some_and(|pane| {
            self.shells.get(&pane.profile_id).is_some_and(|profile| {
                profile.short.eq_ignore_ascii_case("fish")
                    || profile.name.eq_ignore_ascii_case("fish")
            })
        }) {
            return;
        }
        let Some(command) = self.workspace.pane(pane_id).and_then(|pane| {
            let input = pane.terminal.input_line()?;
            let prefix = complete::line::extract_command(&input)?;
            Some(prefix.command.trim().to_owned())
        }) else {
            return;
        };
        if command.is_empty() || command.ends_with('\\') {
            return;
        }
        self.completion.add_history(&command);
    }

    // ------------------------------------------------------------------
    // Keyboard
    // ------------------------------------------------------------------

    fn on_key(
        &mut self,
        event: winit::event::KeyEvent,
        event_loop: &ActiveEventLoop,
        config: &Config,
    ) {
        if event.state != ElementState::Pressed {
            return;
        }

        if self.branch_menu.is_some() {
            self.on_branch_menu_key(&event);
            return;
        }

        if self.sidebar.profile_menu {
            self.on_profile_menu_key(&event, config);
            return;
        }

        if self.search.is_some() && self.search_shortcut_pressed(&event) {
            self.close_search();
            return;
        }

        if self.search.is_some() && self.handle_search_key(&event) {
            return;
        }

        if self.focus == Focus::Diff && self.diff_view.is_some() {
            if self.handle_diff_key(&event, config) {
                return;
            }
            let _ = self.handle_shortcut(&event, event_loop, config);
            return;
        }

        if self.focus == Focus::Editor && self.editor.is_some() {
            if self.handle_editor_key(&event, config) {
                self.editor_dirty = true;
                return;
            }
            // Whitelisted global shortcuts (new tab, split, sidebar, quit...).
            let _ = self.handle_shortcut(&event, event_loop, config);
            return;
        }

        if self.handle_shortcut(&event, event_loop, config) {
            return;
        }

        if self.handle_completion_key(&event, config) {
            return;
        }

        let Some(pane_id) = self.workspace.focused_pane() else {
            return;
        };
        let mode = self
            .workspace
            .pane(pane_id)
            .map(|pane| pane.terminal.mode())
            .unwrap_or_default();

        let enter = matches!(event.logical_key, Key::Named(NamedKey::Enter));
        let escape = matches!(event.logical_key, Key::Named(NamedKey::Escape));
        let ctrl_c = self.modifiers.control_key()
            && !self.modifiers.alt_key()
            && matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyC));
        let ctrl_d = self.modifiers.control_key()
            && !self.modifiers.alt_key()
            && matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyD));
        let disarm = enter || escape || ctrl_c || ctrl_d;
        let submit = enter && !self.modifiers.shift_key() && !self.modifiers.alt_key();

        if submit {
            self.capture_history(pane_id, config);
        }
        if disarm {
            if let Some(pane) = self.workspace.pane_mut(pane_id) {
                pane.suggestion_armed = false;
            }
            self.completion_view = None;
            self.completion_open = false;
            self.completion_selected = 0;
        }

        if let Key::Named(named) = &event.logical_key
            && matches!(named, NamedKey::PageUp | NamedKey::PageDown)
            && !mode.contains(TermMode::ALT_SCREEN)
            && let Some(pane) = self.workspace.pane_mut(pane_id)
        {
            let scroll = if matches!(named, NamedKey::PageUp) {
                Scroll::PageUp
            } else {
                Scroll::PageDown
            };
            pane.terminal.scroll(scroll);
            self.dirty_panes.insert(pane_id);
            self.invalidate_pane_frame(pane_id);
            self.window.request_redraw();
            return;
        }

        if let Some(bytes) = input::encode_key(&event, self.modifiers, mode, config.option_as_meta)
        {
            let mut scrolled_back = false;
            if let Some(pane) = self.workspace.pane_mut(pane_id) {
                if pane.terminal.display_offset() > 0 {
                    pane.terminal.scroll(Scroll::Bottom);
                    scrolled_back = true;
                }
                pane.session.write(&bytes);
                if !disarm {
                    pane.suggestion_armed = true;
                }
            }
            if scrolled_back {
                self.dirty_panes.insert(pane_id);
                self.invalidate_pane_frame(pane_id);
                self.window.request_redraw();
            }
        }
    }

    fn on_branch_menu_key(&mut self, event: &winit::event::KeyEvent) {
        let Some(menu) = self.branch_menu.as_mut() else {
            return;
        };
        let count = menu.branches.len();

        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.branch_menu = None;
            }
            Key::Named(NamedKey::ArrowUp) => {
                menu.selected = menu.selected.saturating_sub(1);
            }
            Key::Named(NamedKey::ArrowDown) => {
                if count > 0 {
                    menu.selected = (menu.selected + 1).min(count - 1);
                }
            }
            Key::Named(NamedKey::Home) => menu.selected = 0,
            Key::Named(NamedKey::End) => menu.selected = count.saturating_sub(1),
            Key::Named(NamedKey::Enter) => {
                self.checkout_selected_branch();
            }
            _ => return,
        }

        self.window.request_redraw();
    }

    fn on_profile_menu_key(&mut self, event: &winit::event::KeyEvent, config: &Config) {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.sidebar.profile_menu = false;
                self.sidebar.hovered_profile = None;
                self.relayout(config);
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.sidebar.profile_selected = self.sidebar.profile_selected.saturating_sub(1);
                self.window.request_redraw();
            }
            Key::Named(NamedKey::ArrowDown) => {
                if !self.shells.profiles.is_empty() {
                    self.sidebar.profile_selected =
                        (self.sidebar.profile_selected + 1).min(self.shells.profiles.len() - 1);
                }
                self.window.request_redraw();
            }
            Key::Named(NamedKey::Home) => {
                self.sidebar.profile_selected = 0;
                self.window.request_redraw();
            }
            Key::Named(NamedKey::End) => {
                self.sidebar.profile_selected = self.shells.profiles.len().saturating_sub(1);
                self.window.request_redraw();
            }
            Key::Named(NamedKey::Enter) => {
                self.select_profile(self.sidebar.profile_selected, config);
            }
            _ => {}
        }
    }

    fn open_search(&mut self) {
        if self.search.is_some() {
            self.close_search();
            return;
        }
        if self.focus != Focus::Terminal {
            return;
        }
        let Some(pane) = self.workspace.focused_pane() else {
            return;
        };
        self.clear_link_hover();
        self.search = Some(SearchState {
            pane,
            query: String::new(),
            matches: Vec::new(),
            current: 0,
            dirty: false,
        });
        self.window.request_redraw();
    }

    fn close_search(&mut self) {
        if self.search.take().is_some() {
            self.window.request_redraw();
        }
    }

    fn search_bar_rect(&self) -> Option<Rect> {
        let search = self.search.as_ref()?;
        let layout = self.view.panes.iter().find(|pane| pane.id == search.pane)?;
        let scale = self.renderer.scale_factor() as f32;
        let bar_width = (420.0 * scale).min((layout.content.width - 16.0 * scale).max(1.0));
        let bar_height = (32.0 * scale).max(22.0);
        Some(Rect::new(
            layout.content.right() - bar_width - 8.0 * scale,
            layout.content.y + 8.0 * scale,
            bar_width,
            bar_height,
        ))
    }

    fn search_bar_contains(&self, x: f32, y: f32) -> bool {
        self.search_bar_rect().is_some_and(|bar| bar.contains(x, y))
    }

    fn refresh_search(&mut self) {
        let Some((pane_id, query)) = self
            .search
            .as_ref()
            .map(|search| (search.pane, search.query.clone()))
        else {
            return;
        };
        let matches = self
            .workspace
            .pane(pane_id)
            .map(|pane| pane.terminal.search_matches(&query, 2000))
            .unwrap_or_default();
        if let Some(search) = &mut self.search {
            search.dirty = false;
            search.current = search.current.min(matches.len().saturating_sub(1));
            search.matches = matches;
        }
        self.scroll_to_search_match();
        self.window.request_redraw();
    }

    fn scroll_to_search_match(&mut self) {
        let Some((pane_id, point)) = self.search.as_ref().and_then(|search| {
            search
                .matches
                .get(search.current)
                .map(|matched| (search.pane, *matched.start()))
        }) else {
            return;
        };
        if let Some(pane) = self.workspace.pane_mut(pane_id) {
            pane.terminal.scroll_to_point(point);
            self.dirty_panes.insert(pane_id);
            self.invalidate_pane_frame(pane_id);
        }
    }

    fn advance_search(&mut self, forward: bool) {
        let Some(len) = self
            .search
            .as_ref()
            .map(|search| search.matches.len())
            .filter(|len| *len > 0)
        else {
            return;
        };
        if let Some(search) = &mut self.search {
            search.current = if forward {
                (search.current + 1) % len
            } else {
                (search.current + len - 1) % len
            };
        }
        self.scroll_to_search_match();
        self.window.request_redraw();
    }

    fn search_push(&mut self, text: &str) {
        let Some(search) = &mut self.search else {
            return;
        };
        if search.query.chars().count() >= 256 {
            return;
        }
        let text: String = text
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if text.is_empty() {
            return;
        }
        search.query.push_str(&text);
        search.current = 0;
        search.dirty = true;
        self.refresh_search();
    }

    fn handle_search_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.search.is_none() {
            return false;
        }
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.close_search();
                true
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some(search) = &mut self.search {
                    search.query.pop();
                    search.current = 0;
                    search.dirty = true;
                }
                self.refresh_search();
                true
            }
            Key::Named(NamedKey::Enter) => {
                self.advance_search(!self.modifiers.shift_key());
                true
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.advance_search(false);
                true
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.advance_search(true);
                true
            }
            _ => {
                if let Some(text) = &event.text
                    && !text.is_empty()
                {
                    self.search_push(text);
                }
                true
            }
        }
    }

    fn search_shortcut_pressed(&self, event: &winit::event::KeyEvent) -> bool {
        let PhysicalKey::Code(code) = event.physical_key else {
            return false;
        };
        if cfg!(target_os = "macos") {
            self.modifiers.super_key()
                && self.modifiers.shift_key()
                && !self.modifiers.control_key()
                && !self.modifiers.alt_key()
                && code == KeyCode::KeyF
        } else {
            self.modifiers.control_key()
                && self.modifiers.shift_key()
                && !self.modifiers.alt_key()
                && !self.modifiers.super_key()
                && code == KeyCode::KeyF
        }
    }

    fn handle_shortcut(
        &mut self,
        event: &winit::event::KeyEvent,
        event_loop: &ActiveEventLoop,
        config: &Config,
    ) -> bool {
        let macos = cfg!(target_os = "macos");
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();
        let alt = self.modifiers.alt_key();
        let super_key = self.modifiers.super_key();

        let PhysicalKey::Code(code) = event.physical_key else {
            return false;
        };

        // Ctrl+Tab switches sidebar terminals on every platform.
        if ctrl && code == KeyCode::Tab {
            self.next_terminal(if shift { -1 } else { 1 }, config);
            return true;
        }

        // Terminal zoom: Ctrl/Cmd plus, minus and zero.
        let zoom_mod = if macos {
            super_key && !ctrl && !alt
        } else {
            ctrl && !alt
        };
        if zoom_mod {
            let size = match code {
                KeyCode::Equal | KeyCode::NumpadAdd => Some(config.font_size + 1.0),
                KeyCode::Minus | KeyCode::NumpadSubtract => Some(config.font_size - 1.0),
                KeyCode::Digit0 | KeyCode::Numpad0 => Some(Config::default().font_size),
                _ => None,
            };
            if let Some(size) = size {
                let _ = self.proxy.send_event(UserEvent::SetFontSize(size));
                return true;
            }
        }

        // Windows/Linux copy and paste shortcuts used by most terminals.
        if !alt && ctrl && !shift && code == KeyCode::Insert {
            self.copy();
            return true;
        }
        if !alt && !ctrl && shift && code == KeyCode::Insert {
            self.paste();
            return true;
        }

        if self.search_shortcut_pressed(event) {
            self.open_search();
            return true;
        }
        // Linux/Windows pane focus: Ctrl+Alt+Arrow.
        if !macos
            && ctrl
            && alt
            && !shift
            && let Some(direction) = arrow_direction(code)
        {
            self.focus_direction(direction);
            return true;
        }

        let app_mod = if macos {
            super_key && !ctrl
        } else {
            ctrl && shift
        };
        if !app_mod {
            return false;
        }

        match code {
            KeyCode::KeyO if shift => {
                self.refresh_cwds(config);
                menu::open_explorer(&self.focused_cwd());
                true
            }
            KeyCode::KeyR if shift => {
                self.refresh_cwds(config);
                let _ = self.proxy.send_event(UserEvent::OpenCodeReview {
                    cwd: self.focused_cwd(),
                });
                true
            }
            KeyCode::KeyM if shift => {
                self.refresh_cwds(config);
                let _ = self.proxy.send_event(UserEvent::OpenGitManager {
                    cwd: self.focused_cwd(),
                });
                true
            }
            KeyCode::KeyS if shift => {
                self.open_settings(config);
                true
            }
            KeyCode::KeyC => {
                self.copy();
                true
            }
            KeyCode::KeyV => {
                self.paste();
                true
            }
            KeyCode::KeyK => {
                self.clear_focused();
                true
            }
            KeyCode::KeyT => {
                self.new_terminal(config);
                true
            }
            KeyCode::KeyN if shift => {
                self.toggle_profile_menu(config);
                true
            }
            KeyCode::KeyW => {
                if macos && shift {
                    if let Some(tab) = self.workspace.active_tab_id() {
                        self.close_tab(tab, event_loop, config);
                    }
                } else {
                    self.close_focused_pane(event_loop, config);
                }
                true
            }
            KeyCode::KeyD => {
                if macos && shift {
                    self.split_focused(SplitDirection::Down, config);
                } else {
                    self.split_focused(SplitDirection::Right, config);
                }
                true
            }
            KeyCode::ArrowDown if !macos && shift => {
                self.split_focused(SplitDirection::Down, config);
                true
            }
            KeyCode::KeyE if shift => {
                self.toggle_sidebar_mode(config);
                true
            }
            KeyCode::KeyB => {
                self.toggle_sidebar(config);
                true
            }
            KeyCode::KeyA if shift => {
                let _ = self.proxy.send_event(UserEvent::OpenAgentMode);
                true
            }
            KeyCode::Backquote => {
                self.workspace.cycle_pane(true);
                self.update_window_title();
                self.window.request_redraw();
                true
            }
            KeyCode::BracketLeft => {
                if !macos || shift {
                    self.next_terminal(-1, config);
                    true
                } else {
                    false
                }
            }
            KeyCode::BracketRight => {
                if !macos || shift {
                    self.next_terminal(1, config);
                    true
                } else {
                    false
                }
            }
            KeyCode::Digit1
            | KeyCode::Digit2
            | KeyCode::Digit3
            | KeyCode::Digit4
            | KeyCode::Digit5
            | KeyCode::Digit6
            | KeyCode::Digit7
            | KeyCode::Digit8
            | KeyCode::Digit9 => {
                if let Some(index) = digit_index(code) {
                    self.select_tab_index(index, config);
                }
                true
            }
            KeyCode::ArrowLeft if macos && alt => {
                self.focus_direction(FocusDirection::Left);
                true
            }
            KeyCode::ArrowRight if macos && alt => {
                self.focus_direction(FocusDirection::Right);
                true
            }
            KeyCode::ArrowUp if macos && alt => {
                self.focus_direction(FocusDirection::Up);
                true
            }
            KeyCode::ArrowDown if macos && alt => {
                self.focus_direction(FocusDirection::Down);
                true
            }
            // Readline/Emacs style navigation.
            KeyCode::ArrowLeft if macos && !alt => {
                self.write_focused(b"\x01");
                true
            }
            KeyCode::ArrowRight if macos && !alt => {
                self.write_focused(b"\x05");
                true
            }
            KeyCode::Backspace if macos && !alt => {
                self.write_focused(b"\x15");
                true
            }
            KeyCode::KeyQ if macos => {
                self.save_session(config, true);
                event_loop.exit();
                true
            }
            _ => false,
        }
    }

    fn write_focused(&mut self, bytes: &[u8]) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.session.write(bytes);
        }
    }

    // ------------------------------------------------------------------
    // Mouse
    // ------------------------------------------------------------------

    fn on_cursor_moved(&mut self, position: PhysicalPosition<f64>, config: &Config) {
        self.cursor_position = position;

        if let Some(mut drag) = self.sidebar_drag {
            let threshold = 4.0 * self.renderer.scale_factor() as f32;
            if (position.y as f32 - drag.start_y).abs() > threshold {
                drag.active = true;
            }
            if drag.active {
                drag.target = self.sidebar_insertion_index(position.y as f32);
                self.window.request_redraw();
            }
            self.sidebar_drag = Some(drag);
            return;
        }

        if let Some(drag) = self.drag {
            match drag {
                DragState::Split(split_id) => {
                    let Some((direction, area, gap)) = self
                        .view
                        .dividers
                        .iter()
                        .find(|divider| divider.split_id == split_id)
                        .map(|divider| (divider.direction, divider.area, divider.line.width))
                    else {
                        self.drag = None;
                        return;
                    };

                    let ratio = match direction {
                        SplitDirection::Right => {
                            let usable = (area.width - gap).max(1.0);
                            ((position.x as f32 - area.x) / usable).clamp(0.1, 0.9)
                        }
                        SplitDirection::Down => {
                            let usable = (area.height - gap).max(1.0);
                            ((position.y as f32 - area.y) / usable).clamp(0.1, 0.9)
                        }
                    };

                    if self.workspace.set_split_ratio(split_id, ratio) {
                        self.relayout(config);
                    }
                }
                DragState::Editor => {
                    if let Some(layout) = self.view.editor {
                        let ratio = ((layout.split_area.right() - position.x as f32)
                            / layout.split_area.width)
                            .clamp(0.2, 0.75);
                        self.editor_ratio = ratio;
                        self.relayout(config);
                    }
                }
            }
            return;
        }

        if let Some(pane_id) = self.selecting {
            if let Some((point, side)) = self.point_for_pane(pane_id, position)
                && let Some(pane) = self.workspace.pane_mut(pane_id)
            {
                pane.terminal.update_selection(point, side);
                self.dirty_panes.insert(pane_id);
                self.invalidate_pane_frame(pane_id);
                self.window.request_redraw();
            }
            return;
        }

        self.update_hovered_link(position);

        let mut hovered_tab = None;
        let mut hovered_pane = None;
        let mut hovered_file = None;
        let mut hovered_profile = None;
        if self.view.sidebar.rect.width > 0.0 {
            match sidebar::hit_test(&self.view.sidebar, position.x as f32, position.y as f32) {
                Some(SidebarHit::Select(tab))
                | Some(SidebarHit::Close(tab))
                | Some(SidebarHit::ToggleWorkspace(tab)) => {
                    hovered_tab = Some(tab);
                }
                Some(SidebarHit::SelectPane(_, pane)) | Some(SidebarHit::ClosePane(pane)) => {
                    hovered_pane = Some(pane);
                }
                Some(SidebarHit::File(row)) => hovered_file = Some(row),
                Some(SidebarHit::Profile(index)) => hovered_profile = Some(index),
                _ => {}
            }
        }

        if hovered_tab != self.sidebar.hovered
            || hovered_pane != self.sidebar.hovered_pane
            || hovered_file != self.sidebar.hovered_file
            || hovered_profile != self.sidebar.hovered_profile
        {
            self.sidebar.hovered = hovered_tab;
            self.sidebar.hovered_pane = hovered_pane;
            self.sidebar.hovered_file = hovered_file;
            self.sidebar.hovered_profile = hovered_profile;
            self.window.request_redraw();
        }

        if self.branch_menu.is_some() {
            self.window.request_redraw();
        }
    }

    fn on_mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
        event_loop: &ActiveEventLoop,
        config: &Config,
    ) {
        let x = self.cursor_position.x as f32;
        let y = self.cursor_position.y as f32;

        if state == ElementState::Released
            && button == MouseButton::Left
            && let Some(drag) = self.sidebar_drag.take()
        {
            if drag.active && self.workspace.move_tab(drag.tab, drag.target) {
                self.save_session(config, false);
                self.relayout(config);
            }
            return;
        }

        if self.branch_menu.is_some() && self.handle_branch_menu_click(state, button, x, y, config)
        {
            return;
        }

        if state == ElementState::Pressed && button == MouseButton::Left {
            if self.completion_open {
                self.clear_completion();
                self.window.request_redraw();
            }

            if self.sidebar.profile_menu && !self.view.sidebar.rect.contains(x, y) {
                self.sidebar.profile_menu = false;
                self.sidebar.hovered_profile = None;
                self.relayout(config);
                return;
            }

            if self.view.sidebar.rect.width > 0.0 {
                if let Some(hit) = sidebar::hit_test(&self.view.sidebar, x, y) {
                    match hit {
                        SidebarHit::ToggleSidebar => self.toggle_sidebar(config),
                        SidebarHit::ToggleMode => self.toggle_sidebar_mode(config),
                        SidebarHit::ToggleProfiles => self.toggle_profile_menu(config),
                        SidebarHit::NewTerminal => self.new_terminal(config),
                        SidebarHit::RefreshFiles => self.refresh_files(config),
                        SidebarHit::ExplorerUp => self.explorer_go_up(config),
                        SidebarHit::ToggleWorkspace(tab) => {
                            self.toggle_workspace(tab, config);
                        }
                        SidebarHit::Select(tab) => {
                            self.sidebar_drag = Some(SidebarDrag {
                                tab,
                                start_y: y,
                                active: false,
                                target: self
                                    .workspace
                                    .tabs
                                    .iter()
                                    .position(|item| item.id == tab)
                                    .unwrap_or(0),
                            });
                            self.select_tab(tab, config);
                        }
                        SidebarHit::SelectPane(tab, pane) => {
                            self.select_pane(tab, pane, config);
                        }
                        SidebarHit::Close(tab) => self.close_tab(tab, event_loop, config),
                        SidebarHit::ClosePane(pane) => {
                            self.close_pane(pane, event_loop, config);
                        }
                        SidebarHit::File(row) => self.activate_file_row(row, config),
                        SidebarHit::Profile(index) => self.select_profile(index, config),
                    }
                    return;
                }
                if self.view.sidebar.rect.contains(x, y) {
                    return;
                }
            }

            if self.editor_divider_at(x, y) {
                self.drag = Some(DragState::Editor);
                return;
            }

            if let Some(split_id) = self.divider_at(x, y) {
                self.drag = Some(DragState::Split(split_id));
                return;
            }
        }

        if self.drag.is_some() {
            if state == ElementState::Released {
                self.drag = None;
                self.window.request_redraw();
            }
            return;
        }

        if let Some(panel) = self.view.editor
            && panel.panel.contains(x, y)
        {
            self.close_search();
            self.clear_link_hover();
            if state == ElementState::Pressed && button == MouseButton::Left {
                if panel.close.contains(x, y) {
                    if self.diff_view.is_some() {
                        self.close_diff_view(config);
                    } else {
                        self.close_editor(config);
                    }
                } else if self.diff_view.is_some() {
                    self.focus = Focus::Diff;
                } else {
                    self.focus = Focus::Editor;
                    self.set_editor_cursor_from_point(self.cursor_position);
                }
                self.window.request_redraw();
            }
            return;
        }

        if self.search.is_some() && state == ElementState::Pressed && button == MouseButton::Left {
            if self.search_bar_contains(x, y) {
                return;
            }
            self.close_search();
        }

        let Some(pane_id) = self.pane_at(x, y) else {
            return;
        };

        let link_modifier = if cfg!(target_os = "macos") {
            self.modifiers.super_key() && !self.modifiers.control_key()
        } else {
            self.modifiers.control_key() && !self.modifiers.super_key()
        };

        if state == ElementState::Pressed
            && button == MouseButton::Left
            && link_modifier
            && self.search.is_none()
            && let Some((point, _)) = self.point_for_pane(pane_id, self.cursor_position)
            && let Some((uri, _)) = self
                .workspace
                .pane(pane_id)
                .and_then(|pane| pane.terminal.link_at(point))
        {
            self.focus = Focus::Terminal;
            self.workspace.focus_pane(pane_id);
            self.sync_explorer_to_focused();
            self.update_window_title();
            self.window.request_redraw();
            open_url(&uri);
            return;
        }

        if state == ElementState::Pressed {
            self.focus = Focus::Terminal;
            self.workspace.focus_pane(pane_id);
            self.sync_explorer_to_focused();
            self.update_window_title();
            self.window.request_redraw();
        }

        if let Some(hits) = self.footer_hits.get(&pane_id).copied() {
            if hits.branch.is_some_and(|rect| rect.contains(x, y)) {
                if state == ElementState::Pressed && button == MouseButton::Left {
                    self.toggle_branch_menu(pane_id);
                }
                return;
            }
            if hits.changes.is_some_and(|rect| rect.contains(x, y)) {
                if state == ElementState::Pressed && button == MouseButton::Left {
                    self.toggle_diff_view(pane_id, config);
                }
                return;
            }
        }

        if self
            .view
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .is_some_and(|pane| pane_footer_band(pane, self.renderer.cell_metrics()).contains(x, y))
        {
            return;
        }

        let mouse_mode = self
            .workspace
            .pane(pane_id)
            .map(|pane| pane.terminal.mode())
            .unwrap_or_default();

        if mouse_mode.intersects(TermMode::MOUSE_MODE) && !self.modifiers.shift_key() {
            if mouse_mode.contains(TermMode::SGR_MOUSE) {
                self.send_mouse(pane_id, button, state == ElementState::Pressed);
            }
            return;
        }

        if button != MouseButton::Left {
            return;
        }

        match state {
            ElementState::Pressed => {
                let Some((point, side)) = self.point_for_pane(pane_id, self.cursor_position) else {
                    return;
                };

                let now = Instant::now();
                let double_click = self
                    .last_click
                    .is_some_and(|(time, last_point, last_pane)| {
                        last_pane == pane_id
                            && now.duration_since(time) < Duration::from_millis(400)
                            && last_point == point
                    });

                if let Some(pane) = self.workspace.pane_mut(pane_id) {
                    if double_click {
                        pane.terminal.start_word_selection(point, side);
                    } else {
                        pane.terminal.start_selection(point, side);
                    }
                }

                self.last_click = Some((now, point, pane_id));
                self.selecting = Some(pane_id);
                self.clear_link_hover();
                self.dirty_panes.insert(pane_id);
                self.invalidate_pane_frame(pane_id);
                self.window.request_redraw();
            }
            ElementState::Released => {
                let Some(pane_id) = self.selecting.take() else {
                    return;
                };

                let empty = self
                    .workspace
                    .pane(pane_id)
                    .and_then(|pane| pane.terminal.selection_to_string())
                    .is_none();

                if empty && let Some(pane) = self.workspace.pane_mut(pane_id) {
                    pane.terminal.clear_selection();
                    self.dirty_panes.insert(pane_id);
                    self.invalidate_pane_frame(pane_id);
                }
                self.window.request_redraw();
            }
        }
    }

    fn on_mouse_wheel(&mut self, delta: MouseScrollDelta, config: &Config) {
        let x = self.cursor_position.x as f32;
        let y = self.cursor_position.y as f32;

        if self.branch_menu.is_some() && self.handle_branch_menu_wheel(delta, config) {
            return;
        }

        if self.view.sidebar.rect.width > 0.0 && self.view.sidebar.rect.contains(x, y) {
            let item_height = self.view.sidebar.item_height.max(1.0);
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(position) => position.y as f32 / item_height,
            };
            if lines != 0.0 {
                self.sidebar.scroll = (self.sidebar.scroll - lines * item_height)
                    .clamp(0.0, self.view.sidebar.max_scroll);
                self.relayout(config);
            }
            return;
        }

        if let Some(layout) = self.view.editor
            && layout.panel.contains(x, y)
        {
            let metrics = self.renderer.cell_metrics();
            let rows = match delta {
                MouseScrollDelta::LineDelta(_, y) => y * 3.0,
                MouseScrollDelta::PixelDelta(position) => position.y as f32 / metrics.height,
            };
            if self.diff_view.is_some() {
                if self.modifiers.shift_key() {
                    self.scroll_diff(0.0, rows);
                } else {
                    self.scroll_diff(rows, 0.0);
                }
                return;
            }
            let visible_rows = (layout.text.height / metrics.height).floor().max(1.0) as usize;
            if let Some(editor) = &mut self.editor {
                let max_scroll = editor.lines.len().saturating_sub(visible_rows) as i32;
                editor.scroll_row =
                    (editor.scroll_row as i32 - rows.round() as i32).clamp(0, max_scroll) as usize;
            }
            self.window.request_redraw();
            self.editor_dirty = true;
            return;
        }

        let Some(pane_id) = self.pane_at(x, y) else {
            return;
        };

        if self
            .view
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .is_some_and(|pane| pane_footer_band(pane, self.renderer.cell_metrics()).contains(x, y))
        {
            return;
        }

        let metrics = self.renderer.cell_metrics();
        let (rows, wheel_notches) = match delta {
            MouseScrollDelta::LineDelta(_, y) => (y * 3.0, y),
            MouseScrollDelta::PixelDelta(position) => {
                let rows = position.y as f32 / metrics.height;
                (rows, rows)
            }
        };
        if rows == 0.0 {
            return;
        }

        let mode = self
            .workspace
            .pane(pane_id)
            .map(|pane| pane.terminal.mode())
            .unwrap_or_default();

        self.clear_link_hover();

        if mode.intersects(TermMode::MOUSE_MODE)
            && mode.contains(TermMode::SGR_MOUSE)
            && !self.modifiers.shift_key()
        {
            if let Some((column, row)) = self.cell_in_pane(pane_id, x, y)
                && let Some(pane) = self.workspace.pane_mut(pane_id)
            {
                let bytes =
                    input::encode_wheel_sgr(wheel_notches > 0.0, column, row, self.modifiers);
                pane.session.write(&bytes);
            }
            return;
        }

        if mode.contains(TermMode::ALT_SCREEN) {
            if mode.contains(TermMode::ALTERNATE_SCROLL) {
                let sequence: &[u8] = if wheel_notches > 0.0 {
                    b"\x1b[A"
                } else {
                    b"\x1b[B"
                };
                let count = (wheel_notches.abs().ceil() as usize).min(5);
                if let Some(pane) = self.workspace.pane_mut(pane_id) {
                    for _ in 0..count {
                        pane.session.write(sequence);
                    }
                }
            }
            return;
        }

        let accumulator = self.scroll_accumulators.entry(pane_id).or_insert(0.0);
        *accumulator += rows;
        let whole_rows = accumulator.trunc();
        if whole_rows != 0.0 {
            *accumulator -= whole_rows;
            if let Some(pane) = self.workspace.pane_mut(pane_id) {
                pane.terminal.scroll(Scroll::Delta(whole_rows as i32));
                self.dirty_panes.insert(pane_id);
                self.invalidate_pane_frame(pane_id);
                self.window.request_redraw();
            }
        }
    }

    fn send_mouse(&mut self, pane_id: PaneId, button: MouseButton, pressed: bool) {
        let Some((column, row)) = self.cell_in_pane(
            pane_id,
            self.cursor_position.x as f32,
            self.cursor_position.y as f32,
        ) else {
            return;
        };
        let modifier_state = self.modifiers;
        if let Some(pane) = self.workspace.pane_mut(pane_id) {
            let bytes = input::encode_mouse_sgr(button, pressed, column, row, modifier_state);
            pane.session.write(&bytes);
        }
    }

    fn divider_at(&self, x: f32, y: f32) -> Option<u64> {
        let threshold = (4.0 * self.renderer.scale_factor() as f32).max(2.0);

        self.view
            .dividers
            .iter()
            .find(|divider| match divider.direction {
                SplitDirection::Right => {
                    x >= divider.line.x - threshold
                        && x <= divider.line.right() + threshold
                        && y >= divider.area.y
                        && y <= divider.area.bottom()
                }
                SplitDirection::Down => {
                    y >= divider.line.y - threshold
                        && y <= divider.line.bottom() + threshold
                        && x >= divider.area.x
                        && x <= divider.area.right()
                }
            })
            .map(|divider| divider.split_id)
    }

    fn editor_divider_at(&self, x: f32, y: f32) -> bool {
        let Some(layout) = self.view.editor else {
            return false;
        };
        let threshold = (4.0 * self.renderer.scale_factor() as f32).max(2.0);
        let divider = layout.divider;

        x >= divider.x - threshold
            && x <= divider.right() + threshold
            && y >= divider.y
            && y <= divider.bottom()
    }

    fn pane_at(&self, x: f32, y: f32) -> Option<PaneId> {
        self.view
            .panes
            .iter()
            .find(|pane| pane.outer.contains(x, y))
            .map(|pane| pane.id)
    }

    fn sidebar_insertion_index(&self, y: f32) -> usize {
        let mut target = 0;
        for row in &self.view.sidebar.rows {
            if let SidebarRow::Workspace(item) = row
                && y > item.rect.y + item.rect.height * 0.5
            {
                target += 1;
            }
        }
        target
    }

    fn update_hovered_link(&mut self, position: PhysicalPosition<f64>) {
        let mut hovered = None;
        let x = position.x as f32;
        let y = position.y as f32;
        if self.search.is_none()
            && !self.search_bar_contains(x, y)
            && let Some(pane_id) = self.pane_at(x, y)
            && let Some((point, _)) = self.point_for_pane(pane_id, position)
            && let Some(pane) = self.workspace.pane(pane_id)
            && let Some((uri, range)) = pane.terminal.link_at(point)
        {
            hovered = Some(LinkHover {
                pane: pane_id,
                uri,
                range,
            });
        }

        if self.hovered_link != hovered {
            let has_link = hovered.is_some();
            self.hovered_link = hovered;
            self.window.set_cursor(if has_link {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });
            self.window.request_redraw();
        }
    }

    fn clear_link_hover(&mut self) {
        if self.hovered_link.take().is_some() {
            self.window.set_cursor(CursorIcon::Default);
            self.window.request_redraw();
        }
    }

    fn point_for_pane(
        &self,
        pane_id: PaneId,
        position: PhysicalPosition<f64>,
    ) -> Option<(Point, Side)> {
        let pane_layout = self.view.panes.iter().find(|pane| pane.id == pane_id)?;
        let pane = self.workspace.pane(pane_id)?;
        let metrics = self.renderer.cell_metrics();
        let x = position.x as f32 - pane_layout.content.x;
        let y = position.y as f32 - pane_layout.content.y;

        if x < 0.0 || y < 0.0 {
            return None;
        }

        let column = (x / metrics.width).floor() as usize;
        let row = (y / metrics.height).floor() as usize;
        if column >= pane.grid.cols || row >= pane.grid.lines {
            return None;
        }

        let shift = self.prompt_shift(pane_id, row);
        let actual_column = (column + shift).min(pane.grid.cols - 1);
        let offset = x - column as f32 * metrics.width;
        let side = if offset < metrics.width / 2.0 {
            Side::Left
        } else {
            Side::Right
        };
        let line = row as i32 - pane.terminal.display_offset() as i32;

        Some((Point::new(Line(line), Column(actual_column)), side))
    }

    fn cell_in_pane(&self, pane_id: PaneId, x: f32, y: f32) -> Option<(usize, usize)> {
        let pane_layout = self.view.panes.iter().find(|pane| pane.id == pane_id)?;
        let pane = self.workspace.pane(pane_id)?;
        let metrics = self.renderer.cell_metrics();
        let local_x = x - pane_layout.content.x;
        let local_y = y - pane_layout.content.y;

        if local_x < 0.0 || local_y < 0.0 {
            return None;
        }

        let column = (local_x / metrics.width).floor() as usize;
        let row = (local_y / metrics.height).floor() as usize;
        if column >= pane.grid.cols || row >= pane.grid.lines {
            return None;
        }

        Some((
            (column + self.prompt_shift(pane_id, row)).min(pane.grid.cols - 1),
            row,
        ))
    }

    // ------------------------------------------------------------------
    // PTY / terminal events
    // ------------------------------------------------------------------

    fn on_pty_data(&mut self, pane_id: PaneId, bytes: &[u8]) -> bool {
        let detected_present = self
            .detected_agents
            .iter()
            .any(|agent| agent.pane == Some(pane_id));
        let Some(pane) = self.workspace.pane_mut(pane_id) else {
            return false;
        };
        pane.terminal.feed(bytes);
        pane.content_revision = pane.content_revision.wrapping_add(1);
        pane.last_output = Instant::now();
        if let Some(search) = &mut self.search
            && search.pane == pane_id
        {
            search.dirty = true;
        }

        let revision = pane.terminal.program_status_revision();
        let status_changed = revision != pane.program_status_revision;
        pane.program_status_revision = revision;

        for reply in pane.terminal.take_program_status_replies() {
            pane.session.write(reply.as_bytes());
        }
        let notifications = pane.terminal.take_program_notifications();
        let agent_present = detected_present || pane.terminal.has_program_status();
        let mut cwd_changed = false;
        if let Some(cwd) = pane.terminal.take_cwd() {
            pane.cwd_from_shell = true;
            if pane.cwd.as_ref() != Some(&cwd) {
                log::debug!("pane {} shell cwd: {}", pane_id.0, cwd.display());
                pane.cwd = Some(cwd);
                cwd_changed = true;
            }
        }

        let visible = self.workspace.is_pane_visible(pane_id);
        self.dirty_panes.insert(pane_id);
        if cwd_changed {
            self.request_git_status(pane_id);
        }

        if status_changed || !notifications.is_empty() {
            self.agents_dirty = true;
            log::debug!("pane {} program status revision {}", pane_id.0, revision);
        } else if agent_present {
            self.agent_preview_dirty = true;
        }
        for notification in notifications {
            if !agent_present {
                continue;
            }
            self.agent_notifications.insert(
                pane_id,
                PaneNotification {
                    title: notification.title,
                    message: notification.message,
                    at: Instant::now(),
                },
            );
        }

        let focused_cwd_changed = cwd_changed && self.workspace.focused_pane() == Some(pane_id);
        if focused_cwd_changed {
            self.explorer_pinned = false;
        }
        if visible || status_changed || cwd_changed {
            self.window.request_redraw();
        }
        focused_cwd_changed
    }

    fn on_term_event(
        &mut self,
        pane_id: PaneId,
        event: Event,
        event_loop: &ActiveEventLoop,
        config: &Config,
    ) {
        match event {
            Event::PtyWrite(text) => {
                if let Some(pane) = self.workspace.pane_mut(pane_id) {
                    pane.session.write(text.as_bytes());
                }
            }
            Event::Title(title) => {
                let title = clean_pty_title(&title, config.show_user_in_prompt);
                self.workspace.set_pane_title(pane_id, title);
                self.update_window_title();
            }
            Event::ResetTitle => {
                self.workspace.set_pane_title(pane_id, None);
                self.update_window_title();
            }
            Event::Bell => {
                if self.pane_has_agent(pane_id) {
                    self.agent_notifications.insert(
                        pane_id,
                        PaneNotification {
                            title: None,
                            message: None,
                            at: Instant::now(),
                        },
                    );
                    self.agents_dirty = true;
                }
            }
            Event::ColorRequest(index, formatter) => {
                let color = config.theme.special(index);
                let response = formatter(color);
                if let Some(pane) = self.workspace.pane_mut(pane_id) {
                    pane.session.write(response.as_bytes());
                }
            }
            Event::TextAreaSizeRequest(formatter) => {
                if let Some(pane) = self.workspace.pane(pane_id) {
                    let metrics = self.renderer.cell_metrics();
                    let window_size = WindowSize {
                        num_lines: pane.grid.lines as u16,
                        num_cols: pane.grid.cols as u16,
                        cell_width: metrics.width.round().max(1.0) as u16,
                        cell_height: metrics.height.round().max(1.0) as u16,
                    };
                    let response = formatter(window_size);
                    if let Some(pane) = self.workspace.pane_mut(pane_id) {
                        pane.session.write(response.as_bytes());
                    }
                }
            }
            Event::ClipboardStore(_, text) => {
                if let Some(clipboard) = self.clipboard()
                    && let Err(error) = clipboard.set_text(text)
                {
                    log::warn!("OSC 52 clipboard store failed: {error}");
                }
            }
            Event::ClipboardLoad(_, formatter) => {
                let text = self
                    .clipboard()
                    .and_then(|clipboard| clipboard.get_text().ok())
                    .unwrap_or_default();
                let response = formatter(&text);
                if let Some(pane) = self.workspace.pane_mut(pane_id) {
                    pane.session.write(response.as_bytes());
                }
            }
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
            Event::Exit | Event::ChildExit(_) => {
                self.close_pane(pane_id, event_loop, config);
                return;
            }
        }

        self.window.request_redraw();
    }

    fn on_redraw(&mut self, config: &Config) {
        if self.window.is_minimized().unwrap_or(false) {
            return;
        }
        let theme = &config.theme;
        let scale = self.renderer.scale_factor() as f32;
        let metrics = self.renderer.cell_metrics();
        let ui_font_size = self.renderer.physical(config.ui_font_size);
        let header_font_size = (ui_font_size * 0.95).max(10.0);

        if self
            .search
            .as_ref()
            .is_some_and(|search| !self.view.panes.iter().any(|pane| pane.id == search.pane))
        {
            self.search = None;
        }
        if self.search.as_ref().is_some_and(|search| search.dirty) {
            self.refresh_search();
        }

        self.refresh_completion(config);

        let Some(tab) = self.workspace.active_tab() else {
            return;
        };
        let focused = tab.focused;

        // Frames are expensive to build (one String and several Vecs per
        // visible row). Reuse the cached frame for panes whose content did not
        // change; UI-only redraws (hover, footer, agent state) then carry no
        // terminal-grid work.
        let mut frames: Vec<FrameEntry> = Vec::with_capacity(self.view.panes.len());
        let mut built: Vec<BuiltFrame> = Vec::new();
        for pane_layout in &self.view.panes {
            let pane_id = pane_layout.id;
            let Some(pane) = self.workspace.pane(pane_id) else {
                continue;
            };
            let is_dirty = self.dirty_panes.contains(&pane_id);
            let has_cache = self.frame_cache.contains_key(&pane_id);
            let can_update = config.show_user_in_prompt && is_dirty && has_cache;

            let (frame, dominant, dirty_rows) = if !is_dirty && has_cache {
                let frame = self
                    .frame_cache
                    .get(&pane_id)
                    .cloned()
                    .expect("frame cache checked above");
                let dominant = self
                    .frame_dominant
                    .get(&pane_id)
                    .copied()
                    .unwrap_or_else(|| dominant_background(&frame));
                (frame, dominant, None)
            } else if can_update {
                let mut frame = self
                    .frame_cache
                    .remove(&pane_id)
                    .expect("cached frame checked above");
                let old_dominant = self
                    .frame_dominant
                    .get(&pane_id)
                    .copied()
                    .unwrap_or_else(|| dominant_background(&frame));
                let rows = self
                    .workspace
                    .pane_mut(pane_id)
                    .map(|pane| pane.terminal.update_frame(theme, Arc::make_mut(&mut frame)))
                    .unwrap_or_default();
                let dominant = if rows.is_empty() {
                    old_dominant
                } else {
                    dominant_background(&frame)
                };
                built.push((pane_id, frame.clone(), dominant, None));
                (frame, dominant, Some(rows))
            } else {
                let mut frame = pane.terminal.frame(theme);
                let trims = if pane.terminal.mode().contains(TermMode::ALT_SCREEN) {
                    Vec::new()
                } else {
                    collect_prompt_trims(&frame, !config.show_user_in_prompt)
                };
                for trim in &trims {
                    trim_prompt_row(&mut frame, trim.row, trim.columns);
                }
                let dominant = dominant_background(&frame);
                let frame = Arc::new(frame);
                built.push((pane_id, frame.clone(), dominant, Some(trims.clone())));
                (frame, dominant, None)
            };
            frames.push((pane_id, pane_layout.content, frame, dominant, dirty_rows));
        }
        for (pane_id, frame, dominant, trims) in built {
            self.frame_cache.insert(pane_id, frame);
            self.frame_dominant.insert(pane_id, dominant);
            if let Some(trims) = trims {
                if trims.is_empty() {
                    self.prompt_trims.remove(&pane_id);
                } else {
                    self.prompt_trims.insert(pane_id, trims);
                }
            }
        }
        let visible_panes: HashSet<PaneId> = self.view.panes.iter().map(|pane| pane.id).collect();
        self.frame_cache
            .retain(|pane_id, _| visible_panes.contains(pane_id));
        self.frame_dominant
            .retain(|pane_id, _| visible_panes.contains(pane_id));
        self.prompt_trims
            .retain(|pane_id, _| visible_panes.contains(pane_id));

        let editor_frame = if self.diff_view.is_some() {
            None
        } else {
            self.editor
                .as_ref()
                .zip(self.view.editor)
                .map(|(editor, layout)| {
                    let cached = (!self.editor_dirty)
                        .then(|| self.editor_frame_cache.clone())
                        .flatten();
                    let frame = if let Some(frame) = cached {
                        frame
                    } else {
                        let frame =
                            Arc::new(build_editor_frame(editor, layout.text, metrics, theme));
                        self.editor_frame_cache = Some(frame.clone());
                        frame
                    };
                    (layout.text, frame)
                })
        };

        let mut quads = std::mem::take(&mut self.quad_scratch);
        quads.clear();
        let mut labels = std::mem::take(&mut self.label_scratch);
        labels.clear();

        let diff_frame = self
            .diff_view
            .as_ref()
            .zip(self.view.editor)
            .map(|(diff, layout)| {
                let cached = (!self.editor_dirty)
                    .then(|| self.diff_frame_cache.clone())
                    .flatten();
                let rebuilt = cached.is_none();
                let frame = if let Some(frame) = cached {
                    frame
                } else {
                    let frame = Arc::new(build_diff_frame(
                        diff,
                        layout.text,
                        metrics,
                        theme,
                        &mut quads,
                    ));
                    self.diff_frame_cache = Some(frame.clone());
                    frame
                };
                if !rebuilt {
                    push_diff_background_quads(diff, layout.text, metrics, theme, &mut quads);
                }
                (layout.text, frame)
            });

        self.build_sidebar_chrome(
            &config.theme,
            &mut quads,
            &mut labels,
            ui_font_size,
            header_font_size,
        );

        if let Some(drag) = &self.sidebar_drag
            && drag.active
        {
            let mut y = self.view.sidebar.list.y;
            let mut index = 0;
            for row in &self.view.sidebar.rows {
                if let SidebarRow::Workspace(item) = row {
                    if index == drag.target {
                        y = item.rect.y;
                        break;
                    }
                    y = item.rect.bottom();
                    index += 1;
                }
            }
            quads.push(Quad::new(
                self.view.sidebar.list.x,
                y.clamp(self.view.sidebar.list.y, self.view.sidebar.list.bottom()) - 1.0,
                self.view.sidebar.list.width,
                2.0,
                to_linear(theme.accent, 1.0),
            ));
        }

        for divider in &self.view.dividers {
            quads.push(Quad::new(
                divider.line.x,
                divider.line.y,
                divider.line.width,
                divider.line.height,
                to_linear(theme.border, 1.0),
            ));
        }

        if let Some(layout) = self.view.editor {
            let color = if matches!(self.drag, Some(DragState::Editor)) {
                theme.accent
            } else {
                theme.border
            };
            quads.push(Quad::new(
                layout.divider.x,
                layout.divider.y,
                layout.divider.width,
                layout.divider.height,
                to_linear(color, 1.0),
            ));
        }

        // Paint each pane with its dominant terminal background, including
        // full-screen TUIs that set their own SGR background. This keeps the
        // padding, footer and row-rounding strip flush with the TUI instead of
        // showing the app frame.
        for (id, _, _, dominant, _) in &frames {
            let Some(pane_layout) = self.view.panes.iter().find(|pane| pane.id == *id) else {
                continue;
            };
            let outer = pane_layout.outer;
            quads.push(Quad::new(
                outer.x,
                outer.y,
                outer.width,
                outer.height,
                to_linear(*dominant, 1.0),
            ));
        }

        for (_, content, frame, _, _) in &frames {
            for (row_index, row) in frame.rows.iter().enumerate() {
                let y = content.y + row_index as f32 * metrics.height;
                for background in &row.backgrounds {
                    let x = content.x + background.start as f32 * metrics.width;
                    let width = (background.end - background.start) as f32 * metrics.width;
                    quads.push(Quad::new(
                        x,
                        y,
                        width,
                        metrics.height,
                        to_linear(background.color, 1.0),
                    ));
                }
            }
        }

        if let Some(search) = &self.search
            && let Some(pane) = self.workspace.pane(search.pane)
            && let Some(layout) = self.view.panes.iter().find(|pane| pane.id == search.pane)
        {
            let offset = pane.terminal.display_offset() as i32;
            for (index, matched) in search.matches.iter().enumerate() {
                let current = index == search.current;
                let color = if current {
                    theme.accent
                } else {
                    theme.selection
                };
                let alpha = if current { 0.55 } else { 0.35 };
                let start = matched.start();
                let end = matched.end();
                for line in start.line.0..=end.line.0 {
                    let row = line + offset;
                    if row < 0 || row >= pane.grid.lines as i32 {
                        continue;
                    }
                    let start_column = if line == start.line.0 {
                        start.column.0
                    } else {
                        0
                    };
                    let end_column = if line == end.line.0 {
                        end.column.0
                    } else {
                        pane.grid.cols.saturating_sub(1)
                    };
                    if start_column > end_column {
                        continue;
                    }
                    quads.push(Quad::new(
                        layout.content.x + start_column as f32 * metrics.width,
                        layout.content.y + row as f32 * metrics.height,
                        (end_column - start_column + 1) as f32 * metrics.width,
                        metrics.height,
                        to_linear(color, alpha),
                    ));
                }
            }
        }

        if let Some(link) = &self.hovered_link
            && let Some(pane) = self.workspace.pane(link.pane)
            && let Some(layout) = self.view.panes.iter().find(|pane| pane.id == link.pane)
        {
            let offset = pane.terminal.display_offset() as i32;
            let start = link.range.start();
            let end = link.range.end();
            for line in start.line.0..=end.line.0 {
                let row = line + offset;
                if row < 0 || row >= pane.grid.lines as i32 {
                    continue;
                }
                let start_column = if line == start.line.0 {
                    start.column.0
                } else {
                    0
                };
                let end_column = if line == end.line.0 {
                    end.column.0
                } else {
                    pane.grid.cols.saturating_sub(1)
                };
                if start_column > end_column {
                    continue;
                }
                quads.push(Quad::new(
                    layout.content.x + start_column as f32 * metrics.width,
                    layout.content.y + (row as f32 + 1.0) * metrics.height - scale,
                    (end_column - start_column + 1) as f32 * metrics.width,
                    scale.max(1.0),
                    to_linear(theme.accent, 1.0),
                ));
            }
        }

        for (_, content, frame, _, _) in &frames {
            push_cursor_quads(&mut quads, *content, frame, metrics, theme);
        }
        if let Some((content, frame)) = &editor_frame {
            push_cursor_quads(&mut quads, *content, frame.as_ref(), metrics, theme);
        }

        let mut footer_hits = Vec::with_capacity(self.view.panes.len());
        {
            let view = &self.view;
            let workspace = &self.workspace;
            let renderer = &mut self.renderer;
            for pane_layout in &view.panes {
                let Some(pane) = workspace.pane(pane_layout.id) else {
                    continue;
                };
                let hits = build_pane_footer(
                    pane,
                    pane_footer_band(pane_layout, metrics),
                    PaneFooterStyle {
                        scale,
                        ui_font_size,
                        theme,
                    },
                    renderer,
                    &mut quads,
                    &mut labels,
                );
                footer_hits.push((pane_layout.id, hits));
            }
        }
        self.footer_hits.clear();
        self.footer_hits.extend(footer_hits);

        if let Some(layout) = self.view.editor {
            quads.push(Quad::new(
                layout.header.x,
                layout.header.y,
                layout.header.width,
                layout.header.height,
                to_linear(theme.sidebar_background, 1.0),
            ));
            quads.push(Quad::new(
                layout.header.x,
                layout.header.bottom() - 1.0,
                layout.header.width,
                1.0,
                to_linear(theme.border, 1.0),
            ));

            let focused_panel = matches!(self.focus, Focus::Editor | Focus::Diff);
            let thickness = if focused_panel {
                (2.0 * scale).round().max(1.0)
            } else {
                1.0
            };
            let border = if focused_panel {
                to_linear(theme.accent, 1.0)
            } else {
                to_linear(theme.border, 1.0)
            };
            quads.push(Quad::new(
                layout.panel.x,
                layout.panel.y,
                layout.panel.width,
                thickness,
                border,
            ));
            quads.push(Quad::new(
                layout.panel.x,
                layout.panel.bottom() - thickness,
                layout.panel.width,
                thickness,
                border,
            ));
            quads.push(Quad::new(
                layout.panel.x,
                layout.panel.y,
                thickness,
                layout.panel.height,
                border,
            ));
            quads.push(Quad::new(
                layout.panel.right() - thickness,
                layout.panel.y,
                thickness,
                layout.panel.height,
                border,
            ));

            let header_font = ui_font_size * 0.88;
            let header_line = header_font * 1.35;
            let title = if let Some(diff) = &self.diff_view {
                diff.detail
                    .as_ref()
                    .map(|detail| {
                        format!(
                            "Cambios locales · {} archivos · +{} −{}",
                            detail.files.len(),
                            detail.additions,
                            detail.deletions
                        )
                    })
                    .unwrap_or_else(|| "Cambios locales".to_owned())
            } else {
                self.editor
                    .as_ref()
                    .map(|editor| editor.path.display().to_string())
                    .unwrap_or_default()
            };
            let editor_dirty =
                self.diff_view.is_none() && self.editor.as_ref().is_some_and(|editor| editor.dirty);
            let dirty_width = if editor_dirty { 16.0 * scale } else { 0.0 };

            labels.push(Label {
                text: title,
                x: layout.header.x + layout.padding,
                y: layout.header.y + (layout.header.height - header_line) / 2.0,
                width: (layout.close.x - layout.header.x - layout.padding * 2.0 - dirty_width)
                    .max(1.0),
                font_size: header_font,
                color: theme.sidebar_text,
                bold: false,
            });

            if editor_dirty {
                labels.push(Label {
                    text: "●".into(),
                    x: layout.close.x - 16.0 * scale,
                    y: layout.header.y + (layout.header.height - header_font * 1.35) / 2.0,
                    width: 12.0 * scale,
                    font_size: header_font,
                    color: theme.accent,
                    bold: false,
                });
            }

            push_close_icon(&mut quads, layout.close, scale, theme.sidebar_text);
        }

        let mut branch_occlusion = None;
        if let Some(menu) = &self.branch_menu
            && let Some(anchor) = self
                .footer_hits
                .get(&menu.pane)
                .and_then(|hits| hits.branch)
            && let Some(pane_layout) = self.view.panes.iter().find(|pane| pane.id == menu.pane)
        {
            let layout = branch_menu_layout(menu, anchor, pane_layout.outer, scale, ui_font_size);
            if let Some((_, content, frame, _, _)) =
                frames.iter_mut().find(|(id, _, _, _, _)| *id == menu.pane)
            {
                let frame = Arc::make_mut(frame);
                let rect = layout.rect;
                let row_start = (((rect.y - content.y) / metrics.height).floor() as isize)
                    .clamp(0, frame.rows.len() as isize - 1)
                    as usize;
                let row_end = (((rect.bottom() - content.y) / metrics.height).ceil() as usize)
                    .min(frame.rows.len());
                let col_start = (((rect.x - content.x) / metrics.width).floor() as isize)
                    .clamp(0, frame.cols as isize - 1) as usize;
                let col_end =
                    (((rect.right() - content.x) / metrics.width).ceil() as usize).min(frame.cols);
                blank_region(frame, row_start..row_end, col_start..col_end, theme);
                branch_occlusion = Some((menu.pane, row_start, row_end, col_start, col_end));
            }

            build_branch_menu(
                menu,
                anchor,
                pane_layout.outer,
                (self.cursor_position.x as f32, self.cursor_position.y as f32),
                BranchMenuStyle {
                    scale,
                    ui_font_size,
                    theme,
                },
                &mut quads,
                &mut labels,
            );
        }

        let mut popup_occlusion = branch_occlusion;
        if self.completion_open
            && let Some(view) = &self.completion_view
            && let Some((_, content, frame, _, _)) =
                frames.iter_mut().find(|(id, _, _, _, _)| *id == view.pane)
        {
            let frame = Arc::make_mut(frame);
            let layout = popup_layout(
                PopupGeometry {
                    content: *content,
                    row: view.row,
                    col: view
                        .col
                        .saturating_sub(self.prompt_shift(view.pane, view.row)),
                    selected: self.completion_selected,
                    metrics,
                    ui_font_size,
                    scale,
                },
                &view.candidates,
            );
            let rect = layout.rect;
            if rect.width > 1.0 && rect.height > 1.0 {
                quads.push(Quad::rounded(
                    rect.x - 1.0,
                    rect.y - 1.0,
                    rect.width + 2.0,
                    rect.height + 2.0,
                    5.0 * scale,
                    to_linear(theme.border, 1.0),
                ));
                quads.push(Quad::rounded(
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    4.0 * scale,
                    to_linear(theme.sidebar_background, 1.0),
                ));

                let selected_y = rect.y
                    + layout.padding
                    + (self.completion_selected - layout.first) as f32 * layout.row_height;
                quads.push(Quad::rounded(
                    rect.x + layout.padding * 0.5,
                    selected_y,
                    rect.width - layout.padding,
                    layout.row_height,
                    3.0 * scale,
                    to_linear(theme.sidebar_active, 0.95),
                ));

                let detail_width = (rect.width * 0.30).max(40.0 * scale);
                let label_width = (rect.width - layout.padding * 2.0 - detail_width).max(1.0);
                let line_height = ui_font_size * 1.35;
                for (offset, candidate) in view.candidates[layout.first..layout.first + layout.rows]
                    .iter()
                    .enumerate()
                {
                    let index = layout.first + offset;
                    let selected = index == self.completion_selected;
                    let row_y = rect.y
                        + layout.padding
                        + offset as f32 * layout.row_height
                        + (layout.row_height - line_height) / 2.0;
                    labels.push(Label {
                        text: candidate.label.clone(),
                        x: rect.x + layout.padding,
                        y: row_y,
                        width: label_width,
                        font_size: ui_font_size,
                        color: if selected {
                            theme.foreground
                        } else {
                            theme.sidebar_text
                        },
                        bold: selected,
                    });
                    labels.push(Label {
                        text: candidate.detail.clone(),
                        x: rect.right() - layout.padding - detail_width,
                        y: row_y,
                        width: detail_width,
                        font_size: ui_font_size * 0.86,
                        color: theme.sidebar_text_dim,
                        bold: false,
                    });
                }

                let row_start = (((rect.y - content.y) / metrics.height).floor() as isize)
                    .clamp(0, frame.rows.len() as isize - 1)
                    as usize;
                let row_end = (((rect.bottom() - content.y) / metrics.height).ceil() as usize)
                    .min(frame.rows.len());
                let col_start = (((rect.x - content.x) / metrics.width).floor() as isize)
                    .clamp(0, frame.cols as isize - 1) as usize;
                let col_end =
                    (((rect.right() - content.x) / metrics.width).ceil() as usize).min(frame.cols);
                blank_region(frame, row_start..row_end, col_start..col_end, theme);
                popup_occlusion = Some((view.pane, row_start, row_end, col_start, col_end));
            }
        }

        if popup_occlusion != self.popup_occlusion {
            if let Some((pane, _, _, _, _)) = self.popup_occlusion {
                self.dirty_panes.insert(pane);
            }
            if let Some((pane, _, _, _, _)) = popup_occlusion {
                self.dirty_panes.insert(pane);
            }
            self.popup_occlusion = popup_occlusion;
        }

        let ghost = self.ghost_text(config);
        let ghost_pane = if ghost.is_some() {
            self.completion_view.as_ref().map(|view| view.pane)
        } else {
            None
        };

        let mut scene_panes: Vec<PaneLayer<'_>> = frames
            .iter()
            .map(|(id, content, frame, _, dirty_rows)| PaneLayer {
                id: id.0,
                content: *content,
                frame: frame.as_ref(),
                dirty: self.dirty_panes.contains(id),
                dirty_rows: dirty_rows.clone(),
                ghost: if Some(*id) == ghost_pane {
                    ghost.clone()
                } else {
                    None
                },
            })
            .collect();
        if let Some((content, frame)) = diff_frame.as_ref().or(editor_frame.as_ref()) {
            scene_panes.push(PaneLayer {
                id: 0,
                content: *content,
                frame: frame.as_ref(),
                dirty: self.editor_dirty,
                dirty_rows: None,
                ghost: None,
            });
        }

        if let Some(search) = &self.search
            && let Some(bar) = self.search_bar_rect()
        {
            quads.push(Quad::rounded(
                bar.x,
                bar.y,
                bar.width,
                bar.height,
                6.0 * scale,
                to_linear(theme.sidebar_background, 0.98),
            ));
            quads.push(Quad::new(
                bar.x,
                bar.y,
                bar.width,
                (1.0 * scale).max(1.0),
                to_linear(theme.accent, 0.9),
            ));
            let count = if search.matches.is_empty() {
                "0/0".to_owned()
            } else {
                format!("{}/{}", search.current + 1, search.matches.len())
            };
            let text = if search.query.is_empty() {
                "Buscar en el scrollback…".to_owned()
            } else {
                format!("{}▌  {count}", search.query)
            };
            let font = (ui_font_size * 0.9).max(10.0);
            labels.push(Label {
                text,
                x: bar.x + 10.0 * scale,
                y: bar.y + (bar.height - font * 1.25) * 0.5,
                width: (bar.width - 20.0 * scale).max(1.0),
                font_size: font,
                color: if search.query.is_empty() {
                    theme.sidebar_text_dim
                } else {
                    theme.foreground
                },
                bold: false,
            });
        }

        let scene = Scene {
            panes: scene_panes,
            labels,
            quads,
        };

        if let Err(error) = self.renderer.render(&scene, theme) {
            log::error!("render failed: {error:#}");
        }
        let Scene {
            panes: _,
            labels,
            quads,
        } = scene;
        self.label_scratch = labels;
        self.quad_scratch = quads;

        self.dirty_panes.clear();
        self.editor_dirty = false;

        let ime_target = match self.focus {
            Focus::Editor => editor_frame
                .as_ref()
                .map(|(content, frame)| (*content, frame.as_ref())),
            Focus::Terminal => frames
                .iter()
                .find(|(id, _, _, _, _)| *id == focused)
                .map(|(_, content, frame, _, _)| (*content, frame.as_ref())),
            Focus::Diff => None,
        };

        if let Some((content, frame)) = ime_target
            && let Some(cursor) = frame.cursor
        {
            let position = PhysicalPosition::new(
                (content.x + cursor.col as f32 * metrics.width) as f64,
                (content.y + cursor.row as f32 * metrics.height) as f64,
            );
            let size = PhysicalSize::new(metrics.width as u32, metrics.height as u32);
            self.window.set_ime_cursor_area(position, size);
        }
    }

    fn build_sidebar_chrome(
        &self,
        theme: &Theme,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
        ui_font_size: f32,
        header_font_size: f32,
    ) {
        let layout = &self.view.sidebar;
        if layout.rect.width <= 0.0 {
            return;
        }

        let scale = self.renderer.scale_factor() as f32;

        quads.push(Quad::new(
            layout.rect.x,
            layout.rect.y,
            layout.rect.width,
            layout.rect.height,
            to_linear(theme.sidebar_background, 1.0),
        ));
        quads.push(Quad::new(
            layout.rect.right() - 1.0,
            layout.rect.y,
            1.0,
            layout.rect.height,
            to_linear(theme.border, 1.0),
        ));

        if layout.collapsed {
            let chevron_size = ui_font_size * 0.95;
            labels.push(Label {
                text: "»".into(),
                x: layout.rect.x + (layout.rect.width - chevron_size * 0.55) / 2.0,
                y: layout.padding,
                width: layout.rect.width,
                font_size: chevron_size,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            return;
        }

        let header_line_height = header_font_size * 1.35;
        let title = if layout.profile_menu {
            "Nueva terminal"
        } else {
            match layout.mode {
                SidebarMode::Terminals => "Sesiones",
                SidebarMode::Files => "Archivos",
            }
        };
        labels.push(Label {
            text: title.into(),
            x: layout.header_label.x,
            y: layout.header.y + (layout.header.height - header_line_height) / 2.0,
            width: layout.header_label.width,
            font_size: header_font_size,
            color: theme.sidebar_text_dim,
            bold: true,
        });

        let collapse_size = ui_font_size * 0.95;
        labels.push(Label {
            text: "«".into(),
            x: layout.collapse_button.x
                + (layout.collapse_button.width - collapse_size * 0.55) / 2.0,
            y: layout.collapse_button.y
                + (layout.collapse_button.height - collapse_size * 1.35) / 2.0,
            width: layout.collapse_button.width,
            font_size: collapse_size,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        quads.push(Quad::new(
            layout.mode_button.x,
            layout.mode_button.y,
            layout.mode_button.width,
            layout.mode_button.height,
            to_linear(theme.sidebar_hover, 1.0),
        ));
        let mode_size = ui_font_size * 0.95;
        let mode_label = match layout.mode {
            SidebarMode::Terminals => "F",
            SidebarMode::Files => "T",
        };
        labels.push(Label {
            text: mode_label.into(),
            x: layout.mode_button.x + (layout.mode_button.width - mode_size * 0.55) / 2.0,
            y: layout.mode_button.y + (layout.mode_button.height - mode_size * 1.35) / 2.0,
            width: layout.mode_button.width,
            font_size: mode_size,
            color: theme.accent,
            bold: false,
        });

        let action_size = ui_font_size * 1.15;
        let action_label = match layout.mode {
            SidebarMode::Terminals => "+",
            SidebarMode::Files => "↻",
        };
        labels.push(Label {
            text: action_label.into(),
            x: layout.new_button.x + (layout.new_button.width - action_size * 0.55) / 2.0,
            y: layout.new_button.y + (layout.new_button.height - action_size * 1.35) / 2.0,
            width: layout.new_button.width,
            font_size: action_size,
            color: theme.accent,
            bold: false,
        });

        if layout.mode == SidebarMode::Terminals {
            let chevron_size = ui_font_size * 0.9;
            labels.push(Label {
                text: "▾".into(),
                x: layout.profile_button.x
                    + (layout.profile_button.width - chevron_size * 0.55) / 2.0,
                y: layout.profile_button.y
                    + (layout.profile_button.height - chevron_size * 1.35) / 2.0,
                width: layout.profile_button.width,
                font_size: chevron_size,
                color: if layout.profile_menu {
                    theme.accent
                } else {
                    theme.sidebar_text_dim
                },
                bold: layout.profile_menu,
            });
        }

        if layout.profile_menu {
            let default_index = self.shells.index_of(&self.shells.default_id);
            for item in &layout.profile_items {
                let Some(profile) = self.shells.profiles.get(item.index) else {
                    continue;
                };
                let hovered = self.sidebar.hovered_profile == Some(item.index);
                let selected = self.sidebar.profile_selected == item.index;
                if hovered || selected {
                    quads.push(Quad::new(
                        item.rect.x,
                        item.rect.y,
                        item.rect.width,
                        item.rect.height,
                        to_linear(
                            if selected {
                                theme.sidebar_active
                            } else {
                                theme.sidebar_hover
                            },
                            1.0,
                        ),
                    ));
                }
                if selected {
                    quads.push(Quad::new(
                        item.rect.x,
                        item.rect.y,
                        2.0 * scale,
                        item.rect.height,
                        to_linear(theme.accent, 1.0),
                    ));
                }

                let is_default = Some(item.index) == default_index;
                labels.push(Label {
                    text: profile.name.clone(),
                    x: item.rect.x + 10.0 * scale,
                    y: item.rect.y + (item.rect.height - ui_font_size * 1.35) / 2.0,
                    width: (item.rect.width - 68.0 * scale).max(1.0),
                    font_size: ui_font_size * 0.88,
                    color: if selected || is_default {
                        theme.sidebar_text
                    } else {
                        theme.sidebar_text_dim
                    },
                    bold: selected || is_default,
                });
                labels.push(Label {
                    text: profile.short.clone(),
                    x: item.rect.right() - 52.0 * scale,
                    y: item.rect.y + (item.rect.height - ui_font_size * 1.35) / 2.0,
                    width: 42.0 * scale,
                    font_size: ui_font_size * 0.7,
                    color: if is_default {
                        theme.accent
                    } else {
                        theme.sidebar_text_dim
                    },
                    bold: false,
                });
            }

            if self.shells.profiles.is_empty() {
                labels.push(Label {
                    text: "No se detectaron shells".into(),
                    x: layout.list.x + layout.padding,
                    y: layout.list.y + layout.padding,
                    width: (layout.list.width - layout.padding * 2.0).max(1.0),
                    font_size: ui_font_size * 0.85,
                    color: theme.sidebar_text_dim,
                    bold: false,
                });
            }
            return;
        }

        if layout.mode == SidebarMode::Files {
            let can_go_up = self.explorer.root.parent().is_some();
            quads.push(Quad::rounded(
                layout.back_button.x,
                layout.back_button.y,
                layout.back_button.width,
                layout.back_button.height,
                4.0 * scale,
                to_linear(
                    if can_go_up {
                        theme.sidebar_hover
                    } else {
                        theme.background
                    },
                    1.0,
                ),
            ));
            let back_font = ui_font_size * 0.9;
            labels.push(Label {
                text: "←".into(),
                x: layout.back_button.x + (layout.back_button.width - back_font * 0.55) / 2.0,
                y: layout.back_button.y + (layout.back_button.height - back_font * 1.35) / 2.0,
                width: layout.back_button.width,
                font_size: back_font,
                color: if can_go_up {
                    theme.accent
                } else {
                    theme.sidebar_text_dim
                },
                bold: false,
            });

            let path_font = ui_font_size * 0.82;
            let path_line = path_font * 1.35;
            labels.push(Label {
                text: self.explorer.display_root(),
                x: layout.path_rect.x,
                y: layout.path_rect.y + (layout.path_rect.height - path_line) / 2.0,
                width: layout.path_rect.width,
                font_size: path_font,
                color: theme.sidebar_text_dim,
                bold: false,
            });
        }

        let line_height = ui_font_size * 1.35;

        match layout.mode {
            SidebarMode::Terminals => {
                let hovered_tab = self.sidebar.hovered;
                let hovered_pane = self.sidebar.hovered_pane;

                for row in &layout.rows {
                    match row {
                        SidebarRow::Workspace(item) => {
                            let is_hovered = Some(item.tab) == hovered_tab;
                            if item.is_active || is_hovered {
                                let background = if item.is_active {
                                    theme.sidebar_active
                                } else {
                                    theme.sidebar_hover
                                };
                                quads.push(Quad::new(
                                    item.rect.x,
                                    item.rect.y,
                                    item.rect.width,
                                    item.rect.height,
                                    to_linear(background, 1.0),
                                ));
                            }
                            if item.is_active {
                                quads.push(Quad::new(
                                    item.rect.x,
                                    item.rect.y,
                                    2.0 * scale,
                                    item.rect.height,
                                    to_linear(theme.accent, 1.0),
                                ));
                            }

                            if item.pane_count > 1 {
                                let chevron_size = ui_font_size * 0.82;
                                labels.push(Label {
                                    text: if item.expanded { "▾" } else { "▸" }.into(),
                                    x: item.toggle.x
                                        + (item.toggle.width - chevron_size * 0.55) / 2.0,
                                    y: item.toggle.y
                                        + (item.toggle.height - chevron_size * 1.35) / 2.0,
                                    width: item.toggle.width,
                                    font_size: chevron_size,
                                    color: if item.is_active {
                                        theme.accent
                                    } else {
                                        theme.sidebar_text_dim
                                    },
                                    bold: false,
                                });
                            }

                            let title = self
                                .workspace
                                .tabs
                                .iter()
                                .find(|tab| tab.id == item.tab)
                                .map(|tab| tab.title.as_str())
                                .unwrap_or_default();
                            labels.push(Label {
                                text: title.to_owned(),
                                x: item.label.x,
                                y: item.label.y + (item.label.height - line_height) / 2.0,
                                width: item.label.width,
                                font_size: ui_font_size,
                                color: if item.is_active {
                                    theme.sidebar_text
                                } else {
                                    theme.sidebar_text_dim
                                },
                                bold: item.is_active,
                            });

                            if item.pane_count > 1 {
                                labels.push(Label {
                                    text: item.pane_count.to_string(),
                                    x: item.close.x - 20.0 * scale,
                                    y: item.close.y
                                        + (item.close.height - ui_font_size * 1.35) / 2.0,
                                    width: 16.0 * scale,
                                    font_size: ui_font_size * 0.7,
                                    color: theme.sidebar_text_dim,
                                    bold: false,
                                });
                            }

                            if let Some(state) = self.tab_agent_state(item.tab) {
                                push_agent_dot(
                                    &mut *quads,
                                    item.close.x - 36.0 * scale,
                                    item.rect.y + item.rect.height / 2.0,
                                    scale,
                                    agent_state_color(theme, &state),
                                );
                            }

                            push_close_icon(&mut *quads, item.close, scale, theme.sidebar_text);
                        }
                        SidebarRow::Pane(item) => {
                            let is_hovered = Some(item.pane) == hovered_pane;
                            if item.is_focused || is_hovered {
                                let background = if item.is_focused {
                                    theme.sidebar_active
                                } else {
                                    theme.sidebar_hover
                                };
                                quads.push(Quad::new(
                                    item.rect.x + 12.0 * scale,
                                    item.rect.y,
                                    (item.rect.width - 12.0 * scale).max(1.0),
                                    item.rect.height,
                                    to_linear(background, 1.0),
                                ));
                            }
                            if item.is_focused {
                                quads.push(Quad::new(
                                    item.rect.x + 12.0 * scale,
                                    item.rect.y,
                                    2.0 * scale,
                                    item.rect.height,
                                    to_linear(theme.accent, 1.0),
                                ));
                            }

                            let connector_size = ui_font_size * 0.9;
                            labels.push(Label {
                                text: if item.last { "└" } else { "├" }.into(),
                                x: item.rect.x + 20.0 * scale,
                                y: item.rect.y + (item.rect.height - connector_size * 1.35) / 2.0,
                                width: 16.0 * scale,
                                font_size: connector_size,
                                color: theme.sidebar_text_dim,
                                bold: false,
                            });

                            let pane = self.workspace.pane(item.pane);
                            let profile = pane
                                .and_then(|pane| self.shells.get(&pane.profile_id))
                                .map(|profile| profile.short.as_str())
                                .unwrap_or("sh");
                            let title = pane
                                .and_then(|pane| pane.title.clone())
                                .filter(|title| !title.is_empty())
                                .unwrap_or_else(|| profile.to_owned());

                            labels.push(Label {
                                text: title,
                                x: item.label.x,
                                y: item.label.y
                                    + (item.label.height - ui_font_size * 0.88 * 1.35) / 2.0,
                                width: item.label.width,
                                font_size: ui_font_size * 0.88,
                                color: if item.is_focused {
                                    theme.sidebar_text
                                } else {
                                    theme.sidebar_text_dim
                                },
                                bold: item.is_focused,
                            });

                            if let Some(state) = self.pane_agent_state(item.pane) {
                                push_agent_dot(
                                    &mut *quads,
                                    item.close.x - 26.0 * scale,
                                    item.rect.y + item.rect.height / 2.0,
                                    scale,
                                    agent_state_color(theme, &state),
                                );
                            }

                            push_close_icon(&mut *quads, item.close, scale, theme.sidebar_text);
                        }
                    }
                }
            }
            SidebarMode::Files => {
                let hovered = self.sidebar.hovered_file;
                let file_font = ui_font_size * 0.92;

                for item in &layout.file_items {
                    let Some(row) = self.explorer.rows.get(item.row) else {
                        continue;
                    };

                    let is_hovered = Some(item.row) == hovered;
                    let is_open = self
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.path == row.path);

                    if is_hovered || is_open {
                        let background = if is_open {
                            theme.sidebar_active
                        } else {
                            theme.sidebar_hover
                        };
                        quads.push(Quad::new(
                            item.rect.x,
                            item.rect.y,
                            item.rect.width,
                            item.rect.height,
                            to_linear(background, 1.0),
                        ));
                    }

                    if is_open {
                        quads.push(Quad::new(
                            item.rect.x,
                            item.rect.y,
                            2.0 * scale,
                            item.rect.height,
                            to_linear(theme.accent, 1.0),
                        ));
                    }

                    let indent = row.depth as f32 * 12.0 * scale;
                    let icon = if row.is_dir {
                        if row.expanded { "▾" } else { "▸" }
                    } else {
                        "·"
                    };
                    let x = item.label.x + indent;
                    let width = (item.rect.right() - x - 4.0 * scale).max(1.0);

                    labels.push(Label {
                        text: format!("{icon} {}", row.name),
                        x,
                        y: item.label.y + (item.label.height - file_font * 1.35) / 2.0,
                        width,
                        font_size: file_font,
                        color: if is_open {
                            theme.sidebar_text
                        } else {
                            theme.sidebar_text_dim
                        },
                        bold: is_open,
                    });
                }

                if self.explorer.rows.is_empty() {
                    let message = self
                        .explorer
                        .error
                        .clone()
                        .unwrap_or_else(|| "empty directory".into());
                    labels.push(Label {
                        text: message,
                        x: layout.list.x + layout.padding,
                        y: layout.list.y + layout.padding,
                        width: (layout.list.width - layout.padding * 2.0).max(1.0),
                        font_size: ui_font_size * 0.85,
                        color: theme.sidebar_text_dim,
                        bold: false,
                    });
                }
            }
        }
    }
}

fn process_agent_row(
    pane: &Pane,
    tab_title: &str,
    pane_label: Option<String>,
    agent: &DetectedAgent,
    notification: Option<&PaneNotification>,
    started: u64,
    preview_lines: Vec<String>,
) -> AgentRow {
    AgentRow {
        id: AgentId::Terminal {
            pane: pane.id,
            record: format!("process:{}", agent.pid),
        },
        kind: agent.kind.clone(),
        state: fallback_agent_state(pane, notification),
        message: notification.and_then(notification_message),
        title: pane.title.clone(),
        cwd: agent.cwd.clone().or_else(|| pane.cwd.clone()),
        pane: Some(pane.id),
        tab_title: Some(tab_title.to_owned()),
        pane_label,
        pid: Some(agent.pid),
        started,
        progress: None,
        source: AgentSource::Process,
        subagents: Vec::new(),
        seen: false,
        memory_bytes: Some(agent.memory_bytes),
        cpu_tenths: Some(agent.cpu_tenths),
        child_processes: agent.child_processes,
        command: agent.command.clone(),
        git_branch: pane.git_status.as_ref().map(|status| status.branch.clone()),
        git_diff: pane
            .git_status
            .as_ref()
            .map(|status| (status.additions, status.deletions)),
        last_output: Some(pane.last_output),
        preview_lines,
    }
}

fn notification_message(notification: &PaneNotification) -> Option<String> {
    notification
        .message
        .clone()
        .or_else(|| notification.title.clone())
}

fn pane_label(pane: &Pane, shells: &ShellCatalog) -> Option<String> {
    pane.title
        .as_deref()
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            shells
                .get(&pane.profile_id)
                .map(|profile| profile.short.clone())
        })
}

fn looks_finished(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("complete")
        || message.contains("finished")
        || message.contains("turn ended")
        || message.contains("done")
}

fn fallback_agent_state(pane: &Pane, notification: Option<&PaneNotification>) -> AgentState {
    let quiet = pane.last_output.elapsed();
    if let Some(notification) = notification {
        if quiet > Duration::from_millis(800) {
            if notification_message(notification)
                .as_deref()
                .is_some_and(looks_finished)
            {
                return AgentState::Done;
            }
            return AgentState::Attention;
        }
        return AgentState::Working;
    }

    if quiet <= Duration::from_millis(2500) {
        AgentState::Working
    } else {
        AgentState::Idle
    }
}

fn protocol_agent_state(
    state: ProgramState,
    block: Option<BlockKind>,
    notification: Option<&PaneNotification>,
    quiet: Duration,
) -> AgentState {
    match state {
        ProgramState::Working => AgentState::Working,
        ProgramState::Blocked => match block {
            Some(BlockKind::Permission) => AgentState::Blocked(BlockKind::Permission),
            Some(BlockKind::Question) => AgentState::Blocked(BlockKind::Question),
            Some(BlockKind::Auth) => AgentState::Blocked(BlockKind::Auth),
            None => AgentState::Attention,
        },
        ProgramState::Done => AgentState::Done,
        ProgramState::Error => AgentState::Error,
        ProgramState::Idle => {
            if let Some(notification) = notification
                && quiet > Duration::from_millis(800)
            {
                if notification_message(notification)
                    .as_deref()
                    .is_some_and(looks_finished)
                {
                    return AgentState::Done;
                }
                return AgentState::Attention;
            }
            AgentState::Idle
        }
    }
}

fn epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn open_url(uri: &str) -> bool {
    if !(uri.starts_with("http://") || uri.starts_with("https://") || uri.starts_with("mailto:")) {
        return false;
    }

    #[cfg(windows)]
    {
        let mut command = crate::process::command("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(uri);
        spawn_url(command, uri)
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = crate::process::command("open");
        command.arg(uri);
        spawn_url(command, uri)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = crate::process::command("xdg-open");
        command.arg(uri);
        spawn_url(command, uri)
    }

    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

#[cfg(any(windows, unix))]
fn spawn_url(mut command: std::process::Command, uri: &str) -> bool {
    match command.spawn() {
        Ok(_) => true,
        Err(error) => {
            log::warn!("could not open {uri}: {error}");
            false
        }
    }
}

fn config_modified_time() -> Option<SystemTime> {
    config_path()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|metadata| metadata.modified().ok())
}

fn default_directory() -> PathBuf {
    let interactive = std::io::stdin().is_terminal() || std::io::stdout().is_terminal();
    let cwd = std::env::current_dir().ok();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    choose_launch_directory(interactive, cwd, home)
}

/// Normalize the title reported by the shell.
///
/// ConPTY (and cmd.exe) use the current directory as the console title, which
/// looks like noise in the sidebar. Path-only titles are dropped so the pane
/// keeps its default label. The `user@host` prefix is optionally removed as
/// well.
fn clean_pty_title(title: &str, show_user: bool) -> Option<String> {
    let mut title = title.trim().to_owned();
    if !show_user && let Some(len) = complete::line::user_host_prefix_len(&title) {
        title = title.chars().skip(len).collect::<String>();
        title = title.trim_start().to_owned();
    }

    let title = title.trim();
    if title.is_empty() || title_looks_like_path(title) {
        return None;
    }
    Some(title.to_owned())
}

/// Detect terminal titles that are only a filesystem path.
fn title_looks_like_path(title: &str) -> bool {
    let bytes = title.as_bytes();
    let windows_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');

    windows_drive
        || title.starts_with('/')
        || title.starts_with('\\')
        || title == "~"
        || title.starts_with("~/")
        || title.starts_with("~\\")
}

fn choose_launch_directory(
    interactive: bool,
    cwd: Option<PathBuf>,
    home: Option<PathBuf>,
) -> PathBuf {
    if !interactive && let Some(home) = home {
        return home;
    }
    cwd.or(home).unwrap_or_else(|| PathBuf::from("."))
}

/// Area from the last rendered terminal row down to the bottom of the pane.
///
/// The grid rounds down to whole rows, so there is usually a small unused
/// strip above the reserved footer. The footer is drawn in that strip too,
/// which keeps its text at a readable size without stealing rows from the
/// terminal.
fn pane_footer_band(pane: &PaneLayout, metrics: CellMetrics) -> Rect {
    let rows = (pane.content.height / metrics.height).floor().max(1.0);
    let top = (pane.content.y + rows * metrics.height).min(pane.footer.bottom());
    Rect::new(
        pane.footer.x,
        top,
        pane.footer.width,
        (pane.footer.bottom() - top).max(1.0),
    )
}

/// Most common background color in a frame, including the default one.
///
/// Full-screen TUIs such as opencode can paint their own background with SGR
/// colors instead of OSC 11. Using the dominant color for the pane lets the
/// footer share that background and removes the app-colored strip around it.
fn dominant_background(frame: &Frame) -> Rgb {
    let total = frame.rows.len().saturating_mul(frame.cols).max(1);
    let mut counts: Vec<(Rgb, usize)> = Vec::new();

    for row in &frame.rows {
        for background in &row.backgrounds {
            let width = background.end.saturating_sub(background.start);
            if width == 0 {
                continue;
            }
            if let Some(entry) = counts
                .iter_mut()
                .find(|(color, _)| *color == background.color)
            {
                entry.1 += width;
            } else {
                counts.push((background.color, width));
            }
        }
    }

    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count * 2 >= total)
        .map(|(color, _)| color)
        .unwrap_or(frame.background)
}

fn branch_menu_layout(
    menu: &BranchMenu,
    anchor: Rect,
    outer: Rect,
    scale: f32,
    ui_font_size: f32,
) -> BranchMenuLayout {
    let padding = (6.0 * scale).max(4.0);
    let header_height = (ui_font_size * 1.75).max(22.0 * scale);
    let row_height = (ui_font_size * 1.75).max(24.0 * scale);
    let message_height = if menu.error.is_some() || menu.busy.is_some() {
        row_height
    } else {
        0.0
    };

    let max_width = (outer.width - 8.0 * scale).max(1.0);
    let width = (240.0 * scale)
        .max(anchor.width + 24.0 * scale)
        .min(max_width);

    let available_rows = ((outer.height
        - 8.0 * scale
        - padding * 2.0
        - header_height
        - message_height)
        / row_height)
        .floor()
        .max(1.0) as usize;
    let visible_rows = menu.branches.len().clamp(1, 10.min(available_rows));
    let height =
        (padding + header_height + visible_rows as f32 * row_height + message_height + padding)
            .min((outer.height - 8.0 * scale).max(1.0));

    let x = anchor
        .x
        .min(outer.right() - width - 4.0 * scale)
        .max(outer.x + 4.0 * scale);
    let mut y = anchor.y - height - 4.0 * scale;
    if y < outer.y + 4.0 * scale {
        y = (anchor.bottom() + 4.0 * scale)
            .min(outer.bottom() - height - 4.0 * scale)
            .max(outer.y + 4.0 * scale);
    }

    let list = Rect::new(
        x + padding,
        y + padding + header_height,
        (width - padding * 2.0).max(1.0),
        visible_rows as f32 * row_height,
    );
    let message = (menu.error.is_some() || menu.busy.is_some()).then(|| {
        Rect::new(
            x + padding,
            list.bottom(),
            (width - padding * 2.0).max(1.0),
            row_height,
        )
    });
    let offset = if menu.branches.is_empty() {
        0
    } else {
        menu.selected
            .saturating_sub(visible_rows.saturating_sub(1))
            .min(menu.branches.len().saturating_sub(visible_rows))
    };

    BranchMenuLayout {
        rect: Rect::new(x, y, width, height),
        list,
        row_height,
        visible_rows,
        offset,
        message,
    }
}

fn build_diff_frame(
    diff: &DiffView,
    area: Rect,
    metrics: CellMetrics,
    theme: &Theme,
    quads: &mut Vec<Quad>,
) -> Frame {
    let cols = (area.width / metrics.width).floor().max(1.0) as usize;
    let rows = (area.height / metrics.height).floor().max(1.0) as usize;
    let mut frame_rows = Vec::with_capacity(rows);

    if diff.rows.is_empty() {
        let (text, color) = if diff.loading {
            ("Loading working diff…".to_owned(), theme.sidebar_text_dim)
        } else if let Some(error) = &diff.error {
            (error.replace('\n', " "), theme.diff_remove_text)
        } else {
            ("Sin cambios".to_owned(), theme.sidebar_text_dim)
        };
        let text: String = text.chars().take(cols).collect();
        let mut row = Row::default();
        if !text.is_empty() {
            row.runs.push(Run {
                range: 0..text.len(),
                color,
            });
            row.text = text;
        }
        frame_rows.push(row);
        while frame_rows.len() < rows {
            frame_rows.push(Row::default());
        }
        return Frame {
            rows: frame_rows,
            cols,
            cursor: None,
            background: theme.background,
        };
    }

    let start = diff.scroll_row.min(diff.rows.len().saturating_sub(rows));
    for index in 0..rows {
        let mut row = Row::default();
        if let Some(diff_row) = diff.rows.get(start + index) {
            let text: String = diff_row
                .text
                .chars()
                .skip(diff.scroll_col)
                .take(cols)
                .collect();
            let (background, color) = match diff_row.kind {
                DiffRowKind::File => (Some(theme.sidebar_active), theme.accent),
                DiffRowKind::Add => (Some(theme.diff_add_background), theme.diff_add_text),
                DiffRowKind::Remove => (Some(theme.diff_remove_background), theme.diff_remove_text),
                DiffRowKind::Context => (None, theme.foreground),
                DiffRowKind::Hunk => (None, theme.diff_hunk_text),
                DiffRowKind::Meta => (None, theme.diff_meta_text),
                DiffRowKind::Separator => (None, theme.background),
            };
            if let Some(background) = background {
                quads.push(Quad::new(
                    area.x,
                    area.y + index as f32 * metrics.height,
                    area.width,
                    metrics.height,
                    to_linear(background, 1.0),
                ));
            }
            if !text.is_empty() {
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

/// Background quads for the visible diff rows. Kept separate from the text
/// frame so a cached frame still gets its row backgrounds.
fn push_diff_background_quads(
    diff: &DiffView,
    area: Rect,
    metrics: CellMetrics,
    theme: &Theme,
    quads: &mut Vec<Quad>,
) {
    let rows = (area.height / metrics.height).floor().max(1.0) as usize;
    let start = diff.scroll_row.min(diff.rows.len().saturating_sub(rows));
    for index in 0..rows {
        let Some(row) = diff.rows.get(start + index) else {
            continue;
        };
        let background = match row.kind {
            DiffRowKind::File => Some(theme.sidebar_active),
            DiffRowKind::Add => Some(theme.diff_add_background),
            DiffRowKind::Remove => Some(theme.diff_remove_background),
            DiffRowKind::Context
            | DiffRowKind::Hunk
            | DiffRowKind::Meta
            | DiffRowKind::Separator => None,
        };
        if let Some(background) = background {
            quads.push(Quad::new(
                area.x,
                area.y + index as f32 * metrics.height,
                area.width,
                metrics.height,
                to_linear(background, 1.0),
            ));
        }
    }
}

struct BranchMenuStyle<'a> {
    scale: f32,
    ui_font_size: f32,
    theme: &'a Theme,
}

fn build_branch_menu(
    menu: &BranchMenu,
    anchor: Rect,
    outer: Rect,
    cursor: (f32, f32),
    style: BranchMenuStyle<'_>,
    quads: &mut Vec<Quad>,
    labels: &mut Vec<Label>,
) {
    let BranchMenuStyle {
        scale,
        ui_font_size,
        theme,
    } = style;
    let layout = branch_menu_layout(menu, anchor, outer, scale, ui_font_size);
    let rect = layout.rect;
    if rect.width <= 1.0 || rect.height <= 1.0 {
        return;
    }

    let border = (1.0 * scale).max(1.0);
    quads.push(Quad::rounded(
        rect.x - border,
        rect.y - border,
        rect.width + border * 2.0,
        rect.height + border * 2.0,
        8.0 * scale,
        to_linear(theme.border, 1.0),
    ));
    quads.push(Quad::rounded(
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        7.0 * scale,
        to_linear(theme.sidebar_background, 1.0),
    ));

    let tokens = ui::Tokens::new(ui_font_size, scale);
    let padding = (6.0 * scale).max(4.0);
    labels.push(Label {
        text: "Ramas".into(),
        x: rect.x + padding,
        y: rect.y + padding,
        width: (rect.width - padding * 2.0).max(1.0),
        font_size: tokens.caption,
        color: theme.sidebar_text_dim,
        bold: true,
    });

    let font = tokens.small;
    let line = font * 1.35;

    if menu.branches.is_empty() {
        let text = if menu.loading {
            "Cargando ramas…"
        } else {
            "Sin ramas locales"
        };
        labels.push(Label {
            text: text.into(),
            x: layout.list.x + 4.0 * scale,
            y: layout.list.y + (layout.row_height - line) / 2.0,
            width: (layout.list.width - 8.0 * scale).max(1.0),
            font_size: font,
            color: theme.sidebar_text_dim,
            bold: false,
        });
    } else {
        let text_x = layout.list.x + 16.0 * scale;
        let text_width = (layout.list.width - 20.0 * scale).max(1.0);
        for row in 0..layout.visible_rows {
            let index = layout.offset + row;
            let Some(branch) = menu.branches.get(index) else {
                break;
            };
            let row_rect = Rect::new(
                layout.list.x,
                layout.list.y + row as f32 * layout.row_height,
                layout.list.width,
                layout.row_height,
            );
            let selected = index == menu.selected;
            let hovered = row_rect.contains(cursor.0, cursor.1);
            if selected || hovered {
                quads.push(Quad::rounded(
                    row_rect.x + 2.0 * scale,
                    row_rect.y + 1.0 * scale,
                    (row_rect.width - 4.0 * scale).max(1.0),
                    (row_rect.height - 2.0 * scale).max(1.0),
                    4.0 * scale,
                    to_linear(
                        if selected {
                            theme.sidebar_active
                        } else {
                            theme.sidebar_hover
                        },
                        1.0,
                    ),
                ));
            }

            let dot = (3.5 * scale).max(2.0);
            if branch.current {
                quads.push(Quad::rounded(
                    layout.list.x + 6.0 * scale,
                    row_rect.y + (row_rect.height - dot) / 2.0,
                    dot,
                    dot,
                    dot / 2.0,
                    to_linear(theme.accent, 1.0),
                ));
            }

            labels.push(Label {
                text: branch.name.clone(),
                x: text_x,
                y: row_rect.y + (layout.row_height - line) / 2.0,
                width: text_width,
                font_size: font,
                color: if branch.current {
                    theme.accent
                } else if selected {
                    theme.sidebar_text
                } else {
                    theme.sidebar_text_dim
                },
                bold: branch.current || selected,
            });
        }
    }

    if let Some(message) = layout.message {
        let (text, color) = if let Some(error) = &menu.error {
            (error.as_str(), theme.diff_remove_text)
        } else if let Some(busy) = &menu.busy {
            (busy.as_str(), theme.sidebar_text_dim)
        } else {
            ("", theme.sidebar_text_dim)
        };
        labels.push(Label {
            text: text.to_owned(),
            x: message.x,
            y: message.y + (message.height - line) / 2.0,
            width: message.width,
            font_size: font,
            color,
            bold: false,
        });
    }
}

struct PaneFooterStyle<'a> {
    scale: f32,
    ui_font_size: f32,
    theme: &'a Theme,
}

fn build_pane_footer(
    pane: &Pane,
    footer: Rect,
    style: PaneFooterStyle<'_>,
    renderer: &mut Renderer,
    quads: &mut Vec<Quad>,
    labels: &mut Vec<Label>,
) -> FooterHits {
    let PaneFooterStyle {
        scale,
        ui_font_size,
        theme,
    } = style;

    let mut hits = FooterHits::default();

    if footer.width <= 1.0 || footer.height <= 1.0 {
        return hits;
    }

    let Some(status) = &pane.git_status else {
        return hits;
    };
    if footer.height < 6.0 * scale {
        return hits;
    }

    // Keep the usual footer size at the bottom. The strip above it belongs to
    // the pane background (already painted with the TUI color), so it no
    // longer shows as an app-colored margin.
    let bottom_margin = (6.0 * scale).max(2.0);
    let available = (footer.height - bottom_margin).max(1.0);
    let block_height = (22.0 * scale).min(available);
    let font = (ui_font_size * 0.9)
        .max(11.0 * scale)
        .min(block_height * 0.7)
        .max(8.0 * scale);
    let line = (font * 1.3).min(block_height);
    let pill_height = (block_height - 3.0 * scale)
        .clamp(18.0 * scale, 40.0 * scale)
        .min(block_height);
    let radius = pill_height / 2.0;
    let padding = 10.0 * scale;
    let gap = 7.0 * scale;
    let edge = 5.0 * scale;
    let border = (1.0 * scale).max(1.0);
    let y = footer.bottom() - bottom_margin - pill_height;
    let text_y = y + (pill_height - line) / 2.0;

    let branch_icon = (font * 0.95).max(10.0 * scale);
    let icon_gap = 6.0 * scale;

    let additions = format!("+{}", status.additions);
    let deletions = format!("-{}", status.deletions);
    let additions_width = renderer.measure_ui_text(&additions, font, true);
    let deletions_width = renderer.measure_ui_text(&deletions, font, true);
    let changes_icon = (font * 0.85).max(9.0 * scale);
    let changes_width =
        padding * 2.0 + changes_icon + icon_gap + additions_width + gap + deletions_width;

    let can_show_changes = footer.width >= changes_width + edge * 2.0 + 60.0 * scale;
    let branch_max = if can_show_changes {
        footer.width - edge * 2.0 - changes_width - gap
    } else {
        footer.width - edge * 2.0
    }
    .max(0.0);

    let branch_text_max = (branch_max - padding * 2.0 - branch_icon - icon_gap).max(0.0);
    let branch = truncate_to_width(&status.branch, branch_text_max, font, true, renderer);

    let mut branch_width = 0.0;
    if !branch.is_empty() && branch_max > branch_icon + icon_gap + padding {
        let branch_text_width = renderer.measure_ui_text(&branch, font, true);
        let width = (branch_text_width + padding * 2.0 + branch_icon + icon_gap).min(branch_max);
        let x = footer.x + edge;
        quads.push(Quad::rounded(
            x - border,
            y - border,
            width + border * 2.0,
            pill_height + border * 2.0,
            radius + border,
            to_linear(theme.border, 1.0),
        ));
        quads.push(Quad::rounded(
            x,
            y,
            width,
            pill_height,
            radius,
            to_linear(theme.sidebar_active, 1.0),
        ));
        push_branch_icon(
            quads,
            x + padding,
            y + (pill_height - branch_icon) / 2.0,
            branch_icon,
            scale,
            theme.accent,
        );
        labels.push(Label {
            text: branch,
            x: x + padding + branch_icon + icon_gap,
            y: text_y,
            width: (width - padding * 2.0 - branch_icon - icon_gap).max(1.0),
            font_size: font,
            color: theme.sidebar_text,
            bold: true,
        });
        branch_width = width;
        hits.branch = Some(Rect::new(x, y, width, pill_height));
    }

    if !can_show_changes {
        return hits;
    }

    let changes_x = footer.x
        + edge
        + if branch_width > 0.0 {
            branch_width + gap
        } else {
            0.0
        };
    hits.changes = Some(Rect::new(changes_x, y, changes_width, pill_height));
    quads.push(Quad::rounded(
        changes_x - border,
        y - border,
        changes_width + border * 2.0,
        pill_height + border * 2.0,
        radius + border,
        to_linear(theme.border, 1.0),
    ));
    quads.push(Quad::rounded(
        changes_x,
        y,
        changes_width,
        pill_height,
        radius,
        to_linear(theme.sidebar_active, 1.0),
    ));

    let additions_background = if status.additions > 0 {
        theme.diff_add_background
    } else {
        theme.sidebar_hover
    };
    let deletions_background = if status.deletions > 0 {
        theme.diff_remove_background
    } else {
        theme.sidebar_hover
    };
    let green_width = padding + changes_icon + icon_gap + additions_width + gap * 0.5;
    let red_width = (changes_width - green_width).max(0.0);
    quads.push(Quad::rounded_corners(
        changes_x,
        y,
        green_width,
        pill_height,
        [radius, 0.0, 0.0, radius],
        to_linear(additions_background, 1.0),
    ));
    quads.push(Quad::rounded_corners(
        changes_x + green_width,
        y,
        red_width,
        pill_height,
        [0.0, radius, radius, 0.0],
        to_linear(deletions_background, 1.0),
    ));

    push_diff_icon(
        &mut *quads,
        changes_x + padding,
        y + (pill_height - changes_icon) / 2.0,
        changes_icon,
        scale,
        theme.sidebar_text,
    );

    let additions_color = if status.additions > 0 {
        theme.diff_add_text
    } else {
        theme.sidebar_text_dim
    };
    let additions_x = changes_x + padding + changes_icon + icon_gap;
    labels.push(Label {
        text: additions,
        x: additions_x,
        y: text_y,
        width: (additions_width + 2.0 * scale).max(1.0),
        font_size: font,
        color: additions_color,
        bold: true,
    });

    let deletions_color = if status.deletions > 0 {
        theme.diff_remove_text
    } else {
        theme.sidebar_text_dim
    };
    labels.push(Label {
        text: deletions,
        x: additions_x + additions_width + gap,
        y: text_y,
        width: (deletions_width + 2.0 * scale).max(1.0),
        font_size: font,
        color: deletions_color,
        bold: true,
    });

    hits
}

fn truncate_to_width(
    text: &str,
    max_width: f32,
    font_size: f32,
    bold: bool,
    renderer: &mut Renderer,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if renderer.measure_ui_text(text, font_size, bold) <= max_width {
        return text.to_owned();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut low = 0usize;
    let mut high = chars.len();
    let mut best = String::new();
    while low < high {
        let middle = (low + high) / 2;
        let mut candidate: String = chars[..middle].iter().collect();
        candidate.push('…');
        if renderer.measure_ui_text(&candidate, font_size, bold) <= max_width {
            best = candidate;
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    best
}

fn push_branch_icon(quads: &mut Vec<Quad>, x: f32, y: f32, size: f32, scale: f32, color: Rgb) {
    let color = to_linear(color, 1.0);
    let dot = (size * 0.28).max(2.0 * scale);
    let bar = (size * 0.12).max(1.0 * scale);
    let right = x + size - dot;
    let bottom = y + size - dot;

    quads.push(Quad::rounded(x, y, dot, dot, dot / 2.0, color));
    quads.push(Quad::rounded(x, bottom, dot, dot, dot / 2.0, color));
    quads.push(Quad::rounded(
        x + dot / 2.0 - bar / 2.0,
        y + dot / 2.0,
        bar,
        size - dot,
        bar / 2.0,
        color,
    ));

    let branch_y = y + size * 0.28;
    quads.push(Quad::rounded(right, branch_y, dot, dot, dot / 2.0, color));
    quads.push(Quad::rounded(
        x + dot / 2.0,
        branch_y + dot / 2.0 - bar / 2.0,
        size - dot,
        bar,
        bar / 2.0,
        color,
    ));
}

fn push_diff_icon(quads: &mut Vec<Quad>, x: f32, y: f32, size: f32, scale: f32, color: Rgb) {
    let color = to_linear(color, 1.0);
    let width = size * 0.68;
    let thickness = (1.2 * scale).max(1.0);

    quads.push(Quad::new(x, y, width, thickness, color));
    quads.push(Quad::new(x, y + size - thickness, width, thickness, color));
    quads.push(Quad::new(x, y, thickness, size, color));
    quads.push(Quad::new(x + width - thickness, y, thickness, size, color));
    quads.push(Quad::new(
        x + width * 0.22,
        y + size * 0.34,
        width * 0.5,
        thickness,
        color,
    ));
    quads.push(Quad::new(
        x + width * 0.22,
        y + size * 0.62,
        width * 0.34,
        thickness,
        color,
    ));
}

fn agent_state_color(theme: &Theme, state: &AgentState) -> Rgb {
    match state {
        AgentState::Working => theme.ansi[6],
        AgentState::Blocked(_) | AgentState::Attention => theme.ansi[3],
        AgentState::Error => theme.ansi[1],
        AgentState::Done => theme.ansi[2],
        AgentState::Idle | AgentState::Detected => theme.sidebar_text_dim,
    }
}

fn push_agent_dot(quads: &mut Vec<Quad>, x: f32, center_y: f32, scale: f32, color: Rgb) {
    let size = (6.0 * scale).max(4.0);
    quads.push(Quad::rounded(
        x,
        center_y - size / 2.0,
        size,
        size,
        size / 2.0,
        to_linear(color, 1.0),
    ));
}

fn push_close_icon(quads: &mut Vec<Quad>, rect: Rect, scale: f32, color: Rgb) {
    let extent = (rect.width.min(rect.height) * 0.5).max(4.0 * scale);
    let thickness = (1.6 * scale).max(1.0);
    let left = rect.x + (rect.width - extent) / 2.0;
    let top = rect.y + (rect.height - extent) / 2.0;
    let travel = (extent - thickness).max(0.0);
    let steps = 5;

    for index in 0..steps {
        let t = index as f32 / (steps - 1) as f32;
        let x = left + t * travel;
        let y = top + t * travel;
        quads.push(Quad::rounded(
            x,
            y,
            thickness,
            thickness,
            thickness / 2.0,
            to_linear(color, 1.0),
        ));
        let x = left + (1.0 - t) * travel;
        quads.push(Quad::rounded(
            x,
            y,
            thickness,
            thickness,
            thickness / 2.0,
            to_linear(color, 1.0),
        ));
    }
}

fn push_cursor_quads(
    quads: &mut Vec<Quad>,
    content: Rect,
    frame: &Frame,
    metrics: CellMetrics,
    theme: &Theme,
) {
    let Some(cursor) = frame.cursor else {
        return;
    };

    let x = content.x + cursor.col as f32 * metrics.width;
    let y = content.y + cursor.row as f32 * metrics.height;

    match cursor.shape {
        CursorShapeKind::Block => quads.push(Quad::new(
            x,
            y,
            metrics.width,
            metrics.height,
            to_linear(theme.cursor, 0.5),
        )),
        CursorShapeKind::Beam => quads.push(Quad::new(
            x,
            y,
            2.0,
            metrics.height,
            to_linear(theme.cursor, 0.9),
        )),
        CursorShapeKind::Underline => quads.push(Quad::new(
            x,
            y + metrics.height - 2.0,
            metrics.width,
            2.0,
            to_linear(theme.cursor, 0.9),
        )),
    }
}

const HIGHLIGHT_LIMIT: usize = 512 * 1024;

struct PopupLayout {
    rect: Rect,
    first: usize,
    rows: usize,
    row_height: f32,
    padding: f32,
}

#[derive(Clone, Copy)]
struct PopupGeometry {
    content: Rect,
    row: usize,
    col: usize,
    selected: usize,
    metrics: CellMetrics,
    ui_font_size: f32,
    scale: f32,
}

fn popup_layout(geometry: PopupGeometry, candidates: &[complete::Candidate]) -> PopupLayout {
    let PopupGeometry {
        content,
        row,
        col,
        selected,
        metrics,
        ui_font_size,
        scale,
    } = geometry;
    let padding = 6.0 * scale;
    if candidates.is_empty() {
        return PopupLayout {
            rect: Rect::default(),
            first: 0,
            rows: 0,
            row_height: 1.0,
            padding,
        };
    }
    let row_height = (ui_font_size * 1.65).max(metrics.height * 0.9);
    let max_chars = candidates
        .iter()
        .map(|candidate| candidate.label.chars().count())
        .max()
        .unwrap_or(12)
        .min(72);
    let max_width = (content.width - 8.0 * scale).max(80.0);
    let width = ((max_chars as f32 * ui_font_size * 0.58) + 150.0 * scale)
        .max(180.0 * scale)
        .min(max_width);
    let max_rows = (((content.height - 18.0 * scale) / row_height).floor() as usize).max(1);
    let rows = candidates.len().min(8).min(max_rows).max(1);
    let selected = selected.min(candidates.len().saturating_sub(1));
    let first = if selected >= rows {
        (selected + 1 - rows).min(candidates.len() - rows)
    } else {
        0
    };
    let height = rows as f32 * row_height + padding * 2.0;

    let anchor_x = content.x + col as f32 * metrics.width;
    let x = anchor_x
        .min(content.right() - width - 3.0 * scale)
        .max(content.x + 3.0 * scale);
    let below = content.y + (row + 1) as f32 * metrics.height + 4.0 * scale;
    let y = if below + height <= content.bottom() - 3.0 * scale {
        below
    } else {
        (content.y + row as f32 * metrics.height - height - 4.0 * scale)
            .max(content.y + 3.0 * scale)
    };

    PopupLayout {
        rect: Rect::new(x, y, width, height),
        first,
        rows,
        row_height,
        padding,
    }
}

/// Replace part of the rendered terminal rows with spaces so the popup is not
/// drawn over terminal glyphs. Runs keep their byte ranges because only ASCII
/// characters are replaced; rows with wide or non-ASCII glyphs are cleared
/// entirely as a safe fallback.
fn collect_prompt_trims(frame: &Frame, enabled: bool) -> Vec<PromptTrim> {
    if !enabled {
        return Vec::new();
    }
    frame
        .rows
        .iter()
        .enumerate()
        .filter_map(|(row, line)| {
            complete::line::prompt_user_prefix_len(&line.text, frame.cols)
                .map(|columns| PromptTrim { row, columns })
        })
        .collect()
}

fn trim_prompt_row(frame: &mut Frame, row_index: usize, columns: usize) {
    let cols = frame.cols;
    let cursor_row = frame.cursor.map(|cursor| cursor.row);
    let Some(row) = frame.rows.get_mut(row_index) else {
        return;
    };
    let char_count = row.text.chars().count();
    if columns == 0 || columns >= char_count {
        return;
    }

    let prefix_bytes: usize = row.text.chars().take(columns).map(char::len_utf8).sum();
    let mut text: String = row.text.chars().skip(columns).collect();
    while text.chars().count() < cols {
        text.push(' ');
    }
    row.text = text;

    for run in &mut row.runs {
        run.range = run.range.start.saturating_sub(prefix_bytes)
            ..run.range.end.saturating_sub(prefix_bytes);
    }
    row.runs.retain(|run| !run.range.is_empty());

    let mut backgrounds = Vec::with_capacity(row.backgrounds.len());
    for background in &row.backgrounds {
        let start = background.start.saturating_sub(columns);
        let end = background.end.saturating_sub(columns);
        if end > start {
            backgrounds.push(Background {
                start,
                end,
                color: background.color,
            });
        }
    }
    row.backgrounds = backgrounds;

    if cursor_row == Some(row_index)
        && let Some(cursor) = &mut frame.cursor
    {
        cursor.col = cursor.col.saturating_sub(columns);
    }
}

fn blank_region(
    frame: &mut Frame,
    rows: std::ops::Range<usize>,
    cols: std::ops::Range<usize>,
    theme: &Theme,
) {
    if rows.is_empty() || cols.is_empty() {
        return;
    }
    let col_start = cols.start;
    let col_end = cols.end;

    for row in frame.rows.iter_mut().take(rows.end).skip(rows.start) {
        let mut chars: Vec<char> = row.text.chars().collect();
        if chars.len() != frame.cols {
            clear_row(row, frame.cols, theme);
            continue;
        }
        if (col_start..col_end).any(|column| {
            chars
                .get(column)
                .is_some_and(|character| !character.is_ascii())
        }) {
            clear_row(row, frame.cols, theme);
            continue;
        }

        let mut changed = false;
        for character in chars.iter_mut().take(col_end).skip(col_start) {
            if *character != ' ' {
                *character = ' ';
                changed = true;
            }
        }
        if changed {
            row.text = chars.into_iter().collect();
        }
    }
}

fn clear_row(row: &mut Row, cols: usize, theme: &Theme) {
    row.text = " ".repeat(cols);
    row.runs = vec![Run {
        range: 0..cols,
        color: theme.background,
    }];
}

fn build_editor_frame(editor: &Editor, text: Rect, metrics: CellMetrics, theme: &Theme) -> Frame {
    let cols = (text.width / metrics.width).floor().max(1.0) as usize;
    let rows = (text.height / metrics.height).floor().max(1.0) as usize;

    let mut frame_rows = Vec::with_capacity(rows);
    for index in 0..rows {
        let mut row = Row::default();
        let line_index = editor.scroll_row + index;
        if let Some(line) = editor.lines.get(line_index) {
            let start_byte = Editor::char_to_byte(line, editor.scroll_col);
            let visible = &line[start_byte..];
            let text: String = visible.chars().take(cols).collect();
            let end_byte = start_byte + text.len();

            if !text.is_empty() {
                if let Some(spans) = editor.highlight.get(line_index) {
                    for span in spans {
                        let start = span.range.start.max(start_byte);
                        let end = span.range.end.min(end_byte);
                        if start < end {
                            row.runs.push(Run {
                                range: (start - start_byte)..(end - start_byte),
                                color: span.color,
                            });
                        }
                    }
                }

                if row.runs.is_empty() {
                    row.runs.push(Run {
                        range: 0..text.len(),
                        color: theme.foreground,
                    });
                }

                row.text = text;
            }
        }
        frame_rows.push(row);
    }

    let cursor = if editor.cursor_row >= editor.scroll_row
        && editor.cursor_row < editor.scroll_row + rows
        && editor.cursor_col >= editor.scroll_col
        && editor.cursor_col < editor.scroll_col + cols
    {
        Some(Cursor {
            row: editor.cursor_row - editor.scroll_row,
            col: editor.cursor_col - editor.scroll_col,
            shape: CursorShapeKind::Beam,
        })
    } else {
        None
    };

    Frame {
        rows: frame_rows,
        cols,
        cursor,
        background: theme.background,
    }
}

/// Scrollback history budget constants and planning.
const MIN_SCROLLBACK_LINES: usize = 1_000;
const SCROLLBACK_CELL_BYTES: usize = 32;

#[derive(Clone, Copy, Debug)]
struct ScrollbackPane {
    id: PaneId,
    cols: usize,
    active: bool,
    focused: bool,
}

fn estimate_scrollback_bytes(cols: usize, lines: usize) -> usize {
    cols.saturating_mul(lines)
        .saturating_mul(SCROLLBACK_CELL_BYTES)
}

fn plan_scrollback_limits(panes: &[ScrollbackPane], config: &Config) -> HashMap<PaneId, usize> {
    let mut limits: HashMap<PaneId, usize> = panes
        .iter()
        .map(|pane| {
            let limit = if pane.active {
                config.scrollback_lines
            } else {
                config.hidden_scrollback_lines
            };
            (pane.id, limit)
        })
        .collect();

    if config.scrollback_budget_mb == 0 {
        return limits;
    }

    let budget = (config.scrollback_budget_mb as usize).saturating_mul(1024 * 1024);
    let estimate = |limits: &HashMap<PaneId, usize>| {
        panes.iter().fold(0usize, |total, pane| {
            let limit = limits.get(&pane.id).copied().unwrap_or(0);
            total.saturating_add(estimate_scrollback_bytes(pane.cols, limit))
        })
    };
    let mut total = estimate(&limits);
    if total <= budget {
        return limits;
    }

    // Hidden sessions yield their history first, then visible panes, then the
    // focused pane. Every pane keeps at least MIN_SCROLLBACK_LINES.
    for priority in 0..=2u8 {
        for pane in panes {
            if total <= budget {
                break;
            }
            let pane_priority = match (pane.active, pane.focused) {
                (false, _) => 0,
                (true, false) => 1,
                (true, true) => 2,
            };
            if pane_priority != priority {
                continue;
            }

            let current = limits.get(&pane.id).copied().unwrap_or(0);
            if current <= MIN_SCROLLBACK_LINES {
                continue;
            }

            let bytes_per_line = pane.cols.saturating_mul(SCROLLBACK_CELL_BYTES).max(1);
            let excess = total - budget;
            let reduce = excess
                .div_ceil(bytes_per_line)
                .max(1)
                .min(current - MIN_SCROLLBACK_LINES);
            let target = current - reduce;
            limits.insert(pane.id, target);
            total = total.saturating_sub((current - target).saturating_mul(bytes_per_line));
        }
    }

    limits
}

fn arrow_direction(code: KeyCode) -> Option<FocusDirection> {
    match code {
        KeyCode::ArrowLeft => Some(FocusDirection::Left),
        KeyCode::ArrowRight => Some(FocusDirection::Right),
        KeyCode::ArrowUp => Some(FocusDirection::Up),
        KeyCode::ArrowDown => Some(FocusDirection::Down),
        _ => None,
    }
}

fn digit_index(code: KeyCode) -> Option<usize> {
    match code {
        KeyCode::Digit1 => Some(0),
        KeyCode::Digit2 => Some(1),
        KeyCode::Digit3 => Some(2),
        KeyCode::Digit4 => Some(3),
        KeyCode::Digit5 => Some(4),
        KeyCode::Digit6 => Some(5),
        KeyCode::Digit7 => Some(6),
        KeyCode::Digit8 => Some(7),
        KeyCode::Digit9 => Some(8),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{DiffLineKind, FileStatus};

    fn candidate(label: &str) -> complete::Candidate {
        complete::Candidate {
            append: label.to_owned(),
            label: label.to_owned(),
            kind: complete::Kind::Path,
            detail: "ruta".into(),
            score: 1,
        }
    }

    #[test]
    fn reads_scrollbacks_in_parallel() {
        let dir = std::env::temp_dir().join(format!(
            "f-terminal-parallel-scrollback-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.ansi");
        let second = dir.join("second.ansi");
        std::fs::write(&first, b"first history").unwrap();
        std::fs::write(&second, b"second history").unwrap();

        let scrollbacks = read_scrollbacks(vec![(PaneId(1), first), (PaneId(2), second)]);
        assert_eq!(
            scrollbacks.get(&PaneId(1)).map(|bytes| bytes.as_slice()),
            Some(b"first history".as_slice())
        );
        assert_eq!(
            scrollbacks.get(&PaneId(2)).map(|bytes| bytes.as_slice()),
            Some(b"second history".as_slice())
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restores_tabs_panes_and_session_keys() {
        let shells = ShellCatalog {
            profiles: vec![TerminalProfile {
                id: "test".into(),
                name: "Test".into(),
                short: "T".into(),
                program: PathBuf::from("sh"),
                args: Vec::new(),
                env: Vec::new(),
                cwd_mode: crate::shell::CwdMode::Native,
            }],
            default_id: "test".into(),
        };
        let cwd = std::env::temp_dir().to_string_lossy().into_owned();
        let session = SessionState {
            version: session::SESSION_VERSION,
            window: None,
            active_tab: 1,
            tabs: vec![
                SavedTab {
                    title: Some("first".into()),
                    focused: 1,
                    layout: SavedLayout::Split {
                        direction: SavedDirection::Right,
                        ratio: 0.5,
                        first: Box::new(SavedLayout::Leaf { index: 0 }),
                        second: Box::new(SavedLayout::Leaf { index: 1 }),
                    },
                    panes: vec![
                        SavedPane {
                            key: 1,
                            profile_id: "test".into(),
                            cwd: Some(cwd.clone()),
                        },
                        SavedPane {
                            key: 2,
                            profile_id: "missing".into(),
                            cwd: Some(cwd.clone()),
                        },
                    ],
                },
                SavedTab {
                    title: Some("second".into()),
                    focused: 0,
                    layout: SavedLayout::Leaf { index: 0 },
                    panes: vec![SavedPane {
                        key: 3,
                        profile_id: "test".into(),
                        cwd: Some(cwd),
                    }],
                },
            ],
            collapsed_tabs: vec![0],
        };

        let mut workspace = Workspace::new();
        let restored = restore_workspace(
            &mut workspace,
            &shells,
            Some(&session),
            Path::new("."),
            false,
        );

        assert_eq!(workspace.tabs.len(), 2);
        assert_eq!(workspace.active_index(), 1);
        assert_eq!(restored.next_key, 4);
        assert_eq!(restored.pending.len(), 3);
        let mut keys: Vec<u64> = restored
            .pending
            .values()
            .map(|spawn| spawn.session_key)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec![1, 2, 3]);
        assert_eq!(
            restored
                .pending
                .values()
                .find(|spawn| spawn.session_key == 2)
                .map(|spawn| spawn.profile_id.as_str()),
            Some("test")
        );
        assert!(restored.collapsed.contains(&workspace.tabs[0].id));
        assert_eq!(workspace.tabs[0].leaves().len(), 2);
    }

    #[test]
    fn path_titles_fall_back_to_the_default_label() {
        for title in [
            r"C:\Users\me",
            "c:/Users/me/project",
            r"\\server\share",
            r"\Windows\System32",
            "/home/me/project",
            "~/projects",
            "~",
        ] {
            assert_eq!(clean_pty_title(title, true), None, "{title}");
        }

        assert_eq!(
            clean_pty_title("Nueva sesión", true).as_deref(),
            Some("Nueva sesión")
        );
        assert_eq!(
            clean_pty_title("  vim README.md  ", true).as_deref(),
            Some("vim README.md")
        );
        assert_eq!(
            clean_pty_title("C: drive", true).as_deref(),
            Some("C: drive")
        );
        assert_eq!(clean_pty_title("", true), None);
    }

    #[test]
    fn path_titles_are_dropped_after_hiding_the_user_prefix() {
        assert_eq!(clean_pty_title("me@host: /home/me/project", false), None);
        assert_eq!(
            clean_pty_title("me@host: opencode", false).as_deref(),
            Some("opencode")
        );
        assert_eq!(clean_pty_title("me@host /home/me", false), None);
    }

    #[test]
    fn popup_keeps_the_selection_visible_and_inside_the_pane() {
        let candidates: Vec<_> = (0..20)
            .map(|index| candidate(&format!("item-{index}")))
            .collect();
        let content = Rect::new(10.0, 20.0, 500.0, 300.0);
        let layout = popup_layout(
            PopupGeometry {
                content,
                row: 0,
                col: 2,
                selected: 10,
                metrics: CellMetrics {
                    width: 8.0,
                    height: 18.0,
                },
                ui_font_size: 14.0,
                scale: 1.0,
            },
            &candidates,
        );
        assert_eq!(layout.rows, 8);
        assert_eq!(layout.first, 3);
        assert!(layout.rect.x >= content.x);
        assert!(layout.rect.right() <= content.right());
        assert!(layout.rect.y >= content.y);
        assert!(layout.rect.bottom() <= content.bottom());
    }

    #[test]
    fn popup_moves_above_when_there_is_no_room_below() {
        let candidates = vec![candidate("one"), candidate("two")];
        let content = Rect::new(0.0, 0.0, 400.0, 100.0);
        let layout = popup_layout(
            PopupGeometry {
                content,
                row: 4,
                col: 0,
                selected: 0,
                metrics: CellMetrics {
                    width: 8.0,
                    height: 18.0,
                },
                ui_font_size: 14.0,
                scale: 1.0,
            },
            &candidates,
        );
        assert!(layout.rect.bottom() <= content.bottom());
        assert!(layout.rect.y >= content.y);
    }

    #[test]
    fn dominant_background_uses_the_most_common_color() {
        let theme = Theme::tokyo_night();
        let black = Rgb { r: 0, g: 0, b: 0 };
        let row = || Row {
            backgrounds: vec![crate::term::frame::Background {
                start: 0,
                end: 2,
                color: black,
            }],
            ..Row::default()
        };
        let frame = Frame {
            rows: vec![row(), row()],
            cols: 2,
            cursor: None,
            background: theme.background,
        };
        assert_eq!(dominant_background(&frame), black);

        let plain = Frame {
            rows: vec![Row::default()],
            cols: 2,
            cursor: None,
            background: theme.background,
        };
        assert_eq!(dominant_background(&plain), theme.background);
    }

    #[test]
    fn branch_menu_opens_above_the_footer_and_scrolls_to_the_selection() {
        let branches: Vec<BranchInfo> = (0..12)
            .map(|index| BranchInfo {
                name: format!("branch-{index}"),
                full: format!("refs/heads/branch-{index}"),
                hash: "abc123".into(),
                upstream: None,
                current: index == 0,
            })
            .collect();
        let mut menu = BranchMenu {
            pane: PaneId(1),
            cwd: PathBuf::new(),
            branches,
            selected: 0,
            loading: false,
            error: None,
            busy: None,
        };
        let anchor = Rect::new(100.0, 700.0, 80.0, 36.0);
        let outer = Rect::new(0.0, 0.0, 800.0, 800.0);
        let layout = branch_menu_layout(&menu, anchor, outer, 2.0, 25.0);

        assert_eq!(layout.visible_rows, 10);
        assert!(layout.rect.bottom() <= anchor.y);
        assert!(layout.rect.x >= outer.x && layout.rect.right() <= outer.right());
        assert_eq!(layout.offset, 0);

        menu.selected = 11;
        let layout = branch_menu_layout(&menu, anchor, outer, 2.0, 25.0);
        assert_eq!(layout.offset, 2);
    }

    #[test]
    fn branch_menu_renders_branches_and_messages() {
        let branches: Vec<BranchInfo> = (0..3)
            .map(|index| BranchInfo {
                name: format!("branch-{index}"),
                full: format!("refs/heads/branch-{index}"),
                hash: "abc123".into(),
                upstream: None,
                current: index == 0,
            })
            .collect();
        let mut menu = BranchMenu {
            pane: PaneId(1),
            cwd: PathBuf::new(),
            branches,
            selected: 1,
            loading: false,
            error: Some("falló el checkout".into()),
            busy: None,
        };
        let theme = Theme::tokyo_night();
        let anchor = Rect::new(100.0, 700.0, 80.0, 36.0);
        let outer = Rect::new(0.0, 0.0, 800.0, 800.0);
        let mut quads = Vec::new();
        let mut labels = Vec::new();

        build_branch_menu(
            &menu,
            anchor,
            outer,
            (150.0, 600.0),
            BranchMenuStyle {
                scale: 2.0,
                ui_font_size: 25.0,
                theme: &theme,
            },
            &mut quads,
            &mut labels,
        );

        assert!(!quads.is_empty());
        assert!(labels.iter().any(|label| label.text == "Ramas"));
        assert!(labels.iter().any(|label| label.text == "branch-0"));
        assert!(labels.iter().any(|label| label.text == "falló el checkout"));

        menu.error = None;
        menu.busy = Some("Cambiando a branch-1…".into());
        labels.clear();
        build_branch_menu(
            &menu,
            anchor,
            outer,
            (150.0, 600.0),
            BranchMenuStyle {
                scale: 2.0,
                ui_font_size: 25.0,
                theme: &theme,
            },
            &mut quads,
            &mut labels,
        );
        assert!(
            labels
                .iter()
                .any(|label| label.text == "Cambiando a branch-1…")
        );
    }

    #[test]
    fn diff_rows_format_files_and_line_numbers() {
        let detail = ReviewDetail {
            summary: ReviewSummary {
                title: "Cambios locales".to_owned(),
                subtitle: String::new(),
                kind: ReviewKind::Working,
            },
            files: vec![crate::review::FileDiff {
                path: "src/lib.rs".to_owned(),
                old_path: None,
                status: FileStatus::Modified,
                additions: 1,
                deletions: 1,
                binary: false,
                lines: vec![
                    crate::review::DiffLine {
                        kind: DiffLineKind::Context,
                        text: "fn main() {".to_owned(),
                        old: Some(1),
                        new: Some(1),
                    },
                    crate::review::DiffLine {
                        kind: DiffLineKind::Add,
                        text: "    let x = 1;".to_owned(),
                        old: None,
                        new: Some(2),
                    },
                    crate::review::DiffLine {
                        kind: DiffLineKind::Remove,
                        text: "    let y = 2;".to_owned(),
                        old: Some(2),
                        new: None,
                    },
                ],
            }],
            additions: 1,
            deletions: 1,
        };

        let rows = build_diff_rows(&detail);
        assert_eq!(rows[0].kind, DiffRowKind::File);
        assert!(rows[0].text.contains("src/lib.rs"));
        assert!(rows[0].text.contains("+1 -1"));
        assert_eq!(rows[1].kind, DiffRowKind::Context);
        assert!(rows[1].text.contains("fn main()"));
        assert_eq!(rows[2].kind, DiffRowKind::Add);
        assert!(rows[2].text.contains("+ "));
        assert_eq!(rows[3].kind, DiffRowKind::Remove);
        assert!(rows[3].text.contains("- "));

        let diff = DiffView {
            pane: PaneId(1),
            cwd: PathBuf::new(),
            generation: 1,
            loading: false,
            error: None,
            detail: Some(detail),
            rows,
            max_cols: 80,
            scroll_row: 0,
            scroll_col: 0,
        };
        let theme = Theme::tokyo_night();
        let metrics = CellMetrics {
            width: 8.0,
            height: 16.0,
        };
        let mut quads = Vec::new();
        let frame = build_diff_frame(
            &diff,
            Rect::new(0.0, 0.0, 400.0, 160.0),
            metrics,
            &theme,
            &mut quads,
        );
        assert_eq!(frame.rows.len(), 10);
        assert!(!quads.is_empty());
    }

    #[test]
    fn collects_and_trims_every_visible_prompt_row() {
        let frame = Frame {
            rows: vec![
                Row {
                    text: "user@host ~/repo % ls".into(),
                    ..Row::default()
                },
                Row {
                    text: "README.md".into(),
                    ..Row::default()
                },
                Row {
                    text: "user@host ~/repo % ".into(),
                    ..Row::default()
                },
            ],
            cols: 30,
            cursor: None,
            background: Theme::github_dark().background,
        };
        let trims = collect_prompt_trims(&frame, true);
        assert_eq!(trims.len(), 2);
        assert_eq!(trims[0].row, 0);
        assert_eq!(trims[1].row, 2);
        assert!(collect_prompt_trims(&frame, false).is_empty());
    }

    #[test]
    fn trimming_prompt_user_keeps_the_rest_of_the_line() {
        let text = "user@host dir % ls";
        let width = text.chars().count();
        let mut frame = Frame {
            rows: vec![Row {
                text: text.into(),
                runs: vec![Run {
                    range: 0..text.len(),
                    color: Theme::tokyo_night().foreground,
                }],
                backgrounds: vec![Background {
                    start: 0,
                    end: width,
                    color: Theme::tokyo_night().sidebar_active,
                }],
            }],
            cols: width,
            cursor: Some(Cursor {
                row: 0,
                col: width - 1,
                shape: CursorShapeKind::Beam,
            }),
            background: Theme::tokyo_night().background,
        };

        let shift = complete::line::user_host_prefix_len(text).unwrap();
        trim_prompt_row(&mut frame, 0, shift);

        assert_eq!(frame.rows[0].text.trim_end(), "dir % ls");
        assert_eq!(frame.cursor.unwrap().col, width - 1 - shift);
        assert_eq!(frame.rows[0].backgrounds[0].start, 0);
        assert_eq!(frame.rows[0].backgrounds[0].end, width - shift);
        assert_eq!(frame.rows[0].runs[0].range, 0..text.len() - shift);
    }

    #[test]
    fn blanking_hides_cells_without_touching_the_runs() {
        let mut frame = Frame {
            rows: vec![Row {
                text: "hello world".into(),
                runs: vec![Run {
                    range: 0..11,
                    color: Theme::tokyo_night().foreground,
                }],
                backgrounds: Vec::new(),
            }],
            cols: 11,
            cursor: None,
            background: Theme::tokyo_night().background,
        };
        blank_region(&mut frame, 0..1, 2..5, &Theme::tokyo_night());
        assert_eq!(frame.rows[0].text, "he    world");
        assert_eq!(frame.rows[0].runs[0].range, 0..11);
    }

    #[test]
    fn blanking_clears_wide_rows_instead_of_breaking_runs() {
        let mut frame = Frame {
            rows: vec![Row {
                text: "héllo".into(),
                runs: vec![Run {
                    range: 0..6,
                    color: Theme::tokyo_night().foreground,
                }],
                backgrounds: Vec::new(),
            }],
            cols: 5,
            cursor: None,
            background: Theme::tokyo_night().background,
        };
        blank_region(&mut frame, 0..1, 1..2, &Theme::tokyo_night());
        assert_eq!(frame.rows[0].text, "     ");
        assert_eq!(frame.rows[0].runs[0].range, 0..5);
    }

    fn pane(id: u64, cols: usize, active: bool, focused: bool) -> ScrollbackPane {
        ScrollbackPane {
            id: PaneId(id),
            cols,
            active,
            focused,
        }
    }

    #[test]
    fn active_and_hidden_sessions_get_their_own_limits() {
        let config = Config::default();
        let panes = [
            pane(1, 100, true, true),
            pane(2, 100, true, false),
            pane(3, 100, false, false),
        ];

        let limits = plan_scrollback_limits(&panes, &config);
        assert_eq!(limits[&PaneId(1)], 6_000);
        assert_eq!(limits[&PaneId(2)], 6_000);
        assert_eq!(limits[&PaneId(3)], 3_000);
    }

    #[test]
    fn budget_trims_hidden_panes_before_visible_and_focused() {
        let config = Config {
            scrollback_budget_mb: 24,
            ..Config::default()
        };
        let panes = [
            pane(1, 100, false, false),
            pane(2, 100, true, false),
            pane(3, 100, true, true),
        ];

        let limits = plan_scrollback_limits(&panes, &config);
        assert_eq!(
            limits[&PaneId(1)],
            1_000,
            "hidden pane should be trimmed first"
        );
        assert_eq!(
            limits[&PaneId(2)],
            1_000,
            "visible pane should be trimmed next"
        );
        assert!(
            limits[&PaneId(3)] > 1_000,
            "focused pane should retain the most history"
        );

        let total = panes.iter().fold(0usize, |total, pane| {
            total.saturating_add(estimate_scrollback_bytes(pane.cols, limits[&pane.id]))
        });
        assert!(total <= 24 * 1024 * 1024);
    }

    #[test]
    fn budget_never_trims_below_the_minimum() {
        let config = Config {
            scrollback_budget_mb: 1,
            ..Config::default()
        };
        let panes = [pane(1, 100, true, true)];

        let limits = plan_scrollback_limits(&panes, &config);
        assert_eq!(limits[&PaneId(1)], 1_000);
    }

    #[test]
    fn disabled_budget_leaves_limits_alone() {
        let config = Config::default();
        let panes = [pane(1, 100, false, false)];
        let limits = plan_scrollback_limits(&panes, &config);
        assert_eq!(limits[&PaneId(1)], 3_000);
    }

    #[test]
    fn gui_launches_start_in_the_home_directory() {
        let directory = choose_launch_directory(
            false,
            Some(PathBuf::from("/")),
            Some(PathBuf::from("/home/user")),
        );
        assert_eq!(directory, PathBuf::from("/home/user"));
    }

    #[test]
    fn terminal_launches_preserve_the_current_directory() {
        let directory = choose_launch_directory(
            true,
            Some(PathBuf::from("/work/project")),
            Some(PathBuf::from("/home/user")),
        );
        assert_eq!(directory, PathBuf::from("/work/project"));
    }
}
