use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, WindowSize};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::term::TermMode;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::config::{Config, RenderMode};
use crate::editor::Editor;
use crate::event::UserEvent;
use crate::explorer::FileExplorer;
use crate::git_status::{self, GitStatus};
use crate::git_window::GitWindow;
use crate::highlight::{self, HighlightRequest, StyledSpan};
use crate::input;
use crate::layout::{DividerLayout, PaneLayout, Rect, compute_layout};
use crate::menu;
use crate::render::quads::Quad;
use crate::render::scene::{Label, PaneLayer, Scene};
use crate::render::{CellMetrics, Renderer, SharedRenderResources, grid_size_for};
use crate::review_window::ReviewWindow;
use crate::shell::ShellCatalog;
use crate::sidebar::{
    self, LayoutSpec, Sidebar, SidebarHit, SidebarLayout, SidebarMode, SidebarRow, TabSummary,
};
use crate::term::color::{Theme, to_linear};
use crate::term::frame::{Cursor, CursorShapeKind, Frame, Row, Run};
use crate::workspace::{CloseOutcome, Pane, PaneId, SplitDirection, TabId, Workspace};

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    window: Option<WindowState>,
    review: Option<ReviewWindow>,
    git: Option<GitWindow>,
    shared_render: Option<SharedRenderResources>,
    _menu: Option<menu::MenuState>,
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
            shared_render: None,
            _menu: None,
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
            .with_min_inner_size(LogicalSize::new(800.0, 500.0));

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
        review.focus();
        review.request_redraw();
        log::info!("opened code review window");
        self.review = Some(review);
    }

    fn open_git_window(&mut self, event_loop: &ActiveEventLoop, cwd: PathBuf) {
        if let Some(git) = &self.git {
            git.focus();
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Git — F-Terminal")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(900.0, 560.0));

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
        git.focus();
        git.request_redraw();
        log::info!("opened git manager window");
        self.git = Some(git);
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
                let cwd = self
                    .window
                    .as_ref()
                    .map(|window| window.focused_cwd())
                    .unwrap_or_else(default_directory);
                self.open_review_window(event_loop, cwd);
                return;
            }
            UserEvent::Menu(menu::Action::GitManager) => {
                let cwd = self
                    .window
                    .as_ref()
                    .map(|window| window.focused_cwd())
                    .unwrap_or_else(default_directory);
                self.open_git_window(event_loop, cwd);
                return;
            }
            _ => {}
        }

        let Some(state) = &mut self.window else {
            return;
        };

        match event {
            UserEvent::PtyData { pane, bytes } => state.on_pty_data(pane, &bytes),
            UserEvent::PtyClosed { pane } => {
                state.close_pane(pane, event_loop, &self.config);
            }
            UserEvent::Term { pane, event } => {
                state.on_term_event(pane, event, event_loop, &self.config);
            }
            UserEvent::Menu(action) => state.dispatch_menu(action),
            UserEvent::Highlight {
                generation,
                revision,
                spans,
            } => state.on_highlight(generation, revision, spans),
            UserEvent::GitStatus { pane, cwd, status } => {
                state.on_git_status(pane, cwd, status);
            }
            UserEvent::OpenCodeReview { .. }
            | UserEvent::OpenGitManager { .. }
            | UserEvent::ReviewLoaded { .. }
            | UserEvent::PullRequestsLoaded { .. }
            | UserEvent::GitSnapshotLoaded { .. }
            | UserEvent::GitStatusLoaded { .. }
            | UserEvent::GitDetailLoaded { .. }
            | UserEvent::GitOperationFinished { .. } => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let mut wait = Duration::from_millis(750);

        if let Some(state) = &mut self.window {
            state.poll_background(&self.config);
            wait = state.next_wake();
        }

        if let Some(git) = &mut self.git {
            git.poll();
            wait = wait.min(git.next_wake());
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait));
    }
}

struct ViewLayout {
    sidebar: SidebarLayout,
    panes: Vec<PaneLayout>,
    dividers: Vec<DividerLayout>,
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

struct WindowState {
    window: Arc<Window>,
    renderer: Renderer,
    workspace: Workspace,
    proxy: EventLoopProxy<UserEvent>,
    clipboard: Option<arboard::Clipboard>,
    sidebar: Sidebar,
    view: ViewLayout,
    modifiers: ModifiersState,
    cursor_position: PhysicalPosition<f64>,
    scroll_accumulators: HashMap<PaneId, f32>,
    drag: Option<DragState>,
    selecting: Option<PaneId>,
    last_click: Option<(Instant, Point, PaneId)>,
    processes: Option<System>,
    explorer: FileExplorer,
    editor: Option<Editor>,
    highlight_tx: Option<Sender<HighlightRequest>>,
    highlight_generation: u64,
    highlight_sent_generation: u64,
    highlight_revision_seen: u64,
    highlight_due: Option<Instant>,
    dirty_panes: HashSet<PaneId>,
    editor_dirty: bool,
    last_cwd_poll: Instant,
    last_git_poll: Instant,
    git_in_flight: HashSet<PaneId>,
    editor_ratio: f32,
    pending_spawns: HashMap<PaneId, PendingSpawn>,
    shells: ShellCatalog,
    focus: Focus,
}

#[derive(Clone, Debug)]
struct PendingSpawn {
    cwd: PathBuf,
    profile_id: String,
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
        let attributes = Window::default_attributes()
            .with_title("F-Terminal")
            .with_inner_size(LogicalSize::new(1100.0, 700.0))
            .with_min_inner_size(LogicalSize::new(520.0, 260.0));
        let window = Arc::new(event_loop.create_window(attributes)?);
        window.set_ime_allowed(true);

        let renderer = pollster::block_on(Renderer::new(window.clone(), event_loop, config, None))?;
        let mut workspace = Workspace::new();
        workspace.create_terminal();
        let working_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let highlight_tx = highlight::spawn_worker(proxy.clone());
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

        let mut state = Self {
            window,
            renderer,
            workspace,
            proxy,
            clipboard: arboard::Clipboard::new().ok(),
            sidebar: Sidebar::default(),
            view: ViewLayout::empty(),
            modifiers: ModifiersState::empty(),
            cursor_position: PhysicalPosition::new(0.0, 0.0),
            scroll_accumulators: HashMap::new(),
            drag: None,
            selecting: None,
            last_click: None,
            processes: System::new().ok(),
            explorer: FileExplorer::new(working_directory),
            editor: None,
            highlight_tx,
            highlight_generation: 0,
            highlight_sent_generation: 0,
            highlight_revision_seen: 0,
            highlight_due: None,
            dirty_panes: HashSet::new(),
            editor_dirty: false,
            last_cwd_poll: Instant::now(),
            last_git_poll: Instant::now(),
            git_in_flight: HashSet::new(),
            editor_ratio: 0.42,
            pending_spawns: HashMap::new(),
            shells,
            focus: Focus::Terminal,
        };

        state.sync_layout(config)?;
        state.refresh_git_statuses();
        state.update_window_title();
        state.window.request_redraw();

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
            WindowEvent::RedrawRequested => self.on_redraw(config),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.on_key(event, event_loop, config);
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if self.focus == Focus::Editor && self.editor.is_some() {
                    if let Some(editor) = &mut self.editor {
                        editor.insert_str(&text);
                    }
                    self.editor_dirty = true;
                    self.ensure_editor_visible();
                    self.window.request_redraw();
                } else if let Some(pane) = self.focused_pane_mut() {
                    pane.session.write(text.as_bytes());
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

        let mut terminal_area = content_area;
        let mut editor_layout = None;

        if self.editor.is_some() {
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
                panel.right() - close_size - padding,
                panel.y + ((header_height - close_size) / 2.0).max(0.0),
                close_size,
                close_size,
            );
            let text = Rect::new(
                panel.x + padding,
                header.bottom() + padding,
                (panel.width - padding * 2.0).max(1.0),
                (panel.height - header_height - padding * 2.0).max(1.0),
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
                padding,
            });
        }

        let layout = compute_layout(
            &tab.layout,
            terminal_area,
            gap,
            padding,
            (26.0 * scale).round().max(18.0),
        );
        let pane_specs: Vec<(PaneId, Rect)> = layout
            .panes
            .iter()
            .map(|pane| (pane.id, pane.content))
            .collect();

        for (pane_id, content) in &pane_specs {
            let grid = grid_size_for(content.width, content.height, metrics);
            if let Some(pane) = self.workspace.pane_mut(*pane_id) {
                if pane.resize(grid) {
                    self.dirty_panes.insert(*pane_id);
                }
            } else {
                let spawn = self
                    .pending_spawns
                    .remove(pane_id)
                    .unwrap_or_else(|| PendingSpawn {
                        cwd: default_directory(),
                        profile_id: self.shells.default_id.clone(),
                    });
                let profile = self
                    .shells
                    .get(&spawn.profile_id)
                    .cloned()
                    .unwrap_or_else(|| self.shells.default_profile().clone());
                let pane = Pane::new(
                    *pane_id,
                    grid,
                    self.proxy.clone(),
                    &profile,
                    spawn.cwd,
                    config.scrollback_lines,
                )?;
                self.workspace.insert_pane(pane);
            }
        }

        self.view = ViewLayout {
            sidebar: sidebar_layout,
            panes: layout.panes,
            dividers: layout.dividers,
            editor: editor_layout,
        };

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

    fn relayout(&mut self, config: &Config) {
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

    fn new_terminal(&mut self, config: &Config) {
        let profile_id = self.shells.default_id.clone();
        self.new_terminal_with(&profile_id, config);
    }

    fn new_terminal_with(&mut self, profile_id: &str, config: &Config) {
        let cwd = self.focused_cwd();
        let (tab_id, pane_id) = self.workspace.create_terminal();
        self.pending_spawns.insert(
            pane_id,
            PendingSpawn {
                cwd,
                profile_id: profile_id.to_owned(),
            },
        );
        self.sidebar.profile_menu = false;
        self.sidebar.hovered_profile = None;
        self.focus = Focus::Terminal;
        // New terminals are appended to the sidebar; reveal the newest one.
        self.sidebar.scroll = f32::MAX;

        if let Err(error) = self.sync_layout(config) {
            log::error!("failed to create terminal: {error:#}");
            self.pending_spawns.remove(&pane_id);
            self.workspace.close_tab(tab_id);
            let _ = self.sync_layout(config);
        }

        self.update_window_title();
        self.window.request_redraw();
    }

    fn split_focused(&mut self, direction: SplitDirection, config: &Config) {
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
        self.pending_spawns
            .insert(new_pane, PendingSpawn { cwd, profile_id });
        self.focus = Focus::Terminal;

        if let Err(error) = self.sync_layout(config) {
            log::error!("failed to split pane: {error:#}");
            self.pending_spawns.remove(&new_pane);
            self.workspace.close_pane(new_pane);
            let _ = self.sync_layout(config);
        }

        self.update_window_title();
        self.window.request_redraw();
    }

    fn close_focused_pane(&mut self, event_loop: &ActiveEventLoop, config: &Config) {
        if let Some(pane) = self.workspace.focused_pane() {
            self.close_pane(pane, event_loop, config);
        }
    }

    fn close_pane(&mut self, pane: PaneId, event_loop: &ActiveEventLoop, config: &Config) {
        match self.workspace.close_pane(pane) {
            CloseOutcome::NotFound => {}
            CloseOutcome::WorkspaceEmpty => event_loop.exit(),
            CloseOutcome::PaneClosed | CloseOutcome::TabClosed => {
                self.pending_spawns.remove(&pane);
                self.scroll_accumulators.remove(&pane);
                self.git_in_flight.remove(&pane);
                self.selecting = None;
                self.sidebar.hovered = None;
                self.relayout(config);
            }
        }
    }

    fn close_tab(&mut self, tab: TabId, event_loop: &ActiveEventLoop, config: &Config) {
        match self.workspace.close_tab(tab) {
            CloseOutcome::NotFound => {}
            CloseOutcome::WorkspaceEmpty => event_loop.exit(),
            CloseOutcome::TabClosed | CloseOutcome::PaneClosed => {
                self.selecting = None;
                self.sidebar.hovered = None;
                self.relayout(config);
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
        self.workspace.select_tab(tab);
        if self.workspace.focus_pane(pane) {
            self.focus = Focus::Terminal;
            self.sync_explorer_to_focused();
            self.update_window_title();
            self.relayout(config);
        }
    }

    fn select_tab(&mut self, tab: TabId, config: &Config) {
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

    fn dispatch_menu(&mut self, action: menu::Action) {
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
            menu::Action::Settings => menu::open_settings(),
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
        let cwd = self.focused_cwd();
        self.explorer.set_root(cwd);
    }

    /// Poll the focused child process cwd so the file explorer and the menu
    /// actions follow `cd`.
    fn refresh_cwds(&mut self, config: &Config) {
        let mut pids = Vec::new();
        for tab in &self.workspace.tabs {
            for pane in tab.panes.values() {
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
            self.explorer.set_root(cwd);
            if self.sidebar.mode == SidebarMode::Files {
                self.relayout(config);
            }
        }

        for pane_id in cwd_changed {
            self.request_git_status(pane_id);
        }
    }

    fn request_git_status(&mut self, pane_id: PaneId) {
        if self.git_in_flight.contains(&pane_id) {
            return;
        }

        let Some(pane) = self.workspace.pane(pane_id) else {
            return;
        };
        let cwd = pane.cwd.clone().unwrap_or_else(default_directory);

        self.git_in_flight.insert(pane_id);
        let proxy = self.proxy.clone();
        let spawned = std::thread::Builder::new()
            .name("git-status".into())
            .spawn(move || {
                let status = git_status::inspect(&cwd);
                let _ = proxy.send_event(UserEvent::GitStatus {
                    pane: pane_id,
                    cwd,
                    status,
                });
            });

        if spawned.is_err() {
            self.git_in_flight.remove(&pane_id);
        }
    }

    fn refresh_git_statuses(&mut self) {
        self.git_in_flight
            .retain(|pane_id| self.workspace.pane(*pane_id).is_some());

        let panes: Vec<PaneId> = self.view.panes.iter().map(|pane| pane.id).collect();
        for pane_id in panes {
            self.request_git_status(pane_id);
        }
    }

    fn on_git_status(&mut self, pane_id: PaneId, cwd: PathBuf, status: Option<GitStatus>) {
        self.git_in_flight.remove(&pane_id);

        let Some(pane) = self.workspace.pane(pane_id) else {
            return;
        };
        if pane.cwd.as_ref() != Some(&cwd) {
            // The shell moved while the request was running; try the new cwd.
            self.request_git_status(pane_id);
            return;
        }

        let changed = pane.git_status != status;
        if changed {
            match &status {
                Some(status) => log::debug!(
                    "pane {} git: {} +{} -{}",
                    pane_id.0,
                    status.branch,
                    status.additions,
                    status.deletions
                ),
                None => log::debug!("pane {} is not in a git repository", pane_id.0),
            }
        }
        if let Some(pane) = self.workspace.pane_mut(pane_id) {
            pane.git_status = status;
        }

        if changed && self.view.panes.iter().any(|pane| pane.id == pane_id) {
            self.window.request_redraw();
        }
    }

    fn toggle_sidebar(&mut self, config: &Config) {
        self.sidebar.visible = !self.sidebar.visible;
        self.sidebar.hovered = None;
        self.sidebar.hovered_file = None;
        self.relayout(config);
    }

    fn toggle_sidebar_mode(&mut self, config: &Config) {
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
            self.explorer.set_root(self.focused_cwd());
            self.explorer.rebuild();
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
        self.explorer.set_root(self.focused_cwd());
        self.explorer.rebuild();
        self.relayout(config);
    }

    fn activate_file_row(&mut self, row_index: usize, config: &Config) {
        let Some(row) = self.explorer.rows.get(row_index).cloned() else {
            return;
        };

        if row.is_dir {
            self.explorer.toggle(&row.path);
            self.relayout(config);
        } else {
            self.open_file(row.path, config);
        }
    }

    fn open_file(&mut self, path: PathBuf, config: &Config) {
        match Editor::open(&path) {
            Ok(editor) => {
                log::info!("editing {}", path.display());
                self.highlight_revision_seen = editor.revision;
                self.highlight_generation = self.highlight_generation.wrapping_add(1);
                self.highlight_due = Some(Instant::now());
                self.editor_dirty = true;
                self.editor = Some(editor);
                self.focus = Focus::Editor;
                self.relayout(config);
            }
            Err(error) => log::warn!("failed to open {}: {error:#}", path.display()),
        }
    }

    fn poll_background(&mut self, config: &Config) {
        let now = Instant::now();

        if now.duration_since(self.last_cwd_poll) >= Duration::from_millis(750) {
            self.last_cwd_poll = now;
            self.refresh_cwds(config);
        }

        if now.duration_since(self.last_git_poll) >= Duration::from_millis(2000) {
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
                | KeyCode::Digit9 => return false,
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

    fn paste_into_editor(&mut self) {
        let Some(clipboard) = &mut self.clipboard else {
            return;
        };
        if let Ok(text) = clipboard.get_text()
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

        match &mut self.clipboard {
            Some(clipboard) => {
                if let Err(error) = clipboard.set_text(text) {
                    log::warn!("clipboard copy failed: {error}");
                }
            }
            None => log::warn!("clipboard is unavailable"),
        }
    }

    fn paste(&mut self) {
        let Some(clipboard) = &mut self.clipboard else {
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
    }

    fn focus_direction(&mut self, direction: FocusDirection) {
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

        if self.sidebar.profile_menu {
            self.on_profile_menu_key(&event, config);
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

        let Some(pane_id) = self.workspace.focused_pane() else {
            return;
        };
        let mode = self
            .workspace
            .pane(pane_id)
            .map(|pane| pane.terminal.mode())
            .unwrap_or_default();

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
            self.window.request_redraw();
            return;
        }

        if let Some(bytes) = input::encode_key(&event, self.modifiers, mode, config.option_as_meta)
            && let Some(pane) = self.workspace.pane_mut(pane_id)
        {
            if pane.terminal.display_offset() > 0 {
                pane.terminal.scroll(Scroll::Bottom);
                self.dirty_panes.insert(pane_id);
                self.window.request_redraw();
            }
            pane.session.write(&bytes);
        }
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
                menu::open_explorer(&self.focused_cwd());
                true
            }
            KeyCode::KeyR if shift => {
                let _ = self.proxy.send_event(UserEvent::OpenCodeReview {
                    cwd: self.focused_cwd(),
                });
                true
            }
            KeyCode::KeyG if shift => {
                let _ = self.proxy.send_event(UserEvent::OpenGitManager {
                    cwd: self.focused_cwd(),
                });
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
            KeyCode::KeyE if !macos => {
                self.split_focused(SplitDirection::Down, config);
                true
            }
            KeyCode::KeyB => {
                self.toggle_sidebar(config);
                true
            }
            KeyCode::KeyF if shift => {
                self.toggle_sidebar_mode(config);
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
                self.window.request_redraw();
            }
            return;
        }

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

        if state == ElementState::Pressed && button == MouseButton::Left {
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
                        SidebarHit::ToggleWorkspace(tab) => {
                            self.toggle_workspace(tab, config);
                        }
                        SidebarHit::Select(tab) => self.select_tab(tab, config),
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

        if let Some(editor_layout) = self.view.editor
            && editor_layout.panel.contains(x, y)
        {
            if state == ElementState::Pressed && button == MouseButton::Left {
                if editor_layout.close.contains(x, y) {
                    self.close_editor(config);
                } else {
                    self.focus = Focus::Editor;
                    self.set_editor_cursor_from_point(self.cursor_position);
                }
                self.window.request_redraw();
            }
            return;
        }

        let Some(pane_id) = self.pane_at(x, y) else {
            return;
        };

        if state == ElementState::Pressed {
            self.focus = Focus::Terminal;
            self.workspace.focus_pane(pane_id);
            self.sync_explorer_to_focused();
            self.update_window_title();
            self.window.request_redraw();
        }

        if self
            .view
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .is_some_and(|pane| pane.footer.contains(x, y))
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
                }
                self.window.request_redraw();
            }
        }
    }

    fn on_mouse_wheel(&mut self, delta: MouseScrollDelta, config: &Config) {
        let x = self.cursor_position.x as f32;
        let y = self.cursor_position.y as f32;

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
            .is_some_and(|pane| pane.footer.contains(x, y))
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

        let offset = x - column as f32 * metrics.width;
        let side = if offset < metrics.width / 2.0 {
            Side::Left
        } else {
            Side::Right
        };
        let line = row as i32 - pane.terminal.display_offset() as i32;

        Some((Point::new(Line(line), Column(column)), side))
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

        Some((column, row))
    }

    // ------------------------------------------------------------------
    // PTY / terminal events
    // ------------------------------------------------------------------

    fn on_pty_data(&mut self, pane_id: PaneId, bytes: &[u8]) {
        let Some(pane) = self.workspace.pane_mut(pane_id) else {
            return;
        };
        pane.terminal.feed(bytes);
        self.dirty_panes.insert(pane_id);

        if self.workspace.is_pane_visible(pane_id) {
            self.window.request_redraw();
        }
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
                self.workspace.set_pane_title(pane_id, Some(title));
                self.update_window_title();
            }
            Event::ResetTitle => {
                self.workspace.set_pane_title(pane_id, None);
                self.update_window_title();
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
                if let Some(clipboard) = &mut self.clipboard
                    && let Err(error) = clipboard.set_text(text)
                {
                    log::warn!("OSC 52 clipboard store failed: {error}");
                }
            }
            Event::ClipboardLoad(_, formatter) => {
                let text = self
                    .clipboard
                    .as_mut()
                    .and_then(|clipboard| clipboard.get_text().ok())
                    .unwrap_or_default();
                let response = formatter(&text);
                if let Some(pane) = self.workspace.pane_mut(pane_id) {
                    pane.session.write(response.as_bytes());
                }
            }
            Event::Bell => log::debug!("terminal bell"),
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
            Event::Exit | Event::ChildExit(_) => {
                self.close_pane(pane_id, event_loop, config);
                return;
            }
        }

        self.window.request_redraw();
    }

    fn on_redraw(&mut self, config: &Config) {
        let theme = &config.theme;
        let scale = self.renderer.scale_factor() as f32;
        let metrics = self.renderer.cell_metrics();
        let ui_font_size = self.renderer.physical(config.ui_font_size);
        let header_font_size = (ui_font_size * 0.82).max(9.0);

        let Some(tab) = self.workspace.active_tab() else {
            return;
        };
        let focused = tab.focused;
        let multi_pane = tab.is_multi_pane();

        let mut frames: Vec<(PaneId, Rect, Frame)> = Vec::with_capacity(self.view.panes.len());
        for pane_layout in &self.view.panes {
            if let Some(pane) = self.workspace.pane(pane_layout.id) {
                frames.push((
                    pane_layout.id,
                    pane_layout.content,
                    pane.terminal.frame(theme),
                ));
            }
        }

        let editor_frame = self
            .editor
            .as_ref()
            .zip(self.view.editor)
            .map(|(editor, layout)| {
                (
                    layout.text,
                    build_editor_frame(editor, layout.text, metrics, theme),
                )
            });

        let mut quads: Vec<Quad> = Vec::new();
        let mut labels: Vec<Label> = Vec::new();

        self.build_sidebar_chrome(
            &config.theme,
            &mut quads,
            &mut labels,
            ui_font_size,
            header_font_size,
        );

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

        if multi_pane && self.focus == Focus::Terminal {
            for pane_layout in &self.view.panes {
                if pane_layout.id != focused {
                    continue;
                }
                let rect = pane_layout.outer;
                let thickness = (2.0 * scale).round().max(1.0);
                let color = to_linear(theme.accent, 0.9);
                quads.push(Quad::new(rect.x, rect.y, rect.width, thickness, color));
                quads.push(Quad::new(
                    rect.x,
                    rect.bottom() - thickness,
                    rect.width,
                    thickness,
                    color,
                ));
                quads.push(Quad::new(rect.x, rect.y, thickness, rect.height, color));
                quads.push(Quad::new(
                    rect.right() - thickness,
                    rect.y,
                    thickness,
                    rect.height,
                    color,
                ));
            }
        }

        for (_, content, frame) in &frames {
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

        for (_, content, frame) in &frames {
            push_cursor_quads(&mut quads, *content, frame, metrics, theme);
        }
        if let Some((content, frame)) = &editor_frame {
            push_cursor_quads(&mut quads, *content, frame, metrics, theme);
        }

        {
            let view = &self.view;
            let workspace = &self.workspace;
            let renderer = &mut self.renderer;
            for pane_layout in &view.panes {
                let Some(pane) = workspace.pane(pane_layout.id) else {
                    continue;
                };
                build_pane_footer(
                    pane,
                    pane_layout.footer,
                    PaneFooterStyle {
                        scale,
                        ui_font_size,
                        theme,
                    },
                    renderer,
                    &mut quads,
                    &mut labels,
                );
            }
        }

        if let (Some(editor), Some(layout)) = (&self.editor, self.view.editor) {
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

            let focused_editor = self.focus == Focus::Editor;
            let thickness = if focused_editor {
                (2.0 * scale).round().max(1.0)
            } else {
                1.0
            };
            let border = if focused_editor {
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
            let dirty_width = if editor.dirty { 16.0 * scale } else { 0.0 };
            labels.push(Label {
                text: editor.path.display().to_string(),
                x: layout.header.x + layout.padding,
                y: layout.header.y + (layout.header.height - header_line) / 2.0,
                width: (layout.close.x - layout.header.x - layout.padding * 2.0 - dirty_width)
                    .max(1.0),
                font_size: header_font,
                color: theme.sidebar_text,
                bold: false,
            });

            if editor.dirty {
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

            let close_font = ui_font_size * 1.0;
            labels.push(Label {
                text: "×".into(),
                x: layout.close.x + (layout.close.width - close_font * 0.55) / 2.0,
                y: layout.close.y + (layout.close.height - close_font * 1.35) / 2.0,
                width: layout.close.width,
                font_size: close_font,
                color: theme.sidebar_text_dim,
                bold: false,
            });
        }

        let mut scene_panes: Vec<PaneLayer<'_>> = frames
            .iter()
            .map(|(id, content, frame)| PaneLayer {
                id: id.0,
                content: *content,
                frame,
                dirty: self.dirty_panes.contains(id),
            })
            .collect();
        if let Some((content, frame)) = &editor_frame {
            scene_panes.push(PaneLayer {
                id: 0,
                content: *content,
                frame,
                dirty: self.editor_dirty,
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

        self.dirty_panes.clear();
        self.editor_dirty = false;

        let ime_target = if self.focus == Focus::Editor {
            editor_frame
                .as_ref()
                .map(|(content, frame)| (*content, frame))
        } else {
            frames
                .iter()
                .find(|(id, _, _)| *id == focused)
                .map(|(_, content, frame)| (*content, frame))
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
            "NEW TERMINAL"
        } else {
            match layout.mode {
                SidebarMode::Terminals => "SESSIONS",
                SidebarMode::Files => "FILES",
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
                    text: "No shells detected".into(),
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
                let close_size = ui_font_size * 1.1;

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

                            labels.push(Label {
                                text: "×".into(),
                                x: item.close.x + (item.close.width - close_size * 0.55) / 2.0,
                                y: item.close.y + (item.close.height - close_size * 1.35) / 2.0,
                                width: item.close.width,
                                font_size: close_size,
                                color: theme.sidebar_text_dim,
                                bold: false,
                            });
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

                            labels.push(Label {
                                text: "×".into(),
                                x: item.close.x + (item.close.width - close_size * 0.55) / 2.0,
                                y: item.close.y + (item.close.height - close_size * 1.35) / 2.0,
                                width: item.close.width,
                                font_size: close_size,
                                color: theme.sidebar_text_dim,
                                bold: false,
                            });
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

fn default_directory() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        return cwd;
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return PathBuf::from(home);
    }
    PathBuf::from(".")
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
) {
    let PaneFooterStyle {
        scale,
        ui_font_size,
        theme,
    } = style;

    if footer.width <= 1.0 || footer.height <= 1.0 {
        return;
    }

    let Some(status) = &pane.git_status else {
        return;
    };
    if footer.height < 12.0 * scale {
        return;
    }

    let font = (ui_font_size * 0.86).max(10.0);
    let line = font * 1.35;
    let pill_height = (footer.height - 6.0 * scale).clamp(16.0 * scale, 34.0 * scale);
    let radius = pill_height / 2.0;
    let padding = 9.0 * scale;
    let gap = 6.0 * scale;
    let edge = 4.0 * scale;
    let y = footer.y + (footer.height - pill_height) / 2.0;
    let text_y = y + (pill_height - line) / 2.0;

    let additions = format!("+{}", status.additions);
    let deletions = format!("-{}", status.deletions);
    let additions_width = renderer.measure_ui_text(&additions, font, false);
    let deletions_width = renderer.measure_ui_text(&deletions, font, false);
    let changes_width = padding * 2.0 + additions_width + gap + deletions_width;

    let can_show_changes = footer.width >= changes_width + edge * 2.0 + 60.0 * scale;
    let branch_max = if can_show_changes {
        footer.width - edge * 2.0 - changes_width - gap
    } else {
        footer.width - edge * 2.0
    }
    .max(0.0);

    let dot = (4.0 * scale).max(2.0);
    let dot_gap = 5.0 * scale;
    let branch_text_max = (branch_max - padding * 2.0 - dot - dot_gap).max(0.0);
    let branch = truncate_to_width(&status.branch, branch_text_max, font, true, renderer);

    let mut branch_width = 0.0;
    if !branch.is_empty() && branch_max > dot + dot_gap + padding {
        let branch_text_width = renderer.measure_ui_text(&branch, font, true);
        let width = (branch_text_width + padding * 2.0 + dot + dot_gap).min(branch_max);
        let x = footer.x + edge;
        quads.push(Quad::rounded(
            x,
            y,
            width,
            pill_height,
            radius,
            to_linear(theme.sidebar_active, 1.0),
        ));
        quads.push(Quad::rounded(
            x + padding,
            y + (pill_height - dot) / 2.0,
            dot,
            dot,
            dot / 2.0,
            to_linear(theme.accent, 1.0),
        ));
        labels.push(Label {
            text: branch,
            x: x + padding + dot + dot_gap,
            y: text_y,
            width: (width - padding * 2.0 - dot - dot_gap).max(1.0),
            font_size: font,
            color: theme.sidebar_text,
            bold: true,
        });
        branch_width = width;
    }

    if !can_show_changes {
        return;
    }

    let changes_x = footer.x
        + edge
        + if branch_width > 0.0 {
            branch_width + gap
        } else {
            0.0
        };
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
    let green_width = padding + additions_width + gap * 0.5;
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

    let additions_color = if status.additions > 0 {
        theme.diff_add_text
    } else {
        theme.sidebar_text_dim
    };
    labels.push(Label {
        text: additions,
        x: changes_x + padding,
        y: text_y,
        width: (additions_width + 2.0 * scale).max(1.0),
        font_size: font,
        color: additions_color,
        bold: false,
    });

    let deletions_color = if status.deletions > 0 {
        theme.diff_remove_text
    } else {
        theme.sidebar_text_dim
    };
    labels.push(Label {
        text: deletions,
        x: changes_x + padding + additions_width + gap,
        y: text_y,
        width: (deletions_width + 2.0 * scale).max(1.0),
        font_size: font,
        color: deletions_color,
        bold: false,
    });
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

    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let mut candidate: String = chars.iter().collect();
        candidate.push('…');
        if renderer.measure_ui_text(&candidate, font_size, bold) <= max_width {
            return candidate;
        }
    }

    String::new()
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
}
