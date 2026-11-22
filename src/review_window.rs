use std::collections::HashSet;
use std::sync::Arc;

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::config::Config;
use crate::diff_view::build_file_frame;
use crate::event::UserEvent;
use crate::forge::{ForgeKind, ForgeTokens};
use crate::layout::Rect;
use crate::render::Renderer;
use crate::render::quads::Quad;
use crate::render::scene::{Label, PaneLayer, Scene};
use crate::review::{
    self, CodeReview, FileDiff, FileStatus, RemoteChange, ReviewDetail, ReviewSummary,
};
use crate::term::color::to_linear;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Reviews,
    Files,
    Diff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListMode {
    Local,
    PullRequests,
}

struct Layout {
    header: Rect,
    footer: Rect,
    close: Rect,
    reviews: Rect,
    tabs: [Rect; 2],
    search: Option<Rect>,
    list_area: Rect,
    files: Option<Rect>,
    diff: Option<Rect>,
    diff_title: Option<Rect>,
    diff_text: Option<Rect>,
    content: Rect,
    review_rows: Vec<(usize, Rect)>,
    file_rows: Vec<(usize, Rect)>,
    padding: f32,
    reviews_visible: usize,
    files_visible: usize,
    diff_visible: usize,
}

pub struct ReviewWindow {
    window: Arc<Window>,
    renderer: Renderer,
    review: CodeReview,
    proxy: EventLoopProxy<UserEvent>,
    modifiers: ModifiersState,
    cursor: PhysicalPosition<f64>,
    focus: Focus,
    mode: ListMode,
    search: String,
    search_focused: bool,
    selected_file: usize,
    file_offset: usize,
    pr_selected: usize,
    pr_offset: usize,
    reviewed: HashSet<String>,
    request_id: u64,
    prs_request_id: u64,
    close_requested: bool,
}

impl ReviewWindow {
    pub fn new(
        window: Arc<Window>,
        renderer: Renderer,
        directory: std::path::PathBuf,
        proxy: EventLoopProxy<UserEvent>,
        tokens: ForgeTokens,
    ) -> Self {
        let mut window = Self {
            window,
            renderer,
            review: CodeReview::open(directory, tokens),
            proxy,
            modifiers: ModifiersState::empty(),
            cursor: PhysicalPosition::new(0.0, 0.0),
            focus: Focus::Reviews,
            mode: ListMode::Local,
            search: String::new(),
            search_focused: false,
            selected_file: 0,
            file_offset: 0,
            pr_selected: 0,
            pr_offset: 0,
            reviewed: HashSet::new(),
            request_id: 0,
            prs_request_id: 0,
            close_requested: false,
        };
        window.start_prs_load();
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
            WindowEvent::KeyboardInput { event, .. } => self.on_key(event, config),
            WindowEvent::Ime(Ime::Commit(text)) => self.on_ime(&text),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse(state, button, config);
            }
            WindowEvent::MouseWheel { delta, .. } => self.on_wheel(delta, config),
            _ => {}
        }
    }

    pub fn apply_detail(
        &mut self,
        request_id: u64,
        result: std::result::Result<ReviewDetail, String>,
    ) {
        if request_id != self.request_id {
            return;
        }

        if let Ok(detail) = &result {
            log::info!(
                "review window loaded: {} files, +{} −{}",
                detail.files.len(),
                detail.additions,
                detail.deletions
            );
        }

        self.review.apply_detail(request_id, result);
        self.selected_file = 0;
        self.file_offset = 0;
        self.review.scroll_row = 0;
        self.review.scroll_col = 0;
        self.reviewed.clear();
        self.window.request_redraw();
    }

    pub fn apply_pull_requests(
        &mut self,
        request_id: u64,
        result: std::result::Result<Vec<RemoteChange>, String>,
    ) {
        if request_id != self.prs_request_id {
            return;
        }

        if let Ok(changes) = &result {
            log::info!("loaded {} remote change(s)", changes.len());
        }

        self.review.apply_pull_requests(result);
        self.pr_selected = 0;
        self.pr_offset = 0;
        self.window.request_redraw();
    }

    fn start_prs_load(&mut self) {
        if self.review.prs_loading {
            return;
        }

        self.prs_request_id = self.prs_request_id.wrapping_add(1);
        let request_id = self.prs_request_id;
        self.review.prs_loading = true;
        self.review.prs_error = None;

        let repo = self.review.repo.clone();
        let tokens = self.review.tokens.clone();
        let proxy = self.proxy.clone();
        let window_id = self.window.id();
        let spawned = std::thread::Builder::new()
            .name("pull-requests".into())
            .spawn(move || {
                let result = crate::forge::load_open_changes(&repo, &tokens)
                    .map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::PullRequestsLoaded {
                    window_id,
                    request_id,
                    result,
                });
            });

        if let Err(error) = spawned {
            self.review.prs_loading = false;
            self.review.prs_error = Some(format!("failed to start pull request loading: {error}"));
        }

        self.window.request_redraw();
    }

    fn filtered_prs(&self) -> Vec<usize> {
        let terms: Vec<String> = self
            .search
            .split_whitespace()
            .map(|term| term.to_lowercase())
            .collect();

        self.review
            .pull_requests
            .iter()
            .enumerate()
            .filter(|(_, pull)| {
                if terms.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {} {} {} {} {}",
                    pull.id_label(),
                    pull.id,
                    pull.title,
                    pull.author,
                    pull.source_branch,
                    pull.target_branch
                )
                .to_lowercase();
                terms.iter().all(|term| haystack.contains(term))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn item_count(&self) -> usize {
        match self.mode {
            ListMode::Local => self.review.reviews.len(),
            ListMode::PullRequests => self.filtered_prs().len(),
        }
    }

    fn selected_index(&self) -> usize {
        match self.mode {
            ListMode::Local => self.review.selected,
            ListMode::PullRequests => self.pr_selected,
        }
    }

    fn set_mode(&mut self, mode: ListMode) {
        if self.mode == mode {
            if mode == ListMode::PullRequests {
                self.focus = Focus::Reviews;
                self.search_focused = true;
                self.window.request_redraw();
            }
            return;
        }

        self.mode = mode;
        self.search_focused = false;
        self.review.back_to_list();
        self.reviewed.clear();
        self.selected_file = 0;
        self.file_offset = 0;

        if mode == ListMode::PullRequests {
            if !self.review.prs_loaded && !self.review.prs_loading {
                self.start_prs_load();
            }
            self.search_focused = true;
            self.pr_selected = 0;
            self.pr_offset = 0;
        }

        self.window.request_redraw();
    }

    fn reset_selection(&mut self) {
        self.review.back_to_list();
        self.reviewed.clear();
        self.selected_file = 0;
        self.file_offset = 0;
    }

    fn reset_pr_selection(&mut self) {
        self.pr_selected = 0;
        self.pr_offset = 0;
        self.reset_selection();
    }

    fn set_selected(&mut self, index: usize) {
        match self.mode {
            ListMode::Local => self.review.selected = index,
            ListMode::PullRequests => self.pr_selected = index,
        }
        self.reset_selection();
    }

    fn select_relative(&mut self, delta: isize) {
        let count = self.item_count();
        if count == 0 {
            return;
        }

        let current = self.selected_index().min(count - 1);
        let next = (current as isize + delta).clamp(0, count as isize - 1) as usize;
        self.set_selected(next);
    }

    fn selected_summary(&self) -> Option<ReviewSummary> {
        match self.mode {
            ListMode::Local => self.review.reviews.get(self.review.selected).cloned(),
            ListMode::PullRequests => {
                let filtered = self.filtered_prs();
                filtered
                    .get(self.pr_selected)
                    .and_then(|index| self.review.pull_requests.get(*index))
                    .map(RemoteChange::to_review)
            }
        }
    }

    fn start_selected(&mut self) {
        let Some(summary) = self.selected_summary() else {
            return;
        };
        self.start_detail(summary);
    }

    fn start_detail(&mut self, summary: ReviewSummary) {
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;
        let (repo, summary) = self.review.start_loading_summary(request_id, summary);

        let proxy = self.proxy.clone();
        let window_id = self.window.id();
        let tokens = self.review.tokens.clone();
        let spawned = std::thread::Builder::new()
            .name("code-review".into())
            .spawn(move || {
                let result = review::load_detail(&repo, &summary, &tokens)
                    .map_err(|error| format!("{error:#}"));
                let _ = proxy.send_event(UserEvent::ReviewLoaded {
                    window_id,
                    request_id,
                    result,
                });
            });

        if let Err(error) = spawned {
            self.review.loading = false;
            self.review.error = Some(format!("failed to start code review: {error}"));
        }

        self.window.request_redraw();
    }

    fn toggle_reviewed(&mut self, index: usize) {
        let Some(file) = self
            .review
            .detail
            .as_ref()
            .and_then(|detail| detail.files.get(index))
        else {
            return;
        };

        let path = file.path.clone();
        if !self.reviewed.remove(&path) {
            self.reviewed.insert(path);
        }
        self.window.request_redraw();
    }

    fn is_reviewed(&self, file: &FileDiff) -> bool {
        self.reviewed.contains(&file.path)
    }

    fn on_ime(&mut self, text: &str) {
        if !self.search_focused || self.focus != Focus::Reviews {
            return;
        }
        if text.chars().any(char::is_control) {
            return;
        }

        self.search.push_str(text);
        self.reset_pr_selection();
        self.window.request_redraw();
    }

    fn on_key(&mut self, event: KeyEvent, config: &Config) {
        if event.state != ElementState::Pressed {
            return;
        }

        let command = self.modifiers.super_key() || self.modifiers.control_key();
        let focusable_files = self
            .review
            .detail
            .as_ref()
            .is_some_and(|detail| !detail.files.is_empty());

        if matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyW)) && command {
            self.close_requested = true;
            return;
        }

        if matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyF)) && command {
            if self.mode != ListMode::PullRequests {
                self.set_mode(ListMode::PullRequests);
            }
            self.focus = Focus::Reviews;
            self.search_focused = true;
            self.window.request_redraw();
            return;
        }

        if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
            if self.search_focused {
                self.search_focused = false;
                self.window.request_redraw();
            } else {
                self.close_requested = true;
            }
            return;
        }

        if matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
            self.search_focused = false;
            self.focus = match self.focus {
                Focus::Reviews if focusable_files => Focus::Files,
                Focus::Reviews => Focus::Reviews,
                Focus::Files => Focus::Diff,
                Focus::Diff => Focus::Reviews,
            };
            self.window.request_redraw();
            return;
        }

        let layout = self.layout(config);

        if self.search_focused && self.focus == Focus::Reviews {
            match &event.logical_key {
                Key::Named(NamedKey::Backspace) => {
                    self.search.pop();
                    self.reset_pr_selection();
                }
                Key::Named(NamedKey::Enter) => {
                    self.search_focused = false;
                    self.start_selected();
                }
                Key::Named(NamedKey::ArrowUp) => self.select_relative(-1),
                Key::Named(NamedKey::ArrowDown) => self.select_relative(1),
                Key::Named(NamedKey::PageUp) => {
                    self.select_relative(-(layout.reviews_visible as isize));
                }
                Key::Named(NamedKey::PageDown) => {
                    self.select_relative(layout.reviews_visible as isize);
                }
                Key::Named(NamedKey::Home) => {
                    if self.item_count() > 0 {
                        self.set_selected(0);
                    }
                }
                Key::Named(NamedKey::End) => {
                    let count = self.item_count();
                    if count > 0 {
                        self.set_selected(count - 1);
                    }
                }
                Key::Named(NamedKey::Space) => {
                    self.search.push(' ');
                    self.reset_pr_selection();
                }
                Key::Character(character) if !command && !self.modifiers.alt_key() => {
                    let text = character.as_str();
                    if !text.is_empty() && text.chars().all(|ch| !ch.is_control()) {
                        self.search.push_str(text);
                        self.reset_pr_selection();
                    }
                }
                _ => {}
            }

            self.window.request_redraw();
            return;
        }

        let mut start_detail = false;

        match self.focus {
            Focus::Reviews => match &event.logical_key {
                Key::Named(NamedKey::ArrowUp) => self.select_relative(-1),
                Key::Named(NamedKey::ArrowDown) => self.select_relative(1),
                Key::Named(NamedKey::PageUp) => {
                    self.select_relative(-(layout.reviews_visible as isize));
                }
                Key::Named(NamedKey::PageDown) => {
                    self.select_relative(layout.reviews_visible as isize);
                }
                Key::Named(NamedKey::Home) => {
                    if self.item_count() > 0 {
                        self.set_selected(0);
                    }
                }
                Key::Named(NamedKey::End) => {
                    let count = self.item_count();
                    if count > 0 {
                        self.set_selected(count - 1);
                    }
                }
                Key::Named(NamedKey::Enter) if !self.review.loading => {
                    start_detail = true;
                }
                Key::Character(character) if character.as_str() == "r" && !command => {
                    match self.mode {
                        ListMode::Local => {
                            self.review.refresh();
                            self.reviewed.clear();
                            self.selected_file = 0;
                            self.file_offset = 0;
                        }
                        ListMode::PullRequests => {
                            self.review.prs_loaded = false;
                            self.start_prs_load();
                            self.pr_selected = 0;
                            self.pr_offset = 0;
                        }
                    }
                }
                Key::Character(character) if character.as_str() == "/" && !command => {
                    if self.mode != ListMode::PullRequests {
                        self.set_mode(ListMode::PullRequests);
                    }
                    self.search_focused = true;
                }
                _ => {}
            },
            Focus::Files => {
                let file_count = self
                    .review
                    .detail
                    .as_ref()
                    .map(|detail| detail.files.len())
                    .unwrap_or(0);
                match &event.logical_key {
                    Key::Named(NamedKey::ArrowUp) => {
                        self.selected_file = self.selected_file.saturating_sub(1);
                        self.review.scroll_row = 0;
                        self.review.scroll_col = 0;
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if file_count > 0 {
                            self.selected_file = (self.selected_file + 1).min(file_count - 1);
                            self.review.scroll_row = 0;
                            self.review.scroll_col = 0;
                        }
                    }
                    Key::Named(NamedKey::PageUp) => {
                        self.selected_file =
                            self.selected_file.saturating_sub(layout.files_visible);
                    }
                    Key::Named(NamedKey::PageDown) => {
                        if file_count > 0 {
                            self.selected_file =
                                (self.selected_file + layout.files_visible).min(file_count - 1);
                        }
                    }
                    Key::Named(NamedKey::Home) => self.selected_file = 0,
                    Key::Named(NamedKey::End) => {
                        self.selected_file = file_count.saturating_sub(1);
                    }
                    Key::Named(NamedKey::Space) => {
                        self.toggle_reviewed(self.selected_file);
                    }
                    Key::Character(character) if character.as_str() == "m" => {
                        self.toggle_reviewed(self.selected_file);
                    }
                    Key::Named(NamedKey::Enter) => self.focus = Focus::Diff,
                    _ => {}
                }

                self.review.scroll_row = 0;
                self.review.scroll_col = 0;
            }
            Focus::Diff => {
                let total = self
                    .review
                    .detail
                    .as_ref()
                    .and_then(|detail| detail.files.get(self.selected_file))
                    .map(|file| file.lines.len())
                    .unwrap_or(0);
                let max_row = total.saturating_sub(layout.diff_visible);

                match &event.logical_key {
                    Key::Named(NamedKey::ArrowUp) => {
                        self.review.scroll_row = self.review.scroll_row.saturating_sub(1);
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        self.review.scroll_row = (self.review.scroll_row + 1).min(max_row);
                    }
                    Key::Named(NamedKey::PageUp) => {
                        self.review.scroll_row =
                            self.review.scroll_row.saturating_sub(layout.diff_visible);
                    }
                    Key::Named(NamedKey::PageDown) => {
                        self.review.scroll_row =
                            (self.review.scroll_row + layout.diff_visible).min(max_row);
                    }
                    Key::Named(NamedKey::Home) => self.review.scroll_row = 0,
                    Key::Named(NamedKey::End) => self.review.scroll_row = max_row,
                    Key::Named(NamedKey::ArrowLeft) => {
                        self.review.scroll_col = self.review.scroll_col.saturating_sub(4);
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        self.review.scroll_col = (self.review.scroll_col + 4).min(4000);
                    }
                    _ => {}
                }
            }
        }

        if start_detail {
            self.start_selected();
        }

        self.window.request_redraw();
    }

    fn on_mouse(&mut self, state: ElementState, button: MouseButton, config: &Config) {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return;
        }

        let layout = self.layout(config);
        let x = self.cursor.x as f32;
        let y = self.cursor.y as f32;

        if layout.close.contains(x, y) {
            self.close_requested = true;
            return;
        }

        if layout.tabs[0].contains(x, y) {
            self.set_mode(ListMode::Local);
            return;
        }

        if layout.tabs[1].contains(x, y) {
            self.set_mode(ListMode::PullRequests);
            return;
        }

        if let Some(search) = layout.search
            && search.contains(x, y)
        {
            self.focus = Focus::Reviews;
            self.search_focused = true;
            self.window.request_redraw();
            return;
        }

        if layout.reviews.contains(x, y) {
            self.focus = Focus::Reviews;
            self.search_focused = false;
            if let Some((index, _)) = layout
                .review_rows
                .iter()
                .find(|(_, rect)| rect.contains(x, y))
            {
                self.set_selected(*index);
                self.start_selected();
            }
            self.window.request_redraw();
            return;
        }

        if let Some(files) = layout.files
            && files.contains(x, y)
        {
            self.focus = Focus::Files;
            if let Some((index, rect)) = layout
                .file_rows
                .iter()
                .find(|(_, rect)| rect.contains(x, y))
            {
                if x <= rect.x + 28.0 * self.renderer.scale_factor() as f32 {
                    self.toggle_reviewed(*index);
                } else {
                    self.selected_file = *index;
                    self.review.scroll_row = 0;
                    self.review.scroll_col = 0;
                }
            }
            self.window.request_redraw();
            return;
        }

        if let Some(diff) = layout.diff
            && diff.contains(x, y)
        {
            self.focus = Focus::Diff;
            self.window.request_redraw();
        }
    }

    fn on_wheel(&mut self, delta: MouseScrollDelta, config: &Config) {
        let layout = self.layout(config);
        let x = self.cursor.x as f32;
        let y = self.cursor.y as f32;
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => (y * 3.0).round() as i32,
            MouseScrollDelta::PixelDelta(position) => {
                (position.y as f32 / self.renderer.cell_metrics().height).round() as i32
            }
        };
        if steps == 0 {
            return;
        }

        if layout.list_area.contains(x, y) {
            self.select_relative(-(steps as isize));
        } else if let Some(files) = layout.files
            && files.contains(x, y)
        {
            let count = self
                .review
                .detail
                .as_ref()
                .map(|detail| detail.files.len())
                .unwrap_or(0);
            if count > 0 {
                self.selected_file =
                    (self.selected_file as i32 - steps).clamp(0, count as i32 - 1) as usize;
                self.review.scroll_row = 0;
                self.review.scroll_col = 0;
            }
        } else if let Some(diff) = layout.diff
            && diff.contains(x, y)
        {
            let total = self
                .review
                .detail
                .as_ref()
                .and_then(|detail| detail.files.get(self.selected_file))
                .map(|file| file.lines.len())
                .unwrap_or(0);
            let max_row = total.saturating_sub(layout.diff_visible) as i32;
            self.review.scroll_row =
                (self.review.scroll_row as i32 - steps).clamp(0, max_row) as usize;
        }

        self.window.request_redraw();
    }

    fn layout(&mut self, _config: &Config) -> Layout {
        let size = self.window.inner_size();
        let width = size.width.max(1) as f32;
        let height = size.height.max(1) as f32;
        let scale = self.renderer.scale_factor() as f32;
        let metrics = self.renderer.cell_metrics();
        let padding = (12.0 * scale).round();

        let header = Rect::new(0.0, 0.0, width, (48.0 * scale).round());
        let footer = Rect::new(
            0.0,
            height - (26.0 * scale).round(),
            width,
            (26.0 * scale).round(),
        );
        let body = Rect::new(
            0.0,
            header.bottom(),
            width,
            (footer.y - header.bottom()).max(1.0),
        );
        let close_size = (20.0 * scale).round();
        let close = Rect::new(
            width - padding - close_size,
            (header.height - close_size) / 2.0,
            close_size,
            close_size,
        );

        let reviews_width = (280.0 * scale).min(body.width * 0.32).max(180.0 * scale);
        let reviews = Rect::new(body.x, body.y, reviews_width, body.height);
        let content = Rect::new(
            reviews.right() + 1.0,
            body.y,
            (body.right() - reviews.right() - 1.0).max(1.0),
            body.height,
        );

        let tabs_height = (34.0 * scale).round();
        let half = reviews.width / 2.0;
        let tabs = [
            Rect::new(reviews.x, reviews.y, half, tabs_height),
            Rect::new(
                reviews.x + half,
                reviews.y,
                (reviews.width - half).max(1.0),
                tabs_height,
            ),
        ];
        let search = (self.mode == ListMode::PullRequests).then(|| {
            Rect::new(
                reviews.x + 8.0 * scale,
                reviews.y + tabs_height + 6.0 * scale,
                (reviews.width - 16.0 * scale).max(1.0),
                (28.0 * scale).round(),
            )
        });
        let list_top = reviews.y
            + tabs_height
            + if self.mode == ListMode::PullRequests {
                (6.0 + 28.0 + 6.0) * scale
            } else {
                0.0
            };
        let list_area = Rect::new(
            reviews.x,
            list_top,
            reviews.width,
            (reviews.bottom() - list_top).max(1.0),
        );
        let reviews_visible = (list_area.height / (50.0 * scale).max(1.0))
            .floor()
            .max(1.0) as usize;

        let count = self.item_count();
        let selected = self.selected_index();
        if count > 0 {
            let offset = match self.mode {
                ListMode::Local => &mut self.review.list_offset,
                ListMode::PullRequests => &mut self.pr_offset,
            };
            if selected < *offset {
                *offset = selected;
            }
            if selected >= *offset + reviews_visible {
                *offset = selected + 1 - reviews_visible;
            }
            *offset = (*offset).min(count.saturating_sub(reviews_visible));
        } else {
            match self.mode {
                ListMode::Local => self.review.list_offset = 0,
                ListMode::PullRequests => self.pr_offset = 0,
            }
        }

        let start = match self.mode {
            ListMode::Local => self.review.list_offset,
            ListMode::PullRequests => self.pr_offset,
        };
        let mut review_rows = Vec::new();
        let end = (start + reviews_visible).min(count);
        for position in start..end {
            let y = list_area.y + (position - start) as f32 * (50.0 * scale);
            review_rows.push((
                position,
                Rect::new(
                    reviews.x + 6.0 * scale,
                    y,
                    (reviews.width - 12.0 * scale).max(1.0),
                    (50.0 * scale - 4.0).max(1.0),
                ),
            ));
        }

        let detail = self.review.detail.as_ref();
        let files_width = (280.0 * scale).min(content.width * 0.4).max(160.0 * scale);
        let files = detail.map(|_| Rect::new(content.x, content.y, files_width, content.height));
        let diff = detail.map(|_| {
            Rect::new(
                content.x + files_width + 1.0,
                content.y,
                (content.right() - content.x - files_width - 1.0).max(1.0),
                content.height,
            )
        });
        let (diff_title, diff_text) = diff
            .map(|area| {
                let title = Rect::new(
                    area.x + padding,
                    area.y + padding * 0.5,
                    (area.width - padding * 2.0).max(1.0),
                    (30.0 * scale).round(),
                );
                let text = Rect::new(
                    area.x + padding,
                    title.bottom() + padding * 0.5,
                    (area.width - padding * 2.0).max(1.0),
                    (area.bottom() - title.bottom() - padding * 1.5).max(1.0),
                );
                (Some(title), Some(text))
            })
            .unwrap_or((None, None));

        let pane_title_height = (30.0 * scale).round();
        let file_visible = (((body.height - pane_title_height) / (38.0 * scale).max(1.0))
            .floor()
            .max(1.0)) as usize;
        let diff_visible = diff_text
            .map(|area| (area.height / metrics.height).floor().max(1.0) as usize)
            .unwrap_or(1);

        let file_count = detail.map(|detail| detail.files.len()).unwrap_or(0);
        if file_count > 0 {
            if self.selected_file < self.file_offset {
                self.file_offset = self.selected_file;
            }
            if self.selected_file >= self.file_offset + file_visible {
                self.file_offset = self.selected_file + 1 - file_visible;
            }
            let max_offset = file_count.saturating_sub(file_visible);
            self.file_offset = self.file_offset.min(max_offset);
        }

        let mut file_rows = Vec::new();
        if let Some(files) = files {
            let end = (self.file_offset + file_visible).min(file_count);
            for index in self.file_offset..end {
                let y = files.y
                    + pane_title_height
                    + (index - self.file_offset) as f32 * (38.0 * scale);
                file_rows.push((
                    index,
                    Rect::new(
                        files.x + 6.0 * scale,
                        y,
                        (files.width - 12.0 * scale).max(1.0),
                        (38.0 * scale - 4.0).max(1.0),
                    ),
                ));
            }
        }

        Layout {
            header,
            footer,
            close,
            reviews,
            tabs,
            search,
            list_area,
            files,
            diff,
            diff_title,
            diff_text,
            content,
            review_rows,
            file_rows,
            padding,
            reviews_visible,
            files_visible: file_visible,
            diff_visible,
        }
    }

    fn draw(&mut self, config: &Config) {
        let theme = &config.theme;
        let scale = self.renderer.scale_factor() as f32;
        let metrics = self.renderer.cell_metrics();
        let ui_font = self.renderer.physical(config.ui_font_size);
        let title_font = (ui_font * 1.15).max(14.0);
        let layout = self.layout(config);

        let mut quads = Vec::new();
        let mut labels = Vec::new();

        quads.push(Quad::new(
            0.0,
            0.0,
            self.window.inner_size().width.max(1) as f32,
            self.window.inner_size().height.max(1) as f32,
            to_linear(theme.background, 1.0),
        ));
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
        quads.push(Quad::new(
            layout.footer.x,
            layout.footer.y,
            layout.footer.width,
            1.0,
            to_linear(theme.border, 1.0),
        ));

        labels.push(Label {
            text: "CODE REVIEW".into(),
            x: layout.padding,
            y: layout.header.y + 7.0 * scale,
            width: (layout.close.x - layout.padding * 2.0).max(1.0),
            font_size: title_font,
            color: theme.sidebar_text,
            bold: true,
        });
        let subtitle = if let Some(detail) = &self.review.detail {
            format!(
                "{} · {} files · +{} −{} · {}",
                detail.summary.title,
                detail.files.len(),
                detail.additions,
                detail.deletions,
                self.review.repo.display()
            )
        } else {
            self.review.repo.display().to_string()
        };

        labels.push(Label {
            text: subtitle,
            x: layout.padding,
            y: layout.header.y + 27.0 * scale,
            width: (layout.close.x - layout.padding * 2.0).max(1.0),
            font_size: ui_font * 0.78,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        labels.push(Label {
            text: "×".into(),
            x: layout.close.x + (layout.close.width - ui_font * 0.6) / 2.0,
            y: layout.close.y + (layout.close.height - ui_font * 1.35) / 2.0,
            width: layout.close.width,
            font_size: ui_font * 1.15,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        // Reviews pane: tabs, search and list.
        quads.push(Quad::new(
            layout.reviews.x,
            layout.reviews.y,
            layout.reviews.width,
            layout.reviews.height,
            to_linear(theme.sidebar_background, 1.0),
        ));
        quads.push(Quad::new(
            layout.reviews.right(),
            layout.reviews.y,
            1.0,
            layout.reviews.height,
            to_linear(theme.border, 1.0),
        ));

        let remote_label = self
            .review
            .forge
            .map_or("REMOTE CHANGES", ForgeKind::tab_label);
        let tab_labels = [
            format!("LOCAL {}", self.review.reviews.len()),
            if self.review.prs_error.is_some() {
                format!("{remote_label} !")
            } else {
                format!("{remote_label} {}", self.review.pull_requests.len())
            },
        ];
        for (index, rect) in layout.tabs.iter().enumerate() {
            let active = (index == 0 && self.mode == ListMode::Local)
                || (index == 1 && self.mode == ListMode::PullRequests);
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
            quads.push(Quad::new(
                rect.x,
                rect.bottom() - 1.0,
                rect.width,
                1.0,
                to_linear(theme.border, 1.0),
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
                text: tab_labels[index].clone(),
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

        if let Some(search) = layout.search {
            quads.push(Quad::new(
                search.x,
                search.y,
                search.width,
                search.height,
                to_linear(theme.background, 1.0),
            ));
            push_outline(
                &mut quads,
                search,
                1.0,
                to_linear(
                    if self.search_focused {
                        theme.accent
                    } else {
                        theme.border
                    },
                    1.0,
                ),
            );

            let placeholder = self
                .review
                .forge
                .map_or("Buscar PR/MR…", ForgeKind::search_placeholder);
            let text = match (self.search.is_empty(), self.search_focused) {
                (true, true) => format!("{placeholder}▌"),
                (true, false) => placeholder.to_owned(),
                (false, true) => format!("{}▌", self.search),
                (false, false) => self.search.clone(),
            };
            labels.push(Label {
                text,
                x: search.x + 8.0 * scale,
                y: search.y + 5.0 * scale,
                width: (search.width - 14.0 * scale).max(1.0),
                font_size: ui_font * 0.82,
                color: if self.search.is_empty() {
                    theme.sidebar_text_dim
                } else {
                    theme.sidebar_text
                },
                bold: false,
            });
        }

        let filtered = if self.mode == ListMode::PullRequests {
            self.filtered_prs()
        } else {
            Vec::new()
        };

        for (position, rect) in &layout.review_rows {
            let (title, subtitle) = match self.mode {
                ListMode::Local => {
                    let Some(summary) = self.review.reviews.get(*position) else {
                        continue;
                    };
                    (summary.title.clone(), summary.subtitle.clone())
                }
                ListMode::PullRequests => {
                    let Some(pull) = filtered
                        .get(*position)
                        .and_then(|index| self.review.pull_requests.get(*index))
                    else {
                        continue;
                    };
                    let draft = if pull.draft { " · draft" } else { "" };
                    (
                        pull.list_title(),
                        format!(
                            "@{} · {} → {}{}",
                            pull.author, pull.source_branch, pull.target_branch, draft
                        ),
                    )
                }
            };

            let selected = *position == self.selected_index();
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
                text: title,
                x: rect.x + 8.0 * scale,
                y: rect.y + 5.0 * scale,
                width: (rect.width - 14.0 * scale).max(1.0),
                font_size: ui_font,
                color: if selected {
                    theme.sidebar_text
                } else {
                    theme.sidebar_text_dim
                },
                bold: selected,
            });
            labels.push(Label {
                text: subtitle,
                x: rect.x + 8.0 * scale,
                y: rect.y + 26.0 * scale,
                width: (rect.width - 14.0 * scale).max(1.0),
                font_size: ui_font * 0.74,
                color: theme.sidebar_text_dim,
                bold: false,
            });
        }

        if self.mode == ListMode::PullRequests && layout.review_rows.is_empty() {
            let changes_name = self
                .review
                .forge
                .map_or("remote changes", ForgeKind::changes_name);
            let empty = self
                .review
                .forge
                .map_or("No open remote changes.".to_owned(), |forge| {
                    forge.empty_message().to_owned()
                });
            let (message, color) = if self.review.prs_loading {
                (
                    format!("Loading open {changes_name}…"),
                    theme.sidebar_text_dim,
                )
            } else if let Some(error) = &self.review.prs_error {
                (error.clone(), theme.diff_remove_text)
            } else if self.review.pull_requests.is_empty() {
                (empty, theme.sidebar_text_dim)
            } else {
                (
                    "No matches for this search.".to_owned(),
                    theme.sidebar_text_dim,
                )
            };
            labels.push(Label {
                text: message,
                x: layout.list_area.x + layout.padding,
                y: layout.list_area.y + layout.padding,
                width: (layout.list_area.width - layout.padding * 2.0).max(1.0),
                font_size: ui_font * 0.82,
                color,
                bold: false,
            });
        }

        let detail = self.review.detail.as_ref();

        if let Some(detail) = detail {
            if let Some(files) = layout.files {
                quads.push(Quad::new(
                    files.x,
                    files.y,
                    files.width,
                    files.height,
                    to_linear(theme.background, 1.0),
                ));
                quads.push(Quad::new(
                    files.right(),
                    files.y,
                    1.0,
                    files.height,
                    to_linear(theme.border, 1.0),
                ));

                let reviewed = detail
                    .files
                    .iter()
                    .filter(|file| self.is_reviewed(file))
                    .count();
                labels.push(Label {
                    text: format!("FILES · {reviewed}/{} revisados", detail.files.len()),
                    x: files.x + layout.padding,
                    y: files.y + 8.0 * scale,
                    width: (files.width - layout.padding * 2.0).max(1.0),
                    font_size: ui_font * 0.82,
                    color: theme.sidebar_text_dim,
                    bold: true,
                });

                for (index, rect) in &layout.file_rows {
                    let Some(file) = detail.files.get(*index) else {
                        continue;
                    };
                    let selected = *index == self.selected_file;
                    let reviewed = self.is_reviewed(file);
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

                    let status = match file.status {
                        FileStatus::Modified => "M",
                        FileStatus::Added => "A",
                        FileStatus::Deleted => "D",
                        FileStatus::Renamed => "R",
                        FileStatus::Binary => "B",
                    };
                    let check = if reviewed { "✓" } else { " " };
                    labels.push(Label {
                        text: check.into(),
                        x: rect.x + 6.0 * scale,
                        y: rect.y + 7.0 * scale,
                        width: 18.0 * scale,
                        font_size: ui_font,
                        color: if reviewed {
                            theme.accent
                        } else {
                            theme.sidebar_text_dim
                        },
                        bold: reviewed,
                    });
                    labels.push(Label {
                        text: format!("{status}  {}", file.path),
                        x: rect.x + 28.0 * scale,
                        y: rect.y + 4.0 * scale,
                        width: (rect.width - 34.0 * scale).max(1.0),
                        font_size: ui_font * 0.86,
                        color: if reviewed {
                            theme.sidebar_text_dim
                        } else {
                            theme.sidebar_text
                        },
                        bold: selected,
                    });
                    labels.push(Label {
                        text: format!("+{} −{}", file.additions, file.deletions),
                        x: rect.x + 28.0 * scale,
                        y: rect.y + 20.0 * scale,
                        width: (rect.width - 34.0 * scale).max(1.0),
                        font_size: ui_font * 0.7,
                        color: theme.sidebar_text_dim,
                        bold: false,
                    });
                }
            }

            let selected_file = detail.files.get(self.selected_file);
            let mut diff_frame = None;
            if let (Some(area), Some(title)) = (layout.diff_text, layout.diff_title) {
                quads.push(Quad::new(
                    layout.diff.map(|r| r.x).unwrap_or(area.x),
                    layout.diff.map(|r| r.y).unwrap_or(area.y),
                    layout.diff.map(|r| r.width).unwrap_or(area.width),
                    layout.diff.map(|r| r.height).unwrap_or(area.height),
                    to_linear(theme.background, 1.0),
                ));

                if let Some(file) = selected_file {
                    let stats = format!(
                        "{}  {}   +{} −{}",
                        match file.status {
                            FileStatus::Modified => "M",
                            FileStatus::Added => "A",
                            FileStatus::Deleted => "D",
                            FileStatus::Renamed => "R",
                            FileStatus::Binary => "B",
                        },
                        file.path,
                        file.additions,
                        file.deletions
                    );
                    labels.push(Label {
                        text: stats,
                        x: title.x,
                        y: title.y,
                        width: title.width,
                        font_size: ui_font * 0.9,
                        color: theme.sidebar_text,
                        bold: true,
                    });

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
                        diff_frame = Some((
                            area,
                            build_file_frame(
                                file,
                                area,
                                metrics,
                                theme,
                                self.review.scroll_row,
                                self.review.scroll_col,
                                &mut quads,
                            ),
                        ));
                    }
                } else {
                    labels.push(Label {
                        text: "Select a file".into(),
                        x: area.x,
                        y: area.y,
                        width: area.width,
                        font_size: ui_font,
                        color: theme.sidebar_text_dim,
                        bold: false,
                    });
                }
            }

            let mut scene_panes = Vec::new();
            if let Some((area, frame)) = &diff_frame {
                scene_panes.push(PaneLayer {
                    id: u64::MAX,
                    content: *area,
                    frame,
                    dirty: true,
                });
            }

            let focus = self.focus;
            if let Some(area) = match focus {
                Focus::Reviews => Some(layout.reviews),
                Focus::Files => layout.files,
                Focus::Diff => layout.diff,
            } {
                push_outline(
                    &mut quads,
                    area,
                    (2.0 * scale).max(1.0),
                    to_linear(theme.accent, 0.8),
                );
            }

            let hint = self.hint(focus);
            labels.push(Label {
                text: hint.into(),
                x: layout.footer.x + layout.padding,
                y: layout.footer.y + 5.0 * scale,
                width: (layout.footer.width - layout.padding * 2.0).max(1.0),
                font_size: ui_font * 0.76,
                color: theme.sidebar_text_dim,
                bold: false,
            });

            let scene = Scene {
                panes: scene_panes,
                labels,
                quads,
            };
            if let Err(error) = self.renderer.render(&scene, theme) {
                log::error!("review window render failed: {error:#}");
            }
            return;
        }

        // No detail: loading/error/welcome.
        let (message, is_error) = match self.mode {
            ListMode::PullRequests => {
                let changes_name = self
                    .review
                    .forge
                    .map_or("remote changes", ForgeKind::changes_name);
                if self.review.prs_loading {
                    (format!("Loading open {changes_name}…"), false)
                } else if let Some(error) = &self.review.prs_error {
                    (error.clone(), true)
                } else if self.review.pull_requests.is_empty() {
                    let empty = self.review.forge.map_or(
                        "No open remote changes found in this repository.".to_owned(),
                        |forge| forge.empty_message().to_owned(),
                    );
                    (empty, false)
                } else {
                    (
                        "Selecciona un pull request o merge request de la izquierda y pulsa Enter."
                            .to_owned(),
                        false,
                    )
                }
            }
            ListMode::Local => {
                if self.review.loading {
                    ("Loading review…".to_owned(), false)
                } else if let Some(error) = &self.review.error {
                    (error.clone(), true)
                } else if self.review.reviews.is_empty() {
                    (
                        "No changes or commits found in this repository.".to_owned(),
                        false,
                    )
                } else {
                    (
                        "Selecciona una review de la izquierda y pulsa Enter.".to_owned(),
                        false,
                    )
                }
            }
        };
        labels.push(Label {
            text: message,
            x: layout.content.x + layout.padding,
            y: layout.content.y + 24.0 * scale,
            width: (layout.content.width - layout.padding * 2.0).max(1.0),
            font_size: ui_font,
            color: if is_error {
                theme.diff_remove_text
            } else {
                theme.sidebar_text_dim
            },
            bold: false,
        });

        let hint = self.hint(Focus::Reviews);
        labels.push(Label {
            text: hint.into(),
            x: layout.footer.x + layout.padding,
            y: layout.footer.y + 5.0 * scale,
            width: (layout.footer.width - layout.padding * 2.0).max(1.0),
            font_size: ui_font * 0.76,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        let scene = Scene {
            panes: Vec::new(),
            labels,
            quads,
        };
        if let Err(error) = self.renderer.render(&scene, theme) {
            log::error!("review window render failed: {error:#}");
        }
    }

    fn hint(&self, focus: Focus) -> &'static str {
        if self.search_focused && self.mode == ListMode::PullRequests {
            return "Escribe para filtrar   Enter abrir   ↑↓ mover   Esc salir de búsqueda";
        }

        match focus {
            Focus::Reviews if self.mode == ListMode::PullRequests => {
                "↑↓ PR/MR   Enter revisar   / o Ctrl+F buscar   R refrescar   Tab panel   Esc cerrar"
            }
            Focus::Reviews => {
                "↑↓ revisar   Enter abrir   / buscar PRs   R refrescar   Tab panel   Esc cerrar"
            }
            Focus::Files => "↑↓ archivo   Espacio marcar   Enter diff   Tab panel   Esc cerrar",
            Focus::Diff => "↑↓ scroll   ←→ horizontal   PgUp/PgDn   Tab panel   Esc cerrar",
        }
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
