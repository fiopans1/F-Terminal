use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::config::Config;
use crate::diff_view::build_file_frame;
use crate::event::UserEvent;
use crate::git::ops::{self, Action as GitAction};
use crate::git::{self, CommitInfo, Snapshot, StatusEntry, StatusSnapshot, status};
use crate::layout::Rect;
use crate::render::Renderer;
use crate::render::quads::Quad;
use crate::render::scene::{Label, PaneLayer, Scene};
use crate::review::{FileStatus, ReviewDetail, ReviewKind, ReviewSummary};
use crate::term::color::{Theme, to_linear};
use crate::term::frame::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeftTab {
    Changes,
    Refs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Changes,
    Refs,
    History,
    Detail,
    Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListKind {
    Staged,
    Unstaged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefTarget {
    Local(usize),
    Remote(usize),
    Tag(usize),
    Stash(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderAction {
    Fetch,
    Pull,
    Push,
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeftAction {
    StageAll,
    UnstageAll,
    AbortMerge,
    NewBranch,
    Checkout,
    Merge,
    Delete,
    StashPop,
    StashDrop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelAction {
    Commit,
    Amend,
    Stash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChangeRowKind {
    Section(&'static str),
    Entry(ListKind, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefRowKind {
    Section(&'static str),
    Target(RefTarget),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
enum DetailTarget {
    #[default]
    Working,
    Commit(String),
}

#[derive(Clone, Debug)]
enum PendingAction {
    Discard { paths: Vec<String>, untracked: bool },
    DeleteBranch { name: String },
    DropStash { index: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputAction {
    NewBranch,
}

#[derive(Clone, Debug)]
enum Dialog {
    Confirm {
        title: String,
        message: String,
        action: PendingAction,
    },
    Input {
        title: String,
        prompt: String,
        value: String,
        action: InputAction,
    },
}

#[derive(Clone, Debug)]
enum RefKey {
    Local(String),
    Remote(String),
    Tag(String),
    Stash(usize),
}

#[derive(Clone, Copy)]
struct DrawContext<'a> {
    theme: &'a Theme,
    scale: f32,
    ui_font: f32,
    metrics: crate::render::CellMetrics,
}

#[derive(Clone, Copy)]
struct ButtonStyle<'a> {
    theme: &'a Theme,
    scale: f32,
    font_size: f32,
}

struct Layout {
    header: Rect,
    footer: Rect,
    left: Rect,
    center: Rect,
    right: Rect,
    tabs: [Rect; 2],
    close: Rect,
    list_area: Rect,
    header_buttons: Vec<(HeaderAction, Rect, bool)>,
    left_actions: Vec<(LeftAction, Rect, bool)>,
    change_rows: Vec<(ChangeRowKind, Rect)>,
    ref_rows: Vec<(RefRowKind, Rect)>,
    history_rows: Vec<(usize, Rect)>,
    detail_header: Rect,
    file_list: Option<Rect>,
    file_rows: Vec<(usize, Rect)>,
    diff_area: Option<Rect>,
    diff_title: Option<Rect>,
    commit_panel: Rect,
    message: Rect,
    panel_buttons: Vec<(PanelAction, Rect, bool)>,
    dialog_buttons: Vec<(bool, Rect)>, // confirm, cancel
    dialog_panel: Option<Rect>,
    dialog_input: Option<Rect>,
    padding: f32,
    left_visible: usize,
    history_visible: usize,
    files_visible: usize,
    diff_visible: usize,
}

pub struct GitWindow {
    window: Arc<Window>,
    renderer: Renderer,
    proxy: EventLoopProxy<UserEvent>,
    directory: PathBuf,
    snapshot: Option<Snapshot>,
    loading: bool,
    error: Option<String>,
    focus: Focus,
    left_tab: LeftTab,
    selected_change: Option<(ListKind, usize)>,
    selected_ref: Option<RefTarget>,
    selected_history: usize,
    selected_file: usize,
    changes_offset: usize,
    refs_offset: usize,
    history_offset: usize,
    file_offset: usize,
    scroll_row: usize,
    scroll_col: usize,
    message: String,
    amend: bool,
    detail: Option<ReviewDetail>,
    detail_loading: bool,
    detail_error: Option<String>,
    detail_target: DetailTarget,
    snapshot_request: u64,
    status_request: u64,
    detail_request: u64,
    operation_request: u64,
    busy: Option<String>,
    toast: Option<(String, bool, Instant)>,
    last_status_poll: Instant,
    dialog: Option<Dialog>,
    last_ref_click: Option<(Instant, RefTarget)>,
    modifiers: ModifiersState,
    cursor: PhysicalPosition<f64>,
    close_requested: bool,
}

impl GitWindow {
    pub fn new(
        window: Arc<Window>,
        renderer: Renderer,
        directory: PathBuf,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let mut window = Self {
            window,
            renderer,
            proxy,
            directory,
            snapshot: None,
            loading: true,
            error: None,
            focus: Focus::Changes,
            left_tab: LeftTab::Changes,
            selected_change: None,
            selected_ref: None,
            selected_history: 0,
            selected_file: 0,
            changes_offset: 0,
            refs_offset: 0,
            history_offset: 0,
            file_offset: 0,
            scroll_row: 0,
            scroll_col: 0,
            message: String::new(),
            amend: false,
            detail: None,
            detail_loading: false,
            detail_error: None,
            detail_target: DetailTarget::Working,
            snapshot_request: 0,
            status_request: 0,
            detail_request: 0,
            operation_request: 0,
            busy: None,
            toast: None,
            last_status_poll: Instant::now(),
            dialog: None,
            last_ref_click: None,
            modifiers: ModifiersState::empty(),
            cursor: PhysicalPosition::new(0.0, 0.0),
            close_requested: false,
        };
        window.start_snapshot();
        window
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    pub fn focus(&self) {
        self.window.focus_window();
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn on_window_event(&mut self, event: WindowEvent, config: &Config) {
        match event {
            WindowEvent::Resized(size) => {
                self.renderer.resize(size);
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.renderer.set_scale_factor(scale_factor, config);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => self.draw(config),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => self.on_key(event),
            WindowEvent::Ime(Ime::Commit(text)) => self.on_ime(&text),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse(state, button, config);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.on_wheel(delta, config);
            }
            WindowEvent::Focused(true) => self.poll(),
            _ => {}
        }
    }

    pub fn poll(&mut self) {
        let now = Instant::now();

        if let Some((_, _, shown)) = &self.toast
            && now.duration_since(*shown) >= Duration::from_secs(7)
        {
            self.toast = None;
            self.window.request_redraw();
        }

        if now.duration_since(self.last_status_poll) >= Duration::from_secs(2) {
            self.last_status_poll = now;
            self.start_status_poll();
        }
    }

    pub fn next_wake(&self) -> Duration {
        Duration::from_secs(2)
            .saturating_sub(Instant::now().duration_since(self.last_status_poll))
            .max(Duration::from_millis(250))
    }

    pub fn apply_snapshot(
        &mut self,
        request_id: u64,
        result: std::result::Result<Snapshot, String>,
    ) {
        if request_id != self.snapshot_request {
            return;
        }

        self.loading = false;
        match result {
            Ok(snapshot) => {
                log::info!(
                    "git window loaded: {} commits, {} local branches, {} changes",
                    snapshot.history.commits.len(),
                    snapshot.refs.local_branches.len(),
                    snapshot.status.entries.len()
                );
                let previous_change = self
                    .selected_change
                    .and_then(|(_, index)| self.entry(index))
                    .map(|entry| entry.path.clone());
                let previous_ref = self.selected_ref.and_then(|target| self.ref_key(target));
                let previous_hash = self.selected_commit().map(|commit| commit.hash.clone());
                let was_working = self.selected_history == 0;

                self.snapshot = Some(snapshot);
                self.error = None;
                self.restore_selection(previous_change, previous_ref, previous_hash, was_working);
                self.last_status_poll = Instant::now();

                self.start_detail();
                self.window.request_redraw();
            }
            Err(error) => {
                if self.snapshot.is_none() {
                    self.error = Some(error.clone());
                    self.loading = false;
                }
                self.toast = Some((error, true, Instant::now()));
                self.window.request_redraw();
            }
        }
    }

    pub fn apply_status(
        &mut self,
        request_id: u64,
        result: std::result::Result<StatusSnapshot, String>,
    ) {
        if request_id != self.status_request {
            return;
        }
        let Ok(status) = result else {
            return;
        };

        let previous_change = self
            .selected_change
            .and_then(|(_, index)| self.entry(index))
            .map(|entry| entry.path.clone());
        let previous_ref = self.selected_ref.and_then(|target| self.ref_key(target));
        let previous_hash = self.selected_commit().map(|commit| commit.hash.clone());
        let was_working = self.selected_history == 0;

        let branch_changed = {
            let Some(snapshot) = &mut self.snapshot else {
                return;
            };
            if snapshot.status == status {
                return;
            }
            let changed =
                snapshot.status.branch != status.branch || snapshot.status.head != status.head;
            snapshot.status = status;
            changed
        };

        self.restore_selection(previous_change, previous_ref, previous_hash, was_working);

        if branch_changed {
            self.start_snapshot();
        } else {
            self.window.request_redraw();
        }
    }

    pub fn apply_detail(
        &mut self,
        request_id: u64,
        result: std::result::Result<ReviewDetail, String>,
    ) {
        if request_id != self.detail_request {
            return;
        }
        self.detail_loading = false;
        match result {
            Ok(detail) => {
                let previous_path = self
                    .detail
                    .as_ref()
                    .and_then(|current| current.files.get(self.selected_file))
                    .map(|file| file.path.clone());
                self.selected_file = previous_path
                    .and_then(|path| detail.files.iter().position(|file| file.path == path))
                    .unwrap_or(0);
                self.detail = Some(detail);
                self.detail_error = None;
                self.file_offset = 0;
                self.scroll_row = 0;
                self.scroll_col = 0;
            }
            Err(error) => {
                self.detail = None;
                self.detail_error = Some(error);
            }
        }
        self.window.request_redraw();
    }

    pub fn apply_operation(
        &mut self,
        request_id: u64,
        result: std::result::Result<String, String>,
    ) {
        if request_id != self.operation_request {
            return;
        }

        let was_commit = self.busy.as_deref() == Some("Committing");
        self.busy = None;

        match result {
            Ok(message) => {
                log::info!("git operation finished: {message}");
                if was_commit {
                    self.message.clear();
                    self.amend = false;
                }
                self.toast = Some((message, false, Instant::now()));
            }
            Err(error) => {
                log::warn!("git operation failed: {error}");
                self.toast = Some((error, true, Instant::now()));
            }
        }

        self.start_snapshot();
        self.window.request_redraw();
    }

    fn start_snapshot(&mut self) {
        self.snapshot_request = self.snapshot_request.wrapping_add(1);
        let request_id = self.snapshot_request;
        self.loading = self.snapshot.is_none();
        let directory = self.directory.clone();
        let proxy = self.proxy.clone();
        let window_id = self.window.id();

        let spawned = std::thread::Builder::new()
            .name("git-snapshot".into())
            .spawn(move || {
                let result = git::load_snapshot(&directory).map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::GitSnapshotLoaded {
                    window_id,
                    request_id,
                    result,
                });
            });

        if spawned.is_err() {
            self.loading = false;
            self.error = Some("failed to start the git snapshot loader".to_owned());
        }
        self.window.request_redraw();
    }

    fn start_status_poll(&mut self) {
        if self.busy.is_some() {
            return;
        }
        let Some(repo) = self.snapshot.as_ref().map(|snapshot| snapshot.repo.clone()) else {
            return;
        };

        self.status_request = self.status_request.wrapping_add(1);
        let request_id = self.status_request;
        let proxy = self.proxy.clone();
        let window_id = self.window.id();

        let spawned = std::thread::Builder::new()
            .name("git-status".into())
            .spawn(move || {
                let result = status::load(&repo).map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::GitStatusLoaded {
                    window_id,
                    request_id,
                    result,
                });
            });

        if spawned.is_err() {
            log::warn!("failed to start the git status poller");
        }
    }

    fn start_detail(&mut self) {
        let Some(repo) = self.snapshot.as_ref().map(|snapshot| snapshot.repo.clone()) else {
            return;
        };

        let target = self.detail_target_for_selection();
        let summary = match &target {
            DetailTarget::Working => ReviewSummary {
                title: "Working tree".to_owned(),
                subtitle: String::new(),
                kind: ReviewKind::Working,
            },
            DetailTarget::Commit(hash) => {
                let Some(commit) = self.selected_commit().cloned() else {
                    return;
                };
                ReviewSummary {
                    title: commit.subject.clone(),
                    subtitle: commit.short.clone(),
                    kind: ReviewKind::Commit { hash: hash.clone() },
                }
            }
        };

        let target_changed = self.detail_target != target;
        self.detail_target = target;
        self.detail_request = self.detail_request.wrapping_add(1);
        let request_id = self.detail_request;
        self.detail_loading = true;
        self.detail_error = None;
        if target_changed {
            self.detail = None;
            self.selected_file = 0;
            self.file_offset = 0;
            self.scroll_row = 0;
            self.scroll_col = 0;
        }

        let proxy = self.proxy.clone();
        let window_id = self.window.id();
        let spawned = std::thread::Builder::new()
            .name("git-detail".into())
            .spawn(move || {
                let tokens = crate::forge::ForgeTokens::default();
                let result = crate::review::load_detail(&repo, &summary, &tokens)
                    .map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::GitDetailLoaded {
                    window_id,
                    request_id,
                    result,
                });
            });

        if spawned.is_err() {
            self.detail_loading = false;
            self.detail_error = Some("failed to start the diff loader".to_owned());
        }
        self.window.request_redraw();
    }

    fn run_operation(&mut self, action: GitAction) {
        if self.busy.is_some() {
            return;
        }
        let Some(repo) = self.snapshot.as_ref().map(|snapshot| snapshot.repo.clone()) else {
            return;
        };

        self.operation_request = self.operation_request.wrapping_add(1);
        let request_id = self.operation_request;
        let label = action.label().to_owned();
        self.busy = Some(label.clone());
        self.toast = None;

        let proxy = self.proxy.clone();
        let window_id = self.window.id();
        let spawned = std::thread::Builder::new()
            .name("git-operation".into())
            .spawn(move || {
                let result = ops::apply(&repo, &action).map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::GitOperationFinished {
                    window_id,
                    request_id,
                    result,
                });
            });

        if spawned.is_err() {
            self.busy = None;
            self.toast = Some((
                "failed to start the git operation".to_owned(),
                true,
                Instant::now(),
            ));
        }
        self.window.request_redraw();
    }

    fn restore_selection(
        &mut self,
        change_path: Option<String>,
        ref_key: Option<RefKey>,
        commit_hash: Option<String>,
        was_working: bool,
    ) {
        let entries = self.change_order();
        self.selected_change = change_path
            .and_then(|path| {
                entries
                    .iter()
                    .copied()
                    .find(|(_, index)| self.entry(*index).is_some_and(|entry| entry.path == path))
            })
            .or_else(|| entries.first().copied());

        if was_working {
            self.selected_history = 0;
        } else if let Some(hash) = commit_hash {
            self.selected_history = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| {
                    snapshot
                        .history
                        .commits
                        .iter()
                        .position(|commit| commit.hash == hash)
                        .map(|index| index + 1)
                })
                .unwrap_or(0);
        } else {
            self.selected_history = self.selected_history.min(self.commit_count());
        }

        self.selected_ref = ref_key
            .and_then(|key| self.find_ref(&key))
            .or_else(|| self.ref_targets().first().copied());
        self.selected_file = self.selected_file.min(
            self.detail
                .as_ref()
                .map_or(0, |detail| detail.files.len().saturating_sub(1)),
        );
    }

    fn ref_key(&self, target: RefTarget) -> Option<RefKey> {
        let snapshot = self.snapshot.as_ref()?;
        match target {
            RefTarget::Local(index) => snapshot
                .refs
                .local_branches
                .get(index)
                .map(|branch| RefKey::Local(branch.name.clone())),
            RefTarget::Remote(index) => snapshot
                .refs
                .remote_branches
                .get(index)
                .map(|branch| RefKey::Remote(branch.name.clone())),
            RefTarget::Tag(index) => snapshot
                .refs
                .tags
                .get(index)
                .map(|tag| RefKey::Tag(tag.name.clone())),
            RefTarget::Stash(index) => snapshot
                .refs
                .stashes
                .get(index)
                .map(|stash| RefKey::Stash(stash.index)),
        }
    }

    fn find_ref(&self, key: &RefKey) -> Option<RefTarget> {
        let snapshot = self.snapshot.as_ref()?;
        match key {
            RefKey::Local(name) => snapshot
                .refs
                .local_branches
                .iter()
                .position(|branch| &branch.name == name)
                .map(RefTarget::Local),
            RefKey::Remote(name) => snapshot
                .refs
                .remote_branches
                .iter()
                .position(|branch| &branch.name == name)
                .map(RefTarget::Remote),
            RefKey::Tag(name) => snapshot
                .refs
                .tags
                .iter()
                .position(|tag| &tag.name == name)
                .map(RefTarget::Tag),
            RefKey::Stash(index) => snapshot
                .refs
                .stashes
                .iter()
                .position(|stash| stash.index == *index)
                .map(RefTarget::Stash),
        }
    }

    fn entry(&self, index: usize) -> Option<&StatusEntry> {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.status.entries.get(index))
    }

    fn change_order(&self) -> Vec<(ListKind, usize)> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let mut order = Vec::new();
        order.extend(
            snapshot
                .status
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.staged)
                .map(|(index, _)| (ListKind::Staged, index)),
        );
        order.extend(
            snapshot
                .status
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.unstaged)
                .map(|(index, _)| (ListKind::Unstaged, index)),
        );
        order
    }

    fn ref_targets(&self) -> Vec<RefTarget> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let mut targets = Vec::new();
        targets.extend((0..snapshot.refs.local_branches.len()).map(RefTarget::Local));
        targets.extend((0..snapshot.refs.remote_branches.len()).map(RefTarget::Remote));
        targets.extend((0..snapshot.refs.tags.len()).map(RefTarget::Tag));
        targets.extend((0..snapshot.refs.stashes.len()).map(RefTarget::Stash));
        targets
    }

    fn commit_count(&self) -> usize {
        self.snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.history.commits.len())
    }

    fn selected_commit(&self) -> Option<&CommitInfo> {
        if self.selected_history == 0 {
            return None;
        }
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.history.commits.get(self.selected_history - 1))
    }

    fn detail_target_for_selection(&self) -> DetailTarget {
        self.selected_commit()
            .map(|commit| DetailTarget::Commit(commit.hash.clone()))
            .unwrap_or(DetailTarget::Working)
    }

    fn select_change(&mut self, list: ListKind, index: usize) {
        self.selected_change = Some((list, index));
        self.focus = if self.left_tab == LeftTab::Changes {
            Focus::Changes
        } else {
            Focus::Refs
        };
        self.window.request_redraw();
    }

    fn select_ref(&mut self, target: RefTarget) {
        self.selected_ref = Some(target);
        self.focus = Focus::Refs;
        self.window.request_redraw();
    }

    fn select_history(&mut self, index: usize) {
        let index = index.min(self.commit_count());
        if self.selected_history != index {
            self.selected_history = index;
            self.start_detail();
        }
        self.focus = Focus::History;
        self.window.request_redraw();
    }

    fn toggle_staged(&mut self, list: ListKind, index: usize) {
        let Some(entry) = self.entry(index).cloned() else {
            return;
        };
        let paths = entry_paths(&entry);
        match list {
            ListKind::Staged => self.run_operation(GitAction::Unstage { paths }),
            ListKind::Unstaged => self.run_operation(GitAction::Stage { paths }),
        }
    }

    fn request_discard(&mut self, index: usize) {
        let Some(entry) = self.entry(index).cloned() else {
            return;
        };
        self.dialog = Some(Dialog::Confirm {
            title: "Descartar cambios".to_owned(),
            message: format!("¿Descartar los cambios en {}?", entry.display_path()),
            action: PendingAction::Discard {
                paths: entry_paths(&entry),
                untracked: entry.untracked,
            },
        });
        self.window.request_redraw();
    }

    fn checkout_ref(&mut self, target: RefTarget) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        match target {
            RefTarget::Local(index) => {
                let Some(branch) = snapshot.refs.local_branches.get(index) else {
                    return;
                };
                if branch.current {
                    return;
                }
                self.run_operation(GitAction::Checkout {
                    branch: branch.name.clone(),
                    track: false,
                    local: branch.name.clone(),
                });
            }
            RefTarget::Remote(index) => {
                let Some(branch) = snapshot.refs.remote_branches.get(index) else {
                    return;
                };
                let local_name = branch
                    .name
                    .rsplit_once('/')
                    .map(|(_, name)| name)
                    .unwrap_or(&branch.name)
                    .to_owned();
                let exists = snapshot
                    .refs
                    .local_branches
                    .iter()
                    .any(|local| local.name == local_name);
                if exists {
                    self.run_operation(GitAction::Checkout {
                        branch: local_name.clone(),
                        track: false,
                        local: local_name,
                    });
                } else {
                    self.run_operation(GitAction::Checkout {
                        branch: branch.name.clone(),
                        track: true,
                        local: local_name,
                    });
                }
            }
            RefTarget::Stash(index) => {
                if let Some(stash) = snapshot.refs.stashes.get(index) {
                    self.run_operation(GitAction::StashPop { index: stash.index });
                }
            }
            RefTarget::Tag(_) => {}
        }
    }

    fn merge_ref(&mut self, target: RefTarget) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let name = match target {
            RefTarget::Local(index) => snapshot.refs.local_branches.get(index).map(|b| &b.name),
            RefTarget::Remote(index) => snapshot.refs.remote_branches.get(index).map(|b| &b.name),
            _ => None,
        };
        let Some(name) = name else {
            return;
        };
        if snapshot.status.branch.as_deref() == Some(name) {
            return;
        }
        self.run_operation(GitAction::Merge {
            branch: name.clone(),
        });
    }

    fn delete_ref(&mut self, target: RefTarget) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let RefTarget::Local(index) = target else {
            return;
        };
        let Some(branch) = snapshot.refs.local_branches.get(index) else {
            return;
        };
        if branch.current {
            return;
        }
        self.dialog = Some(Dialog::Confirm {
            title: "Borrar rama".to_owned(),
            message: format!("¿Borrar la rama {}?", branch.name),
            action: PendingAction::DeleteBranch {
                name: branch.name.clone(),
            },
        });
        self.window.request_redraw();
    }

    fn stash_pop(&mut self, target: RefTarget) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let RefTarget::Stash(index) = target else {
            return;
        };
        if let Some(stash) = snapshot.refs.stashes.get(index) {
            self.run_operation(GitAction::StashPop { index: stash.index });
        }
    }

    fn stash_drop(&mut self, target: RefTarget) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let RefTarget::Stash(index) = target else {
            return;
        };
        let Some(stash) = snapshot.refs.stashes.get(index) else {
            return;
        };
        self.dialog = Some(Dialog::Confirm {
            title: "Borrar stash".to_owned(),
            message: format!("¿Borrar {}?", stash.name),
            action: PendingAction::DropStash { index: stash.index },
        });
        self.window.request_redraw();
    }

    fn request_new_branch(&mut self) {
        self.dialog = Some(Dialog::Input {
            title: "Nueva rama".to_owned(),
            prompt: "Nombre de la rama".to_owned(),
            value: String::new(),
            action: InputAction::NewBranch,
        });
        self.window.request_redraw();
    }

    fn commit(&mut self) {
        if self.message.trim().is_empty() {
            return;
        }
        let staged = self
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.status.staged_count());
        if staged == 0 {
            return;
        }
        self.run_operation(GitAction::Commit {
            message: self.message.clone(),
            amend: self.amend,
        });
    }

    fn push(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let Some(branch) = snapshot.status.branch.clone() else {
            return;
        };
        self.run_operation(GitAction::Push {
            branch,
            set_upstream: snapshot.status.upstream.is_none(),
        });
    }

    fn stash_save(&mut self) {
        let has_changes = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.status.entries.is_empty());
        if !has_changes {
            return;
        }
        self.run_operation(GitAction::StashSave {
            message: self.message.trim().to_owned(),
            include_untracked: true,
        });
    }

    fn confirm_dialog(&mut self) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        match dialog {
            Dialog::Confirm { action, .. } => match action {
                PendingAction::Discard { paths, untracked } => {
                    self.run_operation(GitAction::Discard { paths, untracked });
                }
                PendingAction::DeleteBranch { name } => {
                    self.run_operation(GitAction::DeleteBranch { name, force: false });
                }
                PendingAction::DropStash { index } => {
                    self.run_operation(GitAction::StashDrop { index });
                }
            },
            Dialog::Input { value, action, .. } => {
                if value.trim().is_empty() {
                    return;
                }
                match action {
                    InputAction::NewBranch => {
                        self.run_operation(GitAction::CreateBranch {
                            name: value.trim().to_owned(),
                            checkout: true,
                        });
                    }
                }
            }
        }
        self.window.request_redraw();
    }

    fn cancel_dialog(&mut self) {
        self.dialog = None;
        self.window.request_redraw();
    }

    fn on_ime(&mut self, text: &str) {
        if text.chars().any(char::is_control) {
            return;
        }
        match &mut self.dialog {
            Some(Dialog::Input { value, .. }) => {
                value.push_str(text);
                self.window.request_redraw();
            }
            Some(Dialog::Confirm { .. }) => {}
            None if self.focus == Focus::Message => {
                self.message.push_str(text);
                self.window.request_redraw();
            }
            None => {}
        }
    }

    fn on_key(&mut self, event: KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }

        let command = self.modifiers.super_key() || self.modifiers.control_key();

        if self.dialog.is_some() {
            self.on_dialog_key(event);
            return;
        }

        if command && matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyW)) {
            self.close_requested = true;
            return;
        }

        if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
            if self.focus == Focus::Message {
                self.focus = if self.left_tab == LeftTab::Changes {
                    Focus::Changes
                } else {
                    Focus::Refs
                };
                self.window.request_redraw();
            } else {
                self.close_requested = true;
            }
            return;
        }

        if matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
            self.focus = match self.focus {
                Focus::Changes | Focus::Refs => Focus::History,
                Focus::History => Focus::Detail,
                Focus::Detail => Focus::Message,
                Focus::Message => {
                    if self.left_tab == LeftTab::Changes {
                        Focus::Changes
                    } else {
                        Focus::Refs
                    }
                }
            };
            self.window.request_redraw();
            return;
        }

        if self.focus == Focus::Message {
            self.on_message_key(event, command);
            return;
        }

        let layout = self.layout();
        match self.focus {
            Focus::Changes => match &event.logical_key {
                Key::Named(NamedKey::ArrowUp) => self.move_change(&layout, -1),
                Key::Named(NamedKey::ArrowDown) => self.move_change(&layout, 1),
                Key::Named(NamedKey::PageUp) => {
                    self.move_change(&layout, -(layout.left_visible as isize));
                }
                Key::Named(NamedKey::PageDown) => {
                    self.move_change(&layout, layout.left_visible as isize);
                }
                Key::Named(NamedKey::Home) => {
                    if let Some((list, index)) = self.change_order().first().copied() {
                        self.select_change(list, index);
                    }
                }
                Key::Named(NamedKey::End) => {
                    if let Some((list, index)) = self.change_order().last().copied() {
                        self.select_change(list, index);
                    }
                }
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                    if let Some((list, index)) = self.selected_change {
                        self.toggle_staged(list, index);
                    }
                }
                Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                    if let Some((list, index)) = self.selected_change
                        && list == ListKind::Unstaged
                    {
                        self.request_discard(index);
                    }
                }
                Key::Character(character) if character.as_str() == "r" && !command => {
                    self.start_snapshot();
                }
                _ => {}
            },
            Focus::Refs => match &event.logical_key {
                Key::Named(NamedKey::ArrowUp) => self.move_ref(&layout, -1),
                Key::Named(NamedKey::ArrowDown) => self.move_ref(&layout, 1),
                Key::Named(NamedKey::PageUp) => {
                    self.move_ref(&layout, -(layout.left_visible as isize));
                }
                Key::Named(NamedKey::PageDown) => {
                    self.move_ref(&layout, layout.left_visible as isize);
                }
                Key::Named(NamedKey::Home) => {
                    if let Some(target) = self.ref_targets().first().copied() {
                        self.select_ref(target);
                    }
                }
                Key::Named(NamedKey::End) => {
                    if let Some(target) = self.ref_targets().last().copied() {
                        self.select_ref(target);
                    }
                }
                Key::Named(NamedKey::Enter) => {
                    if let Some(target) = self.selected_ref {
                        self.checkout_ref(target);
                    }
                }
                Key::Character(character) if !command => match character.as_str() {
                    "n" => self.request_new_branch(),
                    "m" => {
                        if let Some(target) = self.selected_ref {
                            self.merge_ref(target);
                        }
                    }
                    "d" => {
                        if let Some(target) = self.selected_ref {
                            self.delete_ref(target);
                        }
                    }
                    "r" => self.start_snapshot(),
                    _ => {}
                },
                _ => {}
            },
            Focus::History => match &event.logical_key {
                Key::Named(NamedKey::ArrowUp) => {
                    self.select_history(self.selected_history.saturating_sub(1));
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.select_history(self.selected_history + 1);
                }
                Key::Named(NamedKey::PageUp) => {
                    self.select_history(
                        self.selected_history.saturating_sub(layout.history_visible),
                    );
                }
                Key::Named(NamedKey::PageDown) => {
                    self.select_history(self.selected_history + layout.history_visible);
                }
                Key::Named(NamedKey::Home) => self.select_history(0),
                Key::Named(NamedKey::End) => self.select_history(self.commit_count()),
                Key::Character(character) if character.as_str() == "r" && !command => {
                    self.start_snapshot();
                }
                _ => {}
            },
            Focus::Detail => {
                let file_count = self.detail.as_ref().map_or(0, |detail| detail.files.len());
                let diff_rows = self
                    .detail
                    .as_ref()
                    .and_then(|detail| detail.files.get(self.selected_file))
                    .map_or(0, |file| file.lines.len());
                let max_row = diff_rows.saturating_sub(layout.diff_visible);

                match &event.logical_key {
                    Key::Named(NamedKey::ArrowUp) => {
                        self.selected_file = self.selected_file.saturating_sub(1);
                        self.file_offset = self.file_offset.min(self.selected_file);
                        self.scroll_row = 0;
                        self.window.request_redraw();
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if file_count > 0 {
                            self.selected_file = (self.selected_file + 1).min(file_count - 1);
                            self.scroll_row = 0;
                            self.window.request_redraw();
                        }
                    }
                    Key::Named(NamedKey::PageUp) => {
                        self.scroll_row = self.scroll_row.saturating_sub(layout.diff_visible);
                        self.window.request_redraw();
                    }
                    Key::Named(NamedKey::PageDown) => {
                        self.scroll_row = (self.scroll_row + layout.diff_visible).min(max_row);
                        self.window.request_redraw();
                    }
                    Key::Named(NamedKey::Home) => {
                        self.scroll_row = 0;
                        self.window.request_redraw();
                    }
                    Key::Named(NamedKey::End) => {
                        self.scroll_row = max_row;
                        self.window.request_redraw();
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        self.scroll_col = self.scroll_col.saturating_sub(4);
                        self.window.request_redraw();
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        self.scroll_col = (self.scroll_col + 4).min(4000);
                        self.window.request_redraw();
                    }
                    _ => {}
                }
            }
            Focus::Message => {}
        }
    }

    fn on_message_key(&mut self, event: KeyEvent, command: bool) {
        match &event.logical_key {
            Key::Named(NamedKey::Space) => {
                self.message.push(' ');
                self.window.request_redraw();
            }
            Key::Named(NamedKey::Backspace) => {
                self.message.pop();
                self.window.request_redraw();
            }
            Key::Named(NamedKey::Enter) => self.commit(),
            Key::Character(character) if !command && !self.modifiers.alt_key() => {
                let text = character.as_str();
                if !text.is_empty() && text.chars().all(|ch| !ch.is_control()) {
                    self.message.push_str(text);
                    self.window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn on_dialog_key(&mut self, event: KeyEvent) {
        let mut confirm = false;
        let mut cancel = false;

        if let Some(dialog) = &mut self.dialog {
            match dialog {
                Dialog::Confirm { .. } => match event.logical_key {
                    Key::Named(NamedKey::Enter) => confirm = true,
                    Key::Named(NamedKey::Escape) => cancel = true,
                    _ => {}
                },
                Dialog::Input { value, .. } => match event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        if !value.trim().is_empty() {
                            confirm = true;
                        }
                    }
                    Key::Named(NamedKey::Escape) => cancel = true,
                    Key::Named(NamedKey::Backspace) => {
                        value.pop();
                    }
                    Key::Named(NamedKey::Space) => value.push(' '),
                    Key::Character(character) => {
                        let text = character.as_str();
                        if !text.is_empty() && text.chars().all(|ch| !ch.is_control()) {
                            value.push_str(text);
                        }
                    }
                    _ => {}
                },
            }
        }

        if cancel {
            self.cancel_dialog();
        } else if confirm {
            self.confirm_dialog();
        } else {
            self.window.request_redraw();
        }
    }

    fn move_change(&mut self, _layout: &Layout, delta: isize) {
        let order = self.change_order();
        if order.is_empty() {
            return;
        }
        let current = self
            .selected_change
            .and_then(|selected| order.iter().position(|item| *item == selected))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, order.len() as isize - 1) as usize;
        let (list, index) = order[next];
        self.select_change(list, index);
    }

    fn move_ref(&mut self, _layout: &Layout, delta: isize) {
        let targets = self.ref_targets();
        if targets.is_empty() {
            return;
        }
        let current = self
            .selected_ref
            .and_then(|selected| targets.iter().position(|target| *target == selected))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, targets.len() as isize - 1) as usize;
        self.select_ref(targets[next]);
    }

    fn on_mouse(&mut self, state: ElementState, button: MouseButton, _config: &Config) {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return;
        }

        let layout = self.layout();
        let x = self.cursor.x as f32;
        let y = self.cursor.y as f32;

        if self.dialog.is_some() {
            if let Some(panel) = layout.dialog_panel
                && !panel.contains(x, y)
            {
                return;
            }
            if let Some((confirm, rect)) = layout.dialog_buttons.first()
                && rect.contains(x, y)
            {
                if *confirm {
                    if let Some(Dialog::Input { value, .. }) = &self.dialog
                        && value.trim().is_empty()
                    {
                        return;
                    }
                    self.confirm_dialog();
                } else {
                    self.cancel_dialog();
                }
            } else if let Some((_, rect)) = layout.dialog_buttons.get(1)
                && rect.contains(x, y)
            {
                self.cancel_dialog();
            }
            return;
        }

        if layout.close.contains(x, y) {
            self.close_requested = true;
            return;
        }

        for (action, rect, enabled) in &layout.header_buttons {
            if *enabled && rect.contains(x, y) {
                match action {
                    HeaderAction::Fetch => self.run_operation(GitAction::Fetch),
                    HeaderAction::Pull => {
                        if self
                            .snapshot
                            .as_ref()
                            .is_some_and(|snapshot| snapshot.status.upstream.is_some())
                        {
                            self.run_operation(GitAction::Pull);
                        } else {
                            self.toast = Some((
                                "No upstream configured for this branch".to_owned(),
                                true,
                                Instant::now(),
                            ));
                            self.window.request_redraw();
                        }
                    }
                    HeaderAction::Push => self.push(),
                    HeaderAction::Refresh => self.start_snapshot(),
                }
                return;
            }
        }

        if layout.tabs[0].contains(x, y) {
            self.left_tab = LeftTab::Changes;
            self.focus = Focus::Changes;
            self.window.request_redraw();
            return;
        }
        if layout.tabs[1].contains(x, y) {
            self.left_tab = LeftTab::Refs;
            self.focus = Focus::Refs;
            self.window.request_redraw();
            return;
        }

        if layout.list_area.contains(x, y) {
            if self.left_tab == LeftTab::Changes {
                if let Some((row, rect)) = layout
                    .change_rows
                    .iter()
                    .find(|(_, rect)| rect.contains(x, y))
                {
                    match row {
                        ChangeRowKind::Section(_) => {}
                        ChangeRowKind::Entry(list, index) => {
                            let scale = self.renderer.scale_factor() as f32;
                            if *list == ListKind::Staged {
                                if x >= rect.right() - 24.0 * scale {
                                    self.toggle_staged(*list, *index);
                                } else {
                                    self.select_change(*list, *index);
                                }
                            } else if x >= rect.right() - 24.0 * scale {
                                self.request_discard(*index);
                            } else if x >= rect.right() - 48.0 * scale {
                                self.toggle_staged(*list, *index);
                            } else {
                                self.select_change(*list, *index);
                            }
                        }
                    }
                }
            } else if let Some((row, _)) =
                layout.ref_rows.iter().find(|(_, rect)| rect.contains(x, y))
                && let RefRowKind::Target(target) = row
            {
                let now = Instant::now();
                let double = self.last_ref_click.is_some_and(|(time, last)| {
                    last == *target && now.duration_since(time) < Duration::from_millis(400)
                });
                self.select_ref(*target);
                if double {
                    self.last_ref_click = None;
                    self.activate_ref(*target);
                } else {
                    self.last_ref_click = Some((now, *target));
                }
            }
            return;
        }

        for (action, rect, enabled) in &layout.left_actions {
            if *enabled && rect.contains(x, y) {
                match action {
                    LeftAction::StageAll => self.run_operation(GitAction::StageAll),
                    LeftAction::UnstageAll => self.run_operation(GitAction::UnstageAll),
                    LeftAction::AbortMerge => self.run_operation(GitAction::AbortMerge),
                    LeftAction::NewBranch => self.request_new_branch(),
                    LeftAction::Checkout => {
                        if let Some(target) = self.selected_ref {
                            self.checkout_ref(target);
                        }
                    }
                    LeftAction::Merge => {
                        if let Some(target) = self.selected_ref {
                            self.merge_ref(target);
                        }
                    }
                    LeftAction::Delete => {
                        if let Some(target) = self.selected_ref {
                            self.delete_ref(target);
                        }
                    }
                    LeftAction::StashPop => {
                        if let Some(target) = self.selected_ref {
                            self.stash_pop(target);
                        }
                    }
                    LeftAction::StashDrop => {
                        if let Some(target) = self.selected_ref {
                            self.stash_drop(target);
                        }
                    }
                }
                return;
            }
        }

        if layout.center.contains(x, y) {
            if let Some((index, _)) = layout
                .history_rows
                .iter()
                .find(|(_, rect)| rect.contains(x, y))
            {
                self.select_history(*index);
            } else {
                self.focus = Focus::History;
                self.window.request_redraw();
            }
            return;
        }

        if layout.right.contains(x, y) {
            if let Some(files) = layout.file_list
                && files.contains(x, y)
                && let Some((index, _)) = layout
                    .file_rows
                    .iter()
                    .find(|(_, rect)| rect.contains(x, y))
            {
                self.selected_file = *index;
                self.scroll_row = 0;
                self.scroll_col = 0;
                self.focus = Focus::Detail;
                self.window.request_redraw();
                return;
            }
            if layout.diff_area.is_some_and(|area| area.contains(x, y)) {
                self.focus = Focus::Detail;
                self.window.request_redraw();
                return;
            }
            self.focus = Focus::Detail;
            self.window.request_redraw();
            return;
        }

        if layout.message.contains(x, y) {
            self.focus = Focus::Message;
            self.window.request_redraw();
            return;
        }

        for (action, rect, enabled) in &layout.panel_buttons {
            if *enabled && rect.contains(x, y) {
                match action {
                    PanelAction::Commit => self.commit(),
                    PanelAction::Amend => {
                        self.amend = !self.amend;
                        self.window.request_redraw();
                    }
                    PanelAction::Stash => self.stash_save(),
                }
                return;
            }
        }
    }

    fn activate_ref(&mut self, target: RefTarget) {
        match target {
            RefTarget::Stash(_) => self.stash_pop(target),
            _ => self.checkout_ref(target),
        }
    }

    fn on_wheel(&mut self, delta: MouseScrollDelta, _config: &Config) {
        let layout = self.layout();
        let x = self.cursor.x as f32;
        let y = self.cursor.y as f32;
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => (y * 3.0).round() as isize,
            MouseScrollDelta::PixelDelta(position) => {
                (position.y as f32 / self.renderer.cell_metrics().height).round() as isize
            }
        };
        if steps == 0 {
            return;
        }

        if layout.list_area.contains(x, y) {
            if self.left_tab == LeftTab::Changes {
                let max = self.change_row_count().saturating_sub(layout.left_visible);
                self.changes_offset =
                    (self.changes_offset as isize - steps).clamp(0, max as isize) as usize;
            } else {
                let max = self.ref_row_count().saturating_sub(layout.left_visible);
                self.refs_offset =
                    (self.refs_offset as isize - steps).clamp(0, max as isize) as usize;
            }
        } else if layout.center.contains(x, y) {
            let max = (self.commit_count() + 1).saturating_sub(layout.history_visible);
            self.history_offset =
                (self.history_offset as isize - steps).clamp(0, max as isize) as usize;
        } else if let Some(files) = layout.file_list
            && files.contains(x, y)
        {
            let count = self.detail.as_ref().map_or(0, |detail| detail.files.len());
            let max = count.saturating_sub(layout.files_visible);
            self.file_offset = (self.file_offset as isize - steps).clamp(0, max as isize) as usize;
        } else if let Some(diff) = layout.diff_area
            && diff.contains(x, y)
        {
            let total = self
                .detail
                .as_ref()
                .and_then(|detail| detail.files.get(self.selected_file))
                .map_or(0, |file| file.lines.len());
            let max = total.saturating_sub(layout.diff_visible) as isize;
            self.scroll_row = (self.scroll_row as isize - steps).clamp(0, max) as usize;
        }

        self.window.request_redraw();
    }

    fn change_row_count(&self) -> usize {
        let snapshot = match &self.snapshot {
            Some(snapshot) => snapshot,
            None => return 0,
        };
        let staged = snapshot.status.staged_count();
        let unstaged = snapshot.status.unstaged_count();
        2 + staged + unstaged
    }

    fn ref_row_count(&self) -> usize {
        let Some(snapshot) = &self.snapshot else {
            return 0;
        };
        let sections = if snapshot.refs.stashes.is_empty() {
            3
        } else {
            4
        };
        sections
            + snapshot.refs.local_branches.len()
            + snapshot.refs.remote_branches.len()
            + snapshot.refs.tags.len()
            + snapshot.refs.stashes.len()
    }

    fn layout(&mut self) -> Layout {
        let size = self.window.inner_size();
        let width = size.width.max(1) as f32;
        let height = size.height.max(1) as f32;
        let scale = self.renderer.scale_factor() as f32;
        let metrics = self.renderer.cell_metrics();
        let padding = (12.0 * scale).round();

        let header = Rect::new(0.0, 0.0, width, (54.0 * scale).round());
        let footer_height = (24.0 * scale).round();
        let commit_height = (76.0 * scale).round();
        let footer = Rect::new(0.0, (height - footer_height).max(0.0), width, footer_height);
        let commit_panel = Rect::new(
            0.0,
            (footer.y - commit_height).max(header.bottom()),
            width,
            commit_height,
        );
        let body = Rect::new(
            0.0,
            header.bottom(),
            width,
            (commit_panel.y - header.bottom()).max(1.0),
        );

        let left_width = (300.0 * scale).min(body.width * 0.34).max(240.0 * scale);
        let left = Rect::new(body.x, body.y, left_width, body.height);
        let min_center = (260.0 * scale).min(body.width * 0.25);
        let max_right = (body.width - left_width - min_center).max(1.0);
        let right_width = (body.width * 0.40).clamp(1.0, max_right);
        let center = Rect::new(
            left.right() + 1.0,
            body.y,
            (body.right() - left.right() - 1.0 - right_width - 1.0).max(1.0),
            body.height,
        );
        let right = Rect::new(
            center.right() + 1.0,
            body.y,
            (body.right() - center.right() - 1.0).max(1.0),
            body.height,
        );

        let close_size = (20.0 * scale).round();
        let close = Rect::new(
            width - padding - close_size,
            (header.height - close_size) / 2.0,
            close_size,
            close_size,
        );

        let button_height = (26.0 * scale).round();
        let button_y = (header.height - button_height) / 2.0;
        let gap = 6.0 * scale;
        let refresh_width = 78.0 * scale;
        let push_width = 54.0 * scale;
        let pull_width = 54.0 * scale;
        let fetch_width = 60.0 * scale;
        let refresh = Rect::new(
            close.x - gap - refresh_width,
            button_y,
            refresh_width,
            button_height,
        );
        let push = Rect::new(
            refresh.x - gap - push_width,
            button_y,
            push_width,
            button_height,
        );
        let pull = Rect::new(
            push.x - gap - pull_width,
            button_y,
            pull_width,
            button_height,
        );
        let fetch = Rect::new(
            pull.x - gap - fetch_width,
            button_y,
            fetch_width,
            button_height,
        );

        let busy = self.busy.is_some();
        let has_snapshot = self.snapshot.is_some();
        let has_upstream = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.status.upstream.is_some());
        let has_branch = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.status.branch.is_some());
        let header_buttons = vec![
            (HeaderAction::Fetch, fetch, has_snapshot && !busy),
            (
                HeaderAction::Pull,
                pull,
                has_snapshot && has_upstream && !busy,
            ),
            (
                HeaderAction::Push,
                push,
                has_snapshot && has_branch && !busy,
            ),
            (HeaderAction::Refresh, refresh, has_snapshot && !busy),
        ];

        let tabs_height = (34.0 * scale).round();
        let half = left.width / 2.0;
        let tabs = [
            Rect::new(left.x, left.y, half, tabs_height),
            Rect::new(
                left.x + half,
                left.y,
                (left.width - half).max(1.0),
                tabs_height,
            ),
        ];

        let actions_height = (34.0 * scale).round();
        let actions = Rect::new(
            left.x,
            (left.bottom() - actions_height).max(left.y + tabs_height),
            left.width,
            actions_height,
        );
        let list_area = Rect::new(
            left.x,
            left.y + tabs_height,
            left.width,
            (actions.y - left.y - tabs_height).max(1.0),
        );

        let left_actions = self.build_left_actions(actions, scale);
        let (change_rows, left_visible) = self.build_change_rows(list_area, scale);
        let (ref_rows, ref_visible) = self.build_ref_rows(list_area, scale);

        let center_header_height = (30.0 * scale).round();
        let history_list = Rect::new(
            center.x,
            center.y + center_header_height,
            center.width,
            (center.height - center_header_height).max(1.0),
        );
        let (history_rows, history_visible) = self.build_history_rows(history_list, scale);

        let detail_header = Rect::new(
            right.x + padding,
            right.y + padding * 0.6,
            (right.width - padding * 2.0).max(1.0),
            (72.0 * scale).round(),
        );
        let detail = self.detail.as_ref();
        let files = detail.map(|_| {
            let file_h = (right.height * 0.30)
                .clamp(90.0 * scale, 210.0 * scale)
                .min((right.bottom() - detail_header.bottom() - 40.0 * scale).max(60.0 * scale));
            Rect::new(right.x, detail_header.bottom(), right.width, file_h)
        });
        let (file_rows, files_visible) = self.build_file_rows(files, scale);
        let (diff_area, diff_title) = files
            .map(|area| {
                let title = Rect::new(
                    area.x + padding,
                    area.bottom() + padding * 0.5,
                    (area.width - padding * 2.0).max(1.0),
                    (28.0 * scale).round(),
                );
                let text = Rect::new(
                    area.x + padding,
                    title.bottom(),
                    (area.width - padding * 2.0).max(1.0),
                    (right.bottom() - title.bottom() - padding).max(1.0),
                );
                (Some(text), Some(title))
            })
            .unwrap_or((None, None));
        let diff_visible = diff_area
            .map(|area| (area.height / metrics.height).floor().max(1.0) as usize)
            .unwrap_or(1);

        let panel_button_height = (30.0 * scale).round();
        let panel_button_y = commit_panel.y + (commit_panel.height - panel_button_height) / 2.0;
        let panel_gap = 6.0 * scale;
        let commit_width = 76.0 * scale;
        let amend_width = 70.0 * scale;
        let stash_width = 58.0 * scale;
        let commit_rect = Rect::new(
            width - padding - commit_width,
            panel_button_y,
            commit_width,
            panel_button_height,
        );
        let amend_rect = Rect::new(
            commit_rect.x - panel_gap - amend_width,
            panel_button_y,
            amend_width,
            panel_button_height,
        );
        let stash_rect = Rect::new(
            amend_rect.x - panel_gap - stash_width,
            panel_button_y,
            stash_width,
            panel_button_height,
        );
        let message = Rect::new(
            padding,
            panel_button_y,
            (stash_rect.x - panel_gap - padding).max(1.0),
            panel_button_height,
        );

        let staged = self
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.status.staged_count());
        let has_changes = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.status.entries.is_empty());
        let panel_buttons = vec![
            (
                PanelAction::Stash,
                stash_rect,
                has_snapshot && has_changes && !busy,
            ),
            (PanelAction::Amend, amend_rect, has_snapshot && !busy),
            (
                PanelAction::Commit,
                commit_rect,
                has_snapshot && !busy && staged > 0 && !self.message.trim().is_empty(),
            ),
        ];

        let (dialog_panel, dialog_buttons, dialog_input) = self.build_dialog_layout(scale);

        Layout {
            header,
            footer,
            left,
            center,
            right,
            tabs,
            close,
            list_area,
            header_buttons,
            left_actions,
            change_rows,
            ref_rows,
            history_rows,
            detail_header,
            file_list: files,
            file_rows,
            diff_area,
            diff_title,
            commit_panel,
            message,
            panel_buttons,
            dialog_buttons,
            dialog_panel,
            dialog_input,
            padding,
            left_visible: if self.left_tab == LeftTab::Changes {
                left_visible
            } else {
                ref_visible
            },
            history_visible,
            files_visible,
            diff_visible,
        }
    }

    fn build_left_actions(&self, area: Rect, scale: f32) -> Vec<(LeftAction, Rect, bool)> {
        let busy = self.busy.is_some();
        let mut actions = Vec::new();
        let height = (24.0 * scale).round();
        let y = area.y + (area.height - height) / 2.0;
        let gap = 5.0 * scale;
        let mut x = area.x + 8.0 * scale;

        let mut push = |action: LeftAction, label_width: f32, enabled: bool| {
            let rect = Rect::new(x, y, label_width, height);
            actions.push((action, rect, enabled));
            x += label_width + gap;
        };

        if self.left_tab == LeftTab::Changes {
            let has_unstaged = self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.status.unstaged_count() > 0);
            let has_staged = self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.status.staged_count() > 0);
            push(LeftAction::StageAll, 80.0 * scale, has_unstaged && !busy);
            push(LeftAction::UnstageAll, 88.0 * scale, has_staged && !busy);
            let has_conflicts = self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.status.entries.iter().any(|entry| entry.conflicted)
            });
            if has_conflicts {
                push(LeftAction::AbortMerge, 92.0 * scale, !busy);
            }
        } else {
            push(LeftAction::NewBranch, 48.0 * scale, !busy);
            if let Some(target) = self.selected_ref {
                match target {
                    RefTarget::Local(index) => {
                        let current = self
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.refs.local_branches.get(index))
                            .is_some_and(|branch| branch.current);
                        push(LeftAction::Checkout, 76.0 * scale, !current && !busy);
                        push(LeftAction::Merge, 56.0 * scale, !current && !busy);
                        push(LeftAction::Delete, 58.0 * scale, !current && !busy);
                    }
                    RefTarget::Remote(_) => {
                        push(LeftAction::Checkout, 76.0 * scale, !busy);
                    }
                    RefTarget::Stash(_) => {
                        push(LeftAction::StashPop, 42.0 * scale, !busy);
                        push(LeftAction::StashDrop, 48.0 * scale, !busy);
                    }
                    RefTarget::Tag(_) => {}
                }
            }
        }

        actions
    }

    fn build_change_rows(&mut self, area: Rect, scale: f32) -> (Vec<(ChangeRowKind, Rect)>, usize) {
        let row_height = (28.0 * scale).round();
        let visible = (area.height / row_height).floor().max(1.0) as usize;
        let mut rows = Vec::new();
        let Some(snapshot) = &self.snapshot else {
            return (rows, visible);
        };

        let mut kinds = Vec::new();
        kinds.push(ChangeRowKind::Section("STAGED"));
        kinds.extend(
            snapshot
                .status
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.staged)
                .map(|(index, _)| ChangeRowKind::Entry(ListKind::Staged, index)),
        );
        kinds.push(ChangeRowKind::Section("CHANGES"));
        kinds.extend(
            snapshot
                .status
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.unstaged)
                .map(|(index, _)| ChangeRowKind::Entry(ListKind::Unstaged, index)),
        );

        let selected = self.selected_change.and_then(|selected| {
            kinds
                .iter()
                .position(|kind| matches!(kind, ChangeRowKind::Entry(list, value) if (*list, *value) == selected))
        });
        if let Some(position) = selected {
            if position < self.changes_offset {
                self.changes_offset = position;
            }
            if position >= self.changes_offset + visible {
                self.changes_offset = position + 1 - visible;
            }
        }
        self.changes_offset = self.changes_offset.min(kinds.len().saturating_sub(visible));

        let start = self.changes_offset;
        let end = (start + visible).min(kinds.len());
        for (position, kind) in kinds.into_iter().enumerate().take(end).skip(start) {
            rows.push((
                kind,
                Rect::new(
                    area.x + 6.0 * scale,
                    area.y + (position - start) as f32 * row_height,
                    (area.width - 12.0 * scale).max(1.0),
                    row_height - 2.0,
                ),
            ));
        }

        (rows, visible)
    }

    fn build_ref_rows(&mut self, area: Rect, scale: f32) -> (Vec<(RefRowKind, Rect)>, usize) {
        let row_height = (28.0 * scale).round();
        let visible = (area.height / row_height).floor().max(1.0) as usize;
        let mut rows = Vec::new();
        let Some(snapshot) = &self.snapshot else {
            return (rows, visible);
        };

        let mut kinds = Vec::new();
        kinds.push(RefRowKind::Section("BRANCHES"));
        kinds.extend(
            (0..snapshot.refs.local_branches.len())
                .map(|index| RefRowKind::Target(RefTarget::Local(index))),
        );
        kinds.push(RefRowKind::Section("REMOTES"));
        kinds.extend(
            (0..snapshot.refs.remote_branches.len())
                .map(|index| RefRowKind::Target(RefTarget::Remote(index))),
        );
        kinds.push(RefRowKind::Section("TAGS"));
        kinds.extend(
            (0..snapshot.refs.tags.len()).map(|index| RefRowKind::Target(RefTarget::Tag(index))),
        );
        if !snapshot.refs.stashes.is_empty() {
            kinds.push(RefRowKind::Section("STASHES"));
            kinds.extend(
                (0..snapshot.refs.stashes.len())
                    .map(|index| RefRowKind::Target(RefTarget::Stash(index))),
            );
        }

        let selected = self.selected_ref.and_then(|target| {
            kinds
                .iter()
                .position(|kind| matches!(kind, RefRowKind::Target(value) if *value == target))
        });
        if let Some(position) = selected {
            if position < self.refs_offset {
                self.refs_offset = position;
            }
            if position >= self.refs_offset + visible {
                self.refs_offset = position + 1 - visible;
            }
        }
        self.refs_offset = self.refs_offset.min(kinds.len().saturating_sub(visible));

        let start = self.refs_offset;
        let end = (start + visible).min(kinds.len());
        for (position, kind) in kinds.into_iter().enumerate().take(end).skip(start) {
            rows.push((
                kind,
                Rect::new(
                    area.x + 6.0 * scale,
                    area.y + (position - start) as f32 * row_height,
                    (area.width - 12.0 * scale).max(1.0),
                    row_height - 2.0,
                ),
            ));
        }

        (rows, visible)
    }

    fn build_history_rows(&mut self, area: Rect, scale: f32) -> (Vec<(usize, Rect)>, usize) {
        let row_height = (38.0 * scale).round();
        let visible = (area.height / row_height).floor().max(1.0) as usize;
        let total = self.commit_count() + 1;

        if self.selected_history < self.history_offset {
            self.history_offset = self.selected_history;
        }
        if self.selected_history >= self.history_offset + visible {
            self.history_offset = self.selected_history + 1 - visible;
        }
        self.history_offset = self.history_offset.min(total.saturating_sub(visible));

        let start = self.history_offset;
        let end = (start + visible).min(total);
        let rows = (start..end)
            .map(|index| {
                (
                    index,
                    Rect::new(
                        area.x + 6.0 * scale,
                        area.y + (index - start) as f32 * row_height,
                        (area.width - 12.0 * scale).max(1.0),
                        row_height - 2.0,
                    ),
                )
            })
            .collect();

        (rows, visible)
    }

    fn build_file_rows(&mut self, area: Option<Rect>, scale: f32) -> (Vec<(usize, Rect)>, usize) {
        let Some(area) = area else {
            return (Vec::new(), 1);
        };
        let title_height = (24.0 * scale).round();
        let row_height = (24.0 * scale).round();
        let visible = ((area.height - title_height) / row_height).floor().max(1.0) as usize;
        let total = self.detail.as_ref().map_or(0, |detail| detail.files.len());

        if self.selected_file < self.file_offset {
            self.file_offset = self.selected_file;
        }
        if self.selected_file >= self.file_offset + visible {
            self.file_offset = self.selected_file + 1 - visible;
        }
        self.file_offset = self.file_offset.min(total.saturating_sub(visible));

        let start = self.file_offset;
        let end = (start + visible).min(total);
        let rows = (start..end)
            .map(|index| {
                (
                    index,
                    Rect::new(
                        area.x,
                        area.y + title_height + (index - start) as f32 * row_height,
                        area.width,
                        row_height - 1.0,
                    ),
                )
            })
            .collect();

        (rows, visible)
    }

    fn build_dialog_layout(&self, scale: f32) -> (Option<Rect>, Vec<(bool, Rect)>, Option<Rect>) {
        if self.dialog.is_none() {
            return (None, Vec::new(), None);
        }

        let size = self.window.inner_size();
        let width = size.width.max(1) as f32;
        let height = size.height.max(1) as f32;
        let panel_width = (460.0 * scale).min(width - 40.0 * scale);
        let panel_height = (190.0 * scale).min(height - 40.0 * scale);
        let panel = Rect::new(
            (width - panel_width) / 2.0,
            (height - panel_height) / 2.0,
            panel_width,
            panel_height,
        );

        let button_height = (30.0 * scale).round();
        let button_width = (82.0 * scale).round();
        let confirm = Rect::new(
            panel.right() - 12.0 * scale - button_width,
            panel.bottom() - 12.0 * scale - button_height,
            button_width,
            button_height,
        );
        let cancel = Rect::new(
            confirm.x - 8.0 * scale - button_width,
            confirm.y,
            button_width,
            button_height,
        );

        let input = matches!(self.dialog, Some(Dialog::Input { .. })).then(|| {
            Rect::new(
                panel.x + 14.0 * scale,
                panel.y + 92.0 * scale,
                panel.width - 28.0 * scale,
                (30.0 * scale).round(),
            )
        });

        (Some(panel), vec![(true, confirm), (false, cancel)], input)
    }

    fn draw(&mut self, config: &Config) {
        let theme = &config.theme;
        let scale = self.renderer.scale_factor() as f32;
        let metrics = self.renderer.cell_metrics();
        let ui_font = self.renderer.physical(config.ui_font_size);
        let layout = self.layout();

        let mut quads = Vec::new();
        let mut labels = Vec::new();

        quads.push(Quad::new(
            0.0,
            0.0,
            self.window.inner_size().width.max(1) as f32,
            self.window.inner_size().height.max(1) as f32,
            to_linear(theme.background, 1.0),
        ));

        if self.dialog.is_some() {
            self.draw_dialog(theme, scale, ui_font, &layout, &mut quads, &mut labels);
            let scene = Scene {
                panes: Vec::new(),
                labels,
                quads,
            };
            if let Err(error) = self.renderer.render(&scene, theme) {
                log::error!("git dialog render failed: {error:#}");
            }
            return;
        }

        self.draw_header(theme, scale, ui_font, &layout, &mut quads, &mut labels);
        self.draw_left(theme, scale, ui_font, &layout, &mut quads, &mut labels);
        self.draw_history(theme, scale, ui_font, &layout, &mut quads, &mut labels);
        let mut scene_panes = Vec::new();
        let mut diff_frame: Option<(Rect, Frame)> = None;
        let context = DrawContext {
            theme,
            scale,
            ui_font,
            metrics,
        };
        self.draw_detail(&context, &layout, &mut diff_frame, &mut quads, &mut labels);
        if let Some((area, frame)) = &diff_frame {
            scene_panes.push(PaneLayer {
                id: u64::MAX - 1,
                content: *area,
                frame,
                dirty: true,
            });
        }
        self.draw_commit_panel(theme, scale, ui_font, &layout, &mut quads, &mut labels);
        self.draw_footer(theme, scale, ui_font, &layout, &mut quads, &mut labels);

        let scene = Scene {
            panes: scene_panes,
            labels,
            quads,
        };
        if let Err(error) = self.renderer.render(&scene, theme) {
            log::error!("git window render failed: {error:#}");
        }
    }

    fn draw_header(
        &self,
        theme: &Theme,
        scale: f32,
        ui_font: f32,
        layout: &Layout,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
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

        labels.push(Label {
            text: "GIT".into(),
            x: layout.padding,
            y: layout.header.y + 7.0 * scale,
            width: 60.0 * scale,
            font_size: (ui_font * 1.05).max(14.0),
            color: theme.sidebar_text,
            bold: true,
        });

        let repo = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.repo.display().to_string())
            .unwrap_or_else(|| self.directory.display().to_string());
        labels.push(Label {
            text: repo,
            x: layout.padding + 56.0 * scale,
            y: layout.header.y + 8.0 * scale,
            width: (layout
                .header_buttons
                .first()
                .map_or(layout.close.x, |(_, rect, _)| rect.x)
                - layout.padding
                - 64.0 * scale)
                .max(1.0),
            font_size: ui_font * 0.82,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        let subtitle = match &self.snapshot {
            None => "loading repository…".to_owned(),
            Some(snapshot) => {
                let branch = snapshot.status.branch.clone().unwrap_or_else(|| {
                    if snapshot.status.detached {
                        "detached HEAD".to_owned()
                    } else {
                        "no branch".to_owned()
                    }
                });
                let mut value = branch;
                if let Some(upstream) = &snapshot.status.upstream {
                    value.push_str(&format!(
                        "  ·  {upstream}  ↑{} ↓{}",
                        snapshot.status.ahead, snapshot.status.behind
                    ));
                }
                value
            }
        };
        labels.push(Label {
            text: subtitle,
            x: layout.padding + 56.0 * scale,
            y: layout.header.y + 27.0 * scale,
            width: (layout
                .header_buttons
                .first()
                .map_or(layout.close.x, |(_, rect, _)| rect.x)
                - layout.padding
                - 64.0 * scale)
                .max(1.0),
            font_size: ui_font * 0.76,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        for (action, rect, enabled) in &layout.header_buttons {
            let text = match action {
                HeaderAction::Fetch => "Fetch",
                HeaderAction::Pull => "Pull",
                HeaderAction::Push => "Push",
                HeaderAction::Refresh => "Refresh",
            };
            push_button(
                quads,
                labels,
                *rect,
                text,
                *enabled,
                false,
                ButtonStyle {
                    theme,
                    scale,
                    font_size: ui_font * 0.78,
                },
            );
        }

        labels.push(Label {
            text: "×".into(),
            x: layout.close.x + (layout.close.width - ui_font * 0.55) / 2.0,
            y: layout.close.y + (layout.close.height - ui_font * 1.35) / 2.0,
            width: layout.close.width,
            font_size: ui_font * 1.1,
            color: theme.sidebar_text_dim,
            bold: false,
        });
    }

    fn draw_left(
        &self,
        theme: &Theme,
        scale: f32,
        ui_font: f32,
        layout: &Layout,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        quads.push(Quad::new(
            layout.left.x,
            layout.left.y,
            layout.left.width,
            layout.left.height,
            to_linear(theme.sidebar_background, 1.0),
        ));
        quads.push(Quad::new(
            layout.left.right() - 1.0,
            layout.left.y,
            1.0,
            layout.left.height,
            to_linear(theme.border, 1.0),
        ));

        let tabs = ["CHANGES", "REFS"];
        for (index, rect) in layout.tabs.iter().enumerate() {
            let active = (index == 0 && self.left_tab == LeftTab::Changes)
                || (index == 1 && self.left_tab == LeftTab::Refs);
            quads.push(Quad::new(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                to_linear(
                    if active {
                        theme.sidebar_active
                    } else {
                        theme.sidebar_background
                    },
                    1.0,
                ),
            ));
            if active {
                quads.push(Quad::new(
                    rect.x,
                    rect.bottom() - (2.0 * scale).max(1.0),
                    rect.width,
                    (2.0 * scale).max(1.0),
                    to_linear(theme.accent, 1.0),
                ));
            }
            labels.push(Label {
                text: tabs[index].into(),
                x: rect.x + 10.0 * scale,
                y: rect.y + 8.0 * scale,
                width: (rect.width - 14.0 * scale).max(1.0),
                font_size: ui_font * 0.76,
                color: if active {
                    theme.sidebar_text
                } else {
                    theme.sidebar_text_dim
                },
                bold: active,
            });
        }

        if self.left_tab == LeftTab::Changes {
            self.draw_change_rows(theme, scale, ui_font, layout, quads, labels);
        } else {
            self.draw_ref_rows(theme, scale, ui_font, layout, quads, labels);
        }

        for (action, rect, enabled) in &layout.left_actions {
            let text = match action {
                LeftAction::StageAll => "Stage all",
                LeftAction::UnstageAll => "Unstage all",
                LeftAction::AbortMerge => "Abort merge",
                LeftAction::NewBranch => "New",
                LeftAction::Checkout => "Checkout",
                LeftAction::Merge => "Merge",
                LeftAction::Delete => "Delete",
                LeftAction::StashPop => "Pop",
                LeftAction::StashDrop => "Drop",
            };
            push_button(
                quads,
                labels,
                *rect,
                text,
                *enabled,
                false,
                ButtonStyle {
                    theme,
                    scale,
                    font_size: ui_font * 0.72,
                },
            );
        }
    }

    fn draw_change_rows(
        &self,
        theme: &Theme,
        scale: f32,
        ui_font: f32,
        layout: &Layout,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };

        for (kind, rect) in &layout.change_rows {
            match kind {
                ChangeRowKind::Section(title) => {
                    let text = match *title {
                        "STAGED" => format!("STAGED {}", snapshot.status.staged_count()),
                        "CHANGES" => format!("CHANGES {}", snapshot.status.unstaged_count()),
                        _ => (*title).into(),
                    };
                    labels.push(Label {
                        text,
                        x: rect.x + 6.0 * scale,
                        y: rect.y + 6.0 * scale,
                        width: (rect.width - 12.0 * scale).max(1.0),
                        font_size: ui_font * 0.68,
                        color: theme.sidebar_text_dim,
                        bold: true,
                    });
                }
                ChangeRowKind::Entry(list, index) => {
                    let Some(entry) = snapshot.status.entries.get(*index) else {
                        continue;
                    };
                    let selected = self.selected_change == Some((*list, *index));
                    if selected {
                        quads.push(Quad::new(
                            rect.x,
                            rect.y,
                            rect.width,
                            rect.height,
                            to_linear(theme.sidebar_active, 1.0),
                        ));
                        quads.push(Quad::new(
                            rect.x,
                            rect.y,
                            2.0 * scale,
                            rect.height,
                            to_linear(theme.accent, 1.0),
                        ));
                    }

                    let color = change_color(entry.kind, theme);
                    labels.push(Label {
                        text: entry.kind.code().into(),
                        x: rect.x + 8.0 * scale,
                        y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                        width: 18.0 * scale,
                        font_size: ui_font * 0.82,
                        color,
                        bold: true,
                    });
                    labels.push(Label {
                        text: entry.display_path(),
                        x: rect.x + 28.0 * scale,
                        y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                        width: (rect.width - 88.0 * scale).max(1.0),
                        font_size: ui_font * 0.8,
                        color: theme.sidebar_text,
                        bold: selected,
                    });

                    if *list == ListKind::Staged {
                        labels.push(Label {
                            text: "−".into(),
                            x: rect.right() - 20.0 * scale,
                            y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                            width: 16.0 * scale,
                            font_size: ui_font,
                            color: theme.sidebar_text_dim,
                            bold: false,
                        });
                    } else {
                        labels.push(Label {
                            text: "+".into(),
                            x: rect.right() - 44.0 * scale,
                            y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                            width: 16.0 * scale,
                            font_size: ui_font,
                            color: theme.diff_add_text,
                            bold: true,
                        });
                        labels.push(Label {
                            text: "×".into(),
                            x: rect.right() - 20.0 * scale,
                            y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                            width: 16.0 * scale,
                            font_size: ui_font,
                            color: theme.sidebar_text_dim,
                            bold: false,
                        });
                    }
                }
            }
        }

        if snapshot.status.entries.is_empty() {
            labels.push(Label {
                text: "Working tree clean".into(),
                x: layout.list_area.x + layout.padding,
                y: layout.list_area.y + layout.padding,
                width: (layout.list_area.width - layout.padding * 2.0).max(1.0),
                font_size: ui_font * 0.82,
                color: theme.sidebar_text_dim,
                bold: false,
            });
        }
    }

    fn draw_ref_rows(
        &self,
        theme: &Theme,
        scale: f32,
        ui_font: f32,
        layout: &Layout,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };

        for (kind, rect) in &layout.ref_rows {
            match kind {
                RefRowKind::Section(title) => {
                    labels.push(Label {
                        text: (*title).into(),
                        x: rect.x + 6.0 * scale,
                        y: rect.y + 6.0 * scale,
                        width: (rect.width - 12.0 * scale).max(1.0),
                        font_size: ui_font * 0.68,
                        color: theme.sidebar_text_dim,
                        bold: true,
                    });
                }
                RefRowKind::Target(target) => {
                    let selected = self.selected_ref == Some(*target);
                    if selected {
                        quads.push(Quad::new(
                            rect.x,
                            rect.y,
                            rect.width,
                            rect.height,
                            to_linear(theme.sidebar_active, 1.0),
                        ));
                        quads.push(Quad::new(
                            rect.x,
                            rect.y,
                            2.0 * scale,
                            rect.height,
                            to_linear(theme.accent, 1.0),
                        ));
                    }

                    let (text, color, bold) = match *target {
                        RefTarget::Local(index) => {
                            let Some(branch) = snapshot.refs.local_branches.get(index) else {
                                continue;
                            };
                            let marker = if branch.current { "* " } else { "  " };
                            (
                                format!("{marker}{}", branch.name),
                                if branch.current {
                                    theme.accent
                                } else {
                                    theme.sidebar_text
                                },
                                branch.current || selected,
                            )
                        }
                        RefTarget::Remote(index) => {
                            let Some(branch) = snapshot.refs.remote_branches.get(index) else {
                                continue;
                            };
                            (
                                format!("  {}", branch.name),
                                theme.sidebar_text_dim,
                                selected,
                            )
                        }
                        RefTarget::Tag(index) => {
                            let Some(tag) = snapshot.refs.tags.get(index) else {
                                continue;
                            };
                            (format!("  {}", tag.name), theme.diff_hunk_text, selected)
                        }
                        RefTarget::Stash(index) => {
                            let Some(stash) = snapshot.refs.stashes.get(index) else {
                                continue;
                            };
                            (
                                format!("  {}  {}", stash.name, stash.message),
                                theme.sidebar_text,
                                selected,
                            )
                        }
                    };

                    labels.push(Label {
                        text,
                        x: rect.x + 8.0 * scale,
                        y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                        width: (rect.width - 14.0 * scale).max(1.0),
                        font_size: ui_font * 0.8,
                        color,
                        bold,
                    });
                }
            }
        }
    }

    fn draw_history(
        &self,
        theme: &Theme,
        scale: f32,
        ui_font: f32,
        layout: &Layout,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        quads.push(Quad::new(
            layout.center.x,
            layout.center.y,
            layout.center.width,
            layout.center.height,
            to_linear(theme.background, 1.0),
        ));
        quads.push(Quad::new(
            layout.center.x,
            layout.center.bottom() - 1.0,
            layout.center.width,
            1.0,
            to_linear(theme.border, 1.0),
        ));

        labels.push(Label {
            text: "HISTORY".into(),
            x: layout.center.x + layout.padding,
            y: layout.center.y + 8.0 * scale,
            width: (layout.center.width - layout.padding * 2.0).max(1.0),
            font_size: ui_font * 0.72,
            color: theme.sidebar_text_dim,
            bold: true,
        });

        for (index, rect) in &layout.history_rows {
            let selected = *index == self.selected_history;
            if selected {
                quads.push(Quad::new(
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    to_linear(theme.sidebar_active, 1.0),
                ));
                quads.push(Quad::new(
                    rect.x,
                    rect.y,
                    2.0 * scale,
                    rect.height,
                    to_linear(theme.accent, 1.0),
                ));
            }

            if *index == 0 {
                let (staged, unstaged) = self.snapshot.as_ref().map_or((0, 0), |snapshot| {
                    (
                        snapshot.status.staged_count(),
                        snapshot.status.unstaged_count(),
                    )
                });
                quads.push(Quad::rounded(
                    rect.x + 12.0 * scale,
                    rect.y + rect.height / 2.0 - 3.0 * scale,
                    6.0 * scale,
                    6.0 * scale,
                    3.0 * scale,
                    to_linear(theme.accent, 1.0),
                ));
                labels.push(Label {
                    text: "Working tree".into(),
                    x: rect.x + 26.0 * scale,
                    y: rect.y + 4.0 * scale,
                    width: (rect.width - 32.0 * scale).max(1.0),
                    font_size: ui_font * 0.84,
                    color: theme.sidebar_text,
                    bold: true,
                });
                labels.push(Label {
                    text: format!("{staged} staged · {unstaged} changes"),
                    x: rect.x + 26.0 * scale,
                    y: rect.y + 21.0 * scale,
                    width: (rect.width - 32.0 * scale).max(1.0),
                    font_size: ui_font * 0.7,
                    color: theme.sidebar_text_dim,
                    bold: false,
                });
                continue;
            }

            let Some(commit) = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.history.commits.get(index - 1))
            else {
                continue;
            };

            quads.push(Quad::rounded(
                rect.x + 12.0 * scale,
                rect.y + rect.height / 2.0 - 3.0 * scale,
                6.0 * scale,
                6.0 * scale,
                3.0 * scale,
                to_linear(theme.diff_hunk_text, 1.0),
            ));
            labels.push(Label {
                text: commit.short.clone(),
                x: rect.x + 24.0 * scale,
                y: rect.y + 4.0 * scale,
                width: 58.0 * scale,
                font_size: ui_font * 0.78,
                color: theme.accent,
                bold: false,
            });
            labels.push(Label {
                text: commit.subject.clone(),
                x: rect.x + 84.0 * scale,
                y: rect.y + 4.0 * scale,
                width: (rect.width - 90.0 * scale).max(1.0),
                font_size: ui_font * 0.82,
                color: theme.sidebar_text,
                bold: selected,
            });
            labels.push(Label {
                text: format!("{} · {}", commit.author, commit.date),
                x: rect.x + 84.0 * scale,
                y: rect.y + 21.0 * scale,
                width: (rect.width * 0.55).max(1.0),
                font_size: ui_font * 0.68,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            if !commit.refs.is_empty() {
                labels.push(Label {
                    text: commit.refs.join("  ·  "),
                    x: rect.x + rect.width * 0.6,
                    y: rect.y + 21.0 * scale,
                    width: (rect.width * 0.4 - 8.0 * scale).max(1.0),
                    font_size: ui_font * 0.68,
                    color: theme.diff_hunk_text,
                    bold: false,
                });
            }
        }
    }

    fn draw_detail(
        &self,
        context: &DrawContext<'_>,
        layout: &Layout,
        diff_frame: &mut Option<(Rect, Frame)>,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let DrawContext {
            theme,
            scale,
            ui_font,
            metrics,
        } = *context;

        quads.push(Quad::new(
            layout.right.x,
            layout.right.y,
            layout.right.width,
            layout.right.height,
            to_linear(theme.background, 1.0),
        ));
        quads.push(Quad::new(
            layout.right.x,
            layout.right.y,
            1.0,
            layout.right.height,
            to_linear(theme.border, 1.0),
        ));

        let (title, subtitle) = if self.selected_history == 0 {
            let (staged, unstaged) = self.snapshot.as_ref().map_or((0, 0), |snapshot| {
                (
                    snapshot.status.staged_count(),
                    snapshot.status.unstaged_count(),
                )
            });
            (
                "WORKING TREE".to_owned(),
                format!("{staged} staged · {unstaged} changes"),
            )
        } else if let Some(commit) = self.selected_commit() {
            (
                format!("#{} {}", commit.short, commit.subject),
                format!(
                    "{} · {}{}",
                    commit.author,
                    commit.date,
                    refs_suffix(&commit.refs)
                ),
            )
        } else {
            ("DETAIL".to_owned(), String::new())
        };

        labels.push(Label {
            text: title,
            x: layout.detail_header.x,
            y: layout.detail_header.y,
            width: layout.detail_header.width,
            font_size: ui_font * 0.9,
            color: theme.sidebar_text,
            bold: true,
        });
        labels.push(Label {
            text: subtitle,
            x: layout.detail_header.x,
            y: layout.detail_header.y + 22.0 * scale,
            width: layout.detail_header.width,
            font_size: ui_font * 0.72,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        if self.detail_loading {
            labels.push(Label {
                text: "Loading diff…".into(),
                x: layout.detail_header.x,
                y: layout.detail_header.y + 44.0 * scale,
                width: layout.detail_header.width,
                font_size: ui_font * 0.8,
                color: theme.sidebar_text_dim,
                bold: false,
            });
        } else if let Some(error) = &self.detail_error {
            labels.push(Label {
                text: error.clone(),
                x: layout.detail_header.x,
                y: layout.detail_header.y + 44.0 * scale,
                width: layout.detail_header.width,
                font_size: ui_font * 0.74,
                color: theme.diff_remove_text,
                bold: false,
            });
        }

        let Some(detail) = &self.detail else {
            if !self.detail_loading && self.detail_error.is_none() {
                labels.push(Label {
                    text: "Select a commit or a working-tree change".into(),
                    x: layout.right.x + layout.padding,
                    y: layout.right.y + 90.0 * scale,
                    width: (layout.right.width - layout.padding * 2.0).max(1.0),
                    font_size: ui_font * 0.82,
                    color: theme.sidebar_text_dim,
                    bold: false,
                });
            }
            return;
        };

        if let Some(files) = layout.file_list {
            labels.push(Label {
                text: format!("FILES · {}", detail.files.len()),
                x: files.x + layout.padding,
                y: files.y + 5.0 * scale,
                width: (files.width - layout.padding * 2.0).max(1.0),
                font_size: ui_font * 0.7,
                color: theme.sidebar_text_dim,
                bold: true,
            });

            for (index, rect) in &layout.file_rows {
                let Some(file) = detail.files.get(*index) else {
                    continue;
                };
                let selected = *index == self.selected_file;
                if selected {
                    quads.push(Quad::new(
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                        to_linear(theme.sidebar_active, 1.0),
                    ));
                    quads.push(Quad::new(
                        rect.x,
                        rect.y,
                        2.0 * scale,
                        rect.height,
                        to_linear(theme.accent, 1.0),
                    ));
                }
                labels.push(Label {
                    text: file_status_code(file.status).into(),
                    x: rect.x + 10.0 * scale,
                    y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                    width: 18.0 * scale,
                    font_size: ui_font * 0.76,
                    color: file_status_color(file.status, theme),
                    bold: true,
                });
                labels.push(Label {
                    text: file.path.clone(),
                    x: rect.x + 30.0 * scale,
                    y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                    width: (rect.width - 130.0 * scale).max(1.0),
                    font_size: ui_font * 0.76,
                    color: theme.sidebar_text,
                    bold: selected,
                });
                labels.push(Label {
                    text: format!("+{} −{}", file.additions, file.deletions),
                    x: rect.right() - 90.0 * scale,
                    y: rect.y + (rect.height - ui_font * 1.35) / 2.0,
                    width: 80.0 * scale,
                    font_size: ui_font * 0.7,
                    color: theme.sidebar_text_dim,
                    bold: false,
                });
            }
        }

        let Some(file) = detail.files.get(self.selected_file) else {
            return;
        };
        if let Some(title) = layout.diff_title {
            labels.push(Label {
                text: format!("{}  ·  +{} −{}", file.path, file.additions, file.deletions),
                x: title.x,
                y: title.y + 4.0 * scale,
                width: title.width,
                font_size: ui_font * 0.76,
                color: theme.sidebar_text,
                bold: true,
            });
        }

        if let Some(area) = layout.diff_area {
            quads.push(Quad::new(
                area.x - layout.padding,
                area.y,
                area.width + layout.padding * 2.0,
                area.height,
                to_linear(theme.background, 1.0),
            ));
            if file.binary {
                labels.push(Label {
                    text: "Binary file".into(),
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    font_size: ui_font,
                    color: theme.sidebar_text_dim,
                    bold: false,
                });
            } else if file.lines.is_empty() {
                labels.push(Label {
                    text: "No textual changes".into(),
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    font_size: ui_font,
                    color: theme.sidebar_text_dim,
                    bold: false,
                });
            } else {
                let frame = build_file_frame(
                    file,
                    area,
                    metrics,
                    theme,
                    self.scroll_row,
                    self.scroll_col,
                    quads,
                );
                *diff_frame = Some((area, frame));
            }
        }
    }

    fn draw_commit_panel(
        &self,
        theme: &Theme,
        scale: f32,
        ui_font: f32,
        layout: &Layout,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        quads.push(Quad::new(
            layout.commit_panel.x,
            layout.commit_panel.y,
            layout.commit_panel.width,
            layout.commit_panel.height,
            to_linear(theme.sidebar_background, 1.0),
        ));
        quads.push(Quad::new(
            layout.commit_panel.x,
            layout.commit_panel.y,
            layout.commit_panel.width,
            1.0,
            to_linear(theme.border, 1.0),
        ));

        quads.push(Quad::new(
            layout.message.x,
            layout.message.y,
            layout.message.width,
            layout.message.height,
            to_linear(theme.background, 1.0),
        ));
        push_outline(
            quads,
            layout.message,
            1.0,
            to_linear(
                if self.focus == Focus::Message {
                    theme.accent
                } else {
                    theme.border
                },
                1.0,
            ),
        );

        let text = if self.message.is_empty() {
            "Mensaje del commit…".to_owned()
        } else if self.focus == Focus::Message {
            format!("{}▌", self.message)
        } else {
            self.message.clone()
        };
        labels.push(Label {
            text,
            x: layout.message.x + 8.0 * scale,
            y: layout.message.y + (layout.message.height - ui_font * 1.35) / 2.0,
            width: (layout.message.width - 14.0 * scale).max(1.0),
            font_size: ui_font * 0.8,
            color: if self.message.is_empty() {
                theme.sidebar_text_dim
            } else {
                theme.sidebar_text
            },
            bold: false,
        });

        for (action, rect, enabled) in &layout.panel_buttons {
            let text = match action {
                PanelAction::Commit => "Commit",
                PanelAction::Amend => "Amend",
                PanelAction::Stash => "Stash",
            };
            push_button(
                quads,
                labels,
                *rect,
                text,
                *enabled,
                matches!(action, PanelAction::Commit)
                    || matches!(action, PanelAction::Amend) && self.amend,
                ButtonStyle {
                    theme,
                    scale,
                    font_size: ui_font * 0.76,
                },
            );
        }
    }

    fn draw_footer(
        &self,
        theme: &Theme,
        scale: f32,
        ui_font: f32,
        layout: &Layout,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        quads.push(Quad::new(
            layout.footer.x,
            layout.footer.y,
            layout.footer.width,
            layout.footer.height,
            to_linear(theme.sidebar_background, 1.0),
        ));
        quads.push(Quad::new(
            layout.footer.x,
            layout.footer.y,
            layout.footer.width,
            1.0,
            to_linear(theme.border, 1.0),
        ));

        let hint = match self.focus {
            Focus::Changes => {
                "↑↓ archivo · Enter stage/unstage · Supr descartar · R refrescar · Tab panel"
            }
            Focus::Refs => "↑↓ rama · Enter checkout · M merge · N nueva · D borrar · R refrescar",
            Focus::History => "↑↓ commit · PgUp/PgDn · R refrescar · Tab panel",
            Focus::Detail => "↑↓ archivo · PgUp/PgDn diff · ←→ horizontal · Tab panel",
            Focus::Message => "Enter commit · Esc salir · Cmd/Ctrl+W cerrar",
        };
        labels.push(Label {
            text: hint.into(),
            x: layout.footer.x + layout.padding,
            y: layout.footer.y + 4.0 * scale,
            width: (layout.footer.width * 0.6).max(1.0),
            font_size: ui_font * 0.7,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        let (text, color) = if let Some(busy) = &self.busy {
            (format!("{busy}…"), theme.accent)
        } else if let Some((message, is_error, _)) = &self.toast {
            (
                message.clone(),
                if *is_error {
                    theme.diff_remove_text
                } else {
                    theme.diff_add_text
                },
            )
        } else if let Some(error) = &self.error {
            (error.clone(), theme.diff_remove_text)
        } else {
            (String::new(), theme.sidebar_text_dim)
        };

        if !text.is_empty() {
            labels.push(Label {
                text,
                x: layout.footer.x + layout.footer.width * 0.6,
                y: layout.footer.y + 4.0 * scale,
                width: (layout.footer.width * 0.4 - layout.padding).max(1.0),
                font_size: ui_font * 0.7,
                color,
                bold: false,
            });
        }
    }

    fn draw_dialog(
        &self,
        theme: &Theme,
        scale: f32,
        ui_font: f32,
        layout: &Layout,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        quads.push(Quad::new(
            0.0,
            0.0,
            self.window.inner_size().width.max(1) as f32,
            self.window.inner_size().height.max(1) as f32,
            to_linear(theme.background, 1.0),
        ));

        let Some(panel) = layout.dialog_panel else {
            return;
        };
        quads.push(Quad::rounded(
            panel.x,
            panel.y,
            panel.width,
            panel.height,
            8.0 * scale,
            to_linear(theme.sidebar_background, 1.0),
        ));
        push_outline(quads, panel, 1.0, to_linear(theme.border, 1.0));

        let (title, message, input) = match &self.dialog {
            Some(Dialog::Confirm { title, message, .. }) => (title.clone(), message.clone(), None),
            Some(Dialog::Input {
                title,
                prompt,
                value,
                ..
            }) => (title.clone(), prompt.clone(), Some(value.clone())),
            None => return,
        };

        labels.push(Label {
            text: title,
            x: panel.x + 16.0 * scale,
            y: panel.y + 16.0 * scale,
            width: (panel.width - 32.0 * scale).max(1.0),
            font_size: ui_font * 0.95,
            color: theme.sidebar_text,
            bold: true,
        });
        labels.push(Label {
            text: message,
            x: panel.x + 16.0 * scale,
            y: panel.y + 52.0 * scale,
            width: (panel.width - 32.0 * scale).max(1.0),
            font_size: ui_font * 0.8,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        if let (Some(input_value), Some(input_rect)) = (input, layout.dialog_input) {
            quads.push(Quad::new(
                input_rect.x,
                input_rect.y,
                input_rect.width,
                input_rect.height,
                to_linear(theme.background, 1.0),
            ));
            push_outline(quads, input_rect, 1.0, to_linear(theme.accent, 1.0));
            labels.push(Label {
                text: if input_value.is_empty() {
                    "Nombre de la rama…▌".to_owned()
                } else {
                    format!("{input_value}▌")
                },
                x: input_rect.x + 8.0 * scale,
                y: input_rect.y + (input_rect.height - ui_font * 1.35) / 2.0,
                width: (input_rect.width - 14.0 * scale).max(1.0),
                font_size: ui_font * 0.82,
                color: if input_value.is_empty() {
                    theme.sidebar_text_dim
                } else {
                    theme.sidebar_text
                },
                bold: false,
            });
        }

        for (confirm, rect) in &layout.dialog_buttons {
            let text = if *confirm {
                if matches!(self.dialog, Some(Dialog::Input { .. })) {
                    "Crear"
                } else {
                    "Confirmar"
                }
            } else {
                "Cancelar"
            };
            push_button(
                quads,
                labels,
                *rect,
                text,
                true,
                *confirm,
                ButtonStyle {
                    theme,
                    scale,
                    font_size: ui_font * 0.78,
                },
            );
        }

        labels.push(Label {
            text: "Enter confirmar · Esc cancelar".into(),
            x: panel.x + 16.0 * scale,
            y: panel.bottom() - 62.0 * scale,
            width: (panel.width - 32.0 * scale).max(1.0),
            font_size: ui_font * 0.7,
            color: theme.sidebar_text_dim,
            bold: false,
        });
    }
}

fn entry_paths(entry: &StatusEntry) -> Vec<String> {
    let mut paths = vec![entry.path.clone()];
    if let Some(original) = &entry.original_path {
        paths.push(original.clone());
    }
    paths
}

fn file_status_code(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Modified => "M",
        FileStatus::Added => "A",
        FileStatus::Deleted => "D",
        FileStatus::Renamed => "R",
        FileStatus::Binary => "B",
    }
}

fn file_status_color(status: FileStatus, theme: &Theme) -> crate::term::color::Rgb {
    match status {
        FileStatus::Added => theme.diff_add_text,
        FileStatus::Deleted => theme.diff_remove_text,
        FileStatus::Renamed => theme.diff_hunk_text,
        FileStatus::Binary => theme.sidebar_text_dim,
        FileStatus::Modified => theme.diff_meta_text,
    }
}

fn change_color(kind: crate::git::ChangeKind, theme: &Theme) -> crate::term::color::Rgb {
    use crate::git::ChangeKind::*;
    match kind {
        Added | Untracked => theme.diff_add_text,
        Deleted => theme.diff_remove_text,
        Unmerged => theme.diff_remove_text,
        Renamed | Copied => theme.diff_hunk_text,
        Modified | TypeChanged => theme.diff_meta_text,
    }
}

fn refs_suffix(refs: &[String]) -> String {
    if refs.is_empty() {
        String::new()
    } else {
        format!(" · {}", refs.join("  ·  "))
    }
}

fn push_outline(quads: &mut Vec<Quad>, rect: Rect, thickness: f32, color: [f32; 4]) {
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

fn push_button(
    quads: &mut Vec<Quad>,
    labels: &mut Vec<Label>,
    rect: Rect,
    text: &str,
    enabled: bool,
    primary: bool,
    style: ButtonStyle<'_>,
) {
    let ButtonStyle {
        theme,
        scale,
        font_size,
    } = style;

    let background = if !enabled {
        theme.sidebar_background
    } else if primary {
        theme.accent
    } else {
        theme.sidebar_active
    };
    let color = if !enabled {
        theme.sidebar_text_dim
    } else if primary {
        theme.background
    } else {
        theme.sidebar_text
    };

    quads.push(Quad::rounded(
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        (rect.height / 2.0).min(6.0 * scale),
        to_linear(background, 1.0),
    ));
    labels.push(Label {
        text: text.to_owned(),
        x: rect.x + 8.0 * scale,
        y: rect.y + (rect.height - font_size * 1.35) / 2.0,
        width: (rect.width - 14.0 * scale).max(1.0),
        font_size,
        color,
        bold: false,
    });
}
