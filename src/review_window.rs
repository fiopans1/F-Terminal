use std::collections::HashSet;
use std::sync::Arc;

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::config::Config;
use crate::event::UserEvent;
use crate::layout::Rect;
use crate::render::quads::Quad;
use crate::render::scene::{Label, PaneLayer, Scene};
use crate::render::{CellMetrics, Renderer};
use crate::review::{self, CodeReview, DiffLineKind, FileDiff, FileStatus, ReviewDetail};
use crate::term::color::{Theme, to_linear};
use crate::term::frame::{Frame, Row, Run};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Reviews,
    Files,
    Diff,
}

struct Layout {
    header: Rect,
    footer: Rect,
    close: Rect,
    reviews: Rect,
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
    selected_file: usize,
    file_offset: usize,
    reviewed: HashSet<String>,
    request_id: u64,
    close_requested: bool,
}

impl ReviewWindow {
    pub fn new(
        window: Arc<Window>,
        renderer: Renderer,
        directory: std::path::PathBuf,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        Self {
            window,
            renderer,
            review: CodeReview::open(directory),
            proxy,
            modifiers: ModifiersState::empty(),
            cursor: PhysicalPosition::new(0.0, 0.0),
            focus: Focus::Reviews,
            selected_file: 0,
            file_offset: 0,
            reviewed: HashSet::new(),
            request_id: 0,
            close_requested: false,
        }
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

    fn start_detail(&mut self) {
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;

        let Some((repo, summary)) = self.review.start_loading(request_id) else {
            return;
        };

        let proxy = self.proxy.clone();
        let window_id = self.window.id();
        let spawned = std::thread::Builder::new()
            .name("code-review".into())
            .spawn(move || {
                let result =
                    review::load_detail(&repo, &summary).map_err(|error| format!("{error:#}"));
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

    fn on_key(&mut self, event: KeyEvent, config: &Config) {
        if event.state != ElementState::Pressed {
            return;
        }

        let focusable_files = self
            .review
            .detail
            .as_ref()
            .is_some_and(|detail| !detail.files.is_empty());

        if matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyW))
            && (self.modifiers.super_key() || self.modifiers.control_key())
        {
            self.close_requested = true;
            return;
        }

        if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
            self.close_requested = true;
            return;
        }

        if matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
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
        let mut start_detail = false;

        match self.focus {
            Focus::Reviews => match &event.logical_key {
                Key::Named(NamedKey::ArrowUp) => {
                    self.review.selected = self.review.selected.saturating_sub(1);
                    self.review.back_to_list();
                    self.reviewed.clear();
                }
                Key::Named(NamedKey::ArrowDown) => {
                    if !self.review.reviews.is_empty() {
                        self.review.selected =
                            (self.review.selected + 1).min(self.review.reviews.len() - 1);
                        self.review.back_to_list();
                        self.reviewed.clear();
                    }
                }
                Key::Named(NamedKey::PageUp) => {
                    self.review.selected =
                        self.review.selected.saturating_sub(layout.reviews_visible);
                    self.review.back_to_list();
                    self.reviewed.clear();
                }
                Key::Named(NamedKey::PageDown) => {
                    if !self.review.reviews.is_empty() {
                        self.review.selected = (self.review.selected + layout.reviews_visible)
                            .min(self.review.reviews.len() - 1);
                        self.review.back_to_list();
                        self.reviewed.clear();
                    }
                }
                Key::Named(NamedKey::Home) => {
                    self.review.selected = 0;
                    self.review.back_to_list();
                    self.reviewed.clear();
                }
                Key::Named(NamedKey::End) => {
                    self.review.selected = self.review.reviews.len().saturating_sub(1);
                    self.review.back_to_list();
                    self.reviewed.clear();
                }
                Key::Named(NamedKey::Enter) if !self.review.loading => {
                    start_detail = true;
                }
                Key::Character(character) if character.as_str() == "r" => {
                    self.review.refresh();
                    self.reviewed.clear();
                    self.selected_file = 0;
                    self.file_offset = 0;
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
            self.start_detail();
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

        if layout.reviews.contains(x, y) {
            self.focus = Focus::Reviews;
            if let Some((index, _)) = layout
                .review_rows
                .iter()
                .find(|(_, rect)| rect.contains(x, y))
            {
                self.review.selected = *index;
                self.review.back_to_list();
                self.reviewed.clear();
                self.selected_file = 0;
                self.file_offset = 0;
                self.start_detail();
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

        if layout.reviews.contains(x, y) {
            if !self.review.reviews.is_empty() {
                self.review.selected = (self.review.selected as i32 - steps)
                    .clamp(0, self.review.reviews.len() as i32 - 1)
                    as usize;
                self.review.back_to_list();
                self.reviewed.clear();
            }
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
        let reviews_visible = (((body.height - pane_title_height) / (50.0 * scale).max(1.0))
            .floor()
            .max(1.0)) as usize;
        let file_visible = (((body.height - pane_title_height) / (38.0 * scale).max(1.0))
            .floor()
            .max(1.0)) as usize;
        let diff_visible = diff_text
            .map(|area| (area.height / metrics.height).floor().max(1.0) as usize)
            .unwrap_or(1);

        let count = self.review.reviews.len();
        if count > 0 {
            if self.review.selected < self.review.list_offset {
                self.review.list_offset = self.review.selected;
            }
            if self.review.selected >= self.review.list_offset + reviews_visible {
                self.review.list_offset = self.review.selected + 1 - reviews_visible;
            }
            let max_offset = count.saturating_sub(reviews_visible);
            self.review.list_offset = self.review.list_offset.min(max_offset);
        }

        let mut review_rows = Vec::new();
        let start = self.review.list_offset;
        let end = (start + reviews_visible).min(count);
        for index in start..end {
            let y = reviews.y + pane_title_height + (index - start) as f32 * (50.0 * scale);
            review_rows.push((
                index,
                Rect::new(
                    reviews.x + 6.0 * scale,
                    y,
                    (reviews.width - 12.0 * scale).max(1.0),
                    (50.0 * scale - 4.0).max(1.0),
                ),
            ));
        }

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

        // Panes.
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

        let review_title = "REVIEWS";
        labels.push(Label {
            text: review_title.into(),
            x: layout.reviews.x + layout.padding,
            y: layout.reviews.y + 8.0 * scale,
            width: (layout.reviews.width - layout.padding * 2.0).max(1.0),
            font_size: ui_font * 0.82,
            color: theme.sidebar_text_dim,
            bold: true,
        });

        for (index, rect) in &layout.review_rows {
            let Some(summary) = self.review.reviews.get(*index) else {
                continue;
            };
            let selected = *index == self.review.selected;
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
                text: summary.title.clone(),
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
                text: summary.subtitle.clone(),
                x: rect.x + 8.0 * scale,
                y: rect.y + 26.0 * scale,
                width: (rect.width - 14.0 * scale).max(1.0),
                font_size: ui_font * 0.74,
                color: theme.sidebar_text_dim,
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

            let hint = match focus {
                Focus::Reviews => "↑↓ revisar   Enter abrir   Tab panel   R refrescar   Esc cerrar",
                Focus::Files => "↑↓ archivo   Espacio marcar   Enter diff   Tab panel   Esc cerrar",
                Focus::Diff => "↑↓ scroll   ←→ horizontal   PgUp/PgDn   Tab panel   Esc cerrar",
            };
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
        let message = if self.review.loading {
            "Loading review…".to_owned()
        } else if let Some(error) = &self.review.error {
            error.clone()
        } else if self.review.reviews.is_empty() {
            "No changes or commits found in this repository.".to_owned()
        } else {
            "Selecciona una review de la izquierda y pulsa Enter.".to_owned()
        };
        labels.push(Label {
            text: message,
            x: layout.content.x + layout.padding,
            y: layout.content.y + 24.0 * scale,
            width: (layout.content.width - layout.padding * 2.0).max(1.0),
            font_size: ui_font,
            color: if self.review.error.is_some() {
                theme.diff_remove_text
            } else {
                theme.sidebar_text_dim
            },
            bold: false,
        });

        labels.push(Label {
            text: "↑↓ revisar   Enter abrir   R refrescar   Esc cerrar".into(),
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
}

fn build_file_frame(
    file: &FileDiff,
    area: Rect,
    metrics: CellMetrics,
    theme: &Theme,
    scroll_row: usize,
    scroll_col: usize,
    quads: &mut Vec<Quad>,
) -> Frame {
    let cols = (area.width / metrics.width).floor().max(1.0) as usize;
    let rows = (area.height / metrics.height).floor().max(1.0) as usize;
    let start = scroll_row.min(file.lines.len().saturating_sub(rows));

    let mut frame_rows = Vec::with_capacity(rows);
    for index in 0..rows {
        let mut row = Row::default();
        let line_index = start + index;
        if let Some(line) = file.lines.get(line_index) {
            let prefix = match line.kind {
                DiffLineKind::Add => {
                    format!("{:>5} {:>5} + ", "", line.new.unwrap_or_default())
                }
                DiffLineKind::Remove => {
                    format!("{:>5} {:>5} - ", line.old.unwrap_or_default(), "")
                }
                DiffLineKind::Context => format!(
                    "{:>5} {:>5}   ",
                    line.old.unwrap_or_default(),
                    line.new.unwrap_or_default()
                ),
                _ => String::new(),
            };

            let text: String = format!("{prefix}{}", line.text)
                .chars()
                .skip(scroll_col)
                .take(cols)
                .collect();

            if matches!(line.kind, DiffLineKind::Add | DiffLineKind::Remove) {
                let color = if line.kind == DiffLineKind::Add {
                    theme.diff_add_background
                } else {
                    theme.diff_remove_background
                };
                quads.push(Quad::new(
                    area.x,
                    area.y + index as f32 * metrics.height,
                    area.width,
                    metrics.height,
                    to_linear(color, 1.0),
                ));
            }

            if !text.is_empty() {
                let color = match line.kind {
                    DiffLineKind::Add => theme.diff_add_text,
                    DiffLineKind::Remove => theme.diff_remove_text,
                    DiffLineKind::Hunk => theme.diff_hunk_text,
                    DiffLineKind::Meta => theme.diff_meta_text,
                    DiffLineKind::Context => theme.foreground,
                };
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
