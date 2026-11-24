use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::agents::{AgentId, AgentRow, AgentSnapshot, AgentState};
use crate::config::Config;
use crate::event::UserEvent;
use crate::layout::Rect;
use crate::render::Renderer;
use crate::render::quads::Quad;
use crate::render::scene::{Label, Scene};
use crate::term::color::{Rgb, Theme, to_linear};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filter {
    All,
    Attention,
    Running,
    Finished,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Attention => "ATTENTION",
            Self::Running => "RUNNING",
            Self::Finished => "FINISHED",
        }
    }

    fn matches(self, row: &AgentRow) -> bool {
        match self {
            Self::All => true,
            Self::Attention => row.state.needs_attention() && (!row.seen || row.is_blocked()),
            Self::Running => !row.state.is_finished(),
            Self::Finished => row.state.is_finished(),
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Attention,
            Self::Attention => Self::Running,
            Self::Running => Self::Finished,
            Self::Finished => Self::All,
        }
    }
}

trait AgentRowExt {
    fn is_blocked(&self) -> bool;
}

impl AgentRowExt for AgentRow {
    fn is_blocked(&self) -> bool {
        matches!(self.state, AgentState::Blocked(_) | AgentState::Attention)
    }
}

struct Layout {
    header: Rect,
    footer: Rect,
    list: Rect,
    detail: Rect,
    close: Rect,
    refresh: Rect,
    kill: Rect,
    tabs: [Rect; 4],
    list_rows: Rect,
    padding: f32,
    row_height: f32,
}

struct Notice {
    text: String,
    at: Instant,
    error: bool,
}

pub struct AgentWindow {
    window: Arc<Window>,
    renderer: Renderer,
    proxy: EventLoopProxy<UserEvent>,
    modifiers: ModifiersState,
    cursor: PhysicalPosition<f64>,
    snapshot: AgentSnapshot,
    filter: Filter,
    selected: Option<AgentId>,
    hovered: Option<AgentId>,
    scroll: usize,
    last_click: Option<(Instant, AgentId)>,
    close_requested: bool,
    last_tick: Instant,
    refreshing_until: Option<Instant>,
    kill_armed: Option<(AgentId, Instant)>,
    notice: Option<Notice>,
}

impl AgentWindow {
    pub fn new(window: Arc<Window>, renderer: Renderer, proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            window,
            renderer,
            proxy,
            modifiers: ModifiersState::empty(),
            cursor: PhysicalPosition::new(0.0, 0.0),
            snapshot: AgentSnapshot::default(),
            filter: Filter::All,
            selected: None,
            hovered: None,
            scroll: 0,
            last_click: None,
            close_requested: false,
            last_tick: Instant::now(),
            refreshing_until: None,
            kill_armed: None,
            notice: None,
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

    pub fn apply_snapshot(&mut self, snapshot: AgentSnapshot) {
        let selected = self.selected.clone();
        let changed = self.snapshot != snapshot;
        let generated_changed = self.snapshot.generated != snapshot.generated;
        self.snapshot = snapshot;
        self.window.set_title(&format!(
            "Agent Mode — {} running · {} need attention",
            self.snapshot.active_count(),
            self.snapshot.attention_count()
        ));
        if selected
            .as_ref()
            .is_some_and(|id| self.snapshot.rows.iter().any(|row| &row.id == id))
        {
            self.selected = selected;
        } else {
            self.selected = self.filtered().first().map(|row| row.id.clone());
        }
        if self
            .kill_armed
            .as_ref()
            .is_some_and(|(id, _)| !self.snapshot.rows.iter().any(|row| &row.id == id))
        {
            self.kill_armed = None;
        }
        self.clamp_selection();
        if changed || generated_changed {
            self.window.request_redraw();
        }
    }

    pub fn tick(&mut self) {
        if self.last_tick.elapsed() >= Duration::from_secs(1) {
            self.last_tick = Instant::now();
            if self
                .refreshing_until
                .is_some_and(|until| Instant::now() >= until)
            {
                self.refreshing_until = None;
            }
            if self
                .kill_armed
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(4))
            {
                self.kill_armed = None;
            }
            if self
                .notice
                .as_ref()
                .is_some_and(|notice| notice.at.elapsed() > Duration::from_secs(6))
            {
                self.notice = None;
            }
            self.window.request_redraw();
        }
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
                let hovered = self.row_at(position, config);
                if hovered != self.hovered {
                    self.hovered = hovered;
                    self.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse(state, button, config);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32 / 40.0,
                };
                let max = self.max_scroll(config);
                if lines > 0.0 {
                    self.scroll = self.scroll.saturating_sub(lines.abs() as usize);
                } else {
                    self.scroll = (self.scroll + lines.abs() as usize).min(max);
                }
                self.window.request_redraw();
            }
            _ => {}
        }
    }

    fn on_key(&mut self, event: KeyEvent, config: &Config) {
        if event.state != ElementState::Pressed {
            return;
        }
        let ctrl = self.modifiers.control_key();
        let alt = self.modifiers.alt_key();
        let shift = self.modifiers.shift_key();
        let super_key = self.modifiers.super_key();
        let app_mod = if cfg!(target_os = "macos") {
            super_key && !ctrl
        } else {
            ctrl && shift
        };
        if app_mod
            && matches!(&event.logical_key, Key::Character(character) if character.eq_ignore_ascii_case("w"))
        {
            self.close_requested = true;
            return;
        }
        match event.logical_key {
            Key::Named(NamedKey::Escape) => self.close_requested = true,
            Key::Named(NamedKey::ArrowUp) => self.move_selection(-1),
            Key::Named(NamedKey::ArrowDown) => self.move_selection(1),
            Key::Named(NamedKey::PageUp) => self.move_selection(-6),
            Key::Named(NamedKey::PageDown) => self.move_selection(6),
            Key::Named(NamedKey::Home) => self.select_first(),
            Key::Named(NamedKey::End) => self.select_last(),
            Key::Named(NamedKey::Enter) => self.focus_selected(),
            Key::Named(NamedKey::F5) => self.refresh(),
            Key::Named(NamedKey::Delete | NamedKey::Backspace) => self.kill_selected(),
            Key::Named(NamedKey::Tab) => {
                self.filter = if shift {
                    match self.filter {
                        Filter::All => Filter::Finished,
                        Filter::Attention => Filter::All,
                        Filter::Running => Filter::Attention,
                        Filter::Finished => Filter::Running,
                    }
                } else {
                    self.filter.next()
                };
                self.after_filter_change();
            }
            Key::Character(character) if !ctrl && !alt => match character.as_str() {
                "f" | "F" => {
                    self.filter = self.filter.next();
                    self.after_filter_change();
                }
                "j" | "J" => self.move_selection(1),
                "k" | "K" => self.move_selection(-1),
                "r" | "R" => self.refresh(),
                "x" | "X" => self.kill_selected(),
                "1" => self.set_filter(Filter::All),
                "2" => self.set_filter(Filter::Attention),
                "3" => self.set_filter(Filter::Running),
                "4" => self.set_filter(Filter::Finished),
                _ => {}
            },
            _ => {}
        }
        let _ = config;
        self.window.request_redraw();
    }

    fn on_mouse(&mut self, state: ElementState, button: MouseButton, config: &Config) {
        if state != ElementState::Pressed {
            return;
        }
        if button != MouseButton::Left {
            return;
        }

        let layout = self.layout(config);
        if layout
            .close
            .contains(self.cursor.x as f32, self.cursor.y as f32)
        {
            self.close_requested = true;
            return;
        }

        if layout
            .refresh
            .contains(self.cursor.x as f32, self.cursor.y as f32)
        {
            self.refresh();
            return;
        }

        if self.selected_row().is_some_and(|row| row.pid.is_some())
            && layout
                .kill
                .contains(self.cursor.x as f32, self.cursor.y as f32)
        {
            self.kill_selected();
            return;
        }

        for (filter, rect) in self.filters().iter().zip(layout.tabs.iter()) {
            if rect.contains(self.cursor.x as f32, self.cursor.y as f32) {
                self.set_filter(*filter);
                return;
            }
        }

        let Some(id) = self.row_at(self.cursor, config) else {
            return;
        };
        let now = Instant::now();
        let double_click = self.last_click.as_ref().is_some_and(|(at, last)| {
            *last == id && now.duration_since(*at) < Duration::from_millis(400)
        });
        self.selected = Some(id.clone());
        self.last_click = Some((now, id.clone()));
        if double_click {
            self.focus_selected();
        }
        self.window.request_redraw();
    }

    fn move_selection(&mut self, delta: isize) {
        let rows = self.filtered();
        if rows.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id))
            .unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, rows.len() as isize - 1) as usize;
        self.selected = Some(rows[next].id.clone());
        self.clamp_selection();
    }

    fn select_first(&mut self) {
        self.selected = self.filtered().first().map(|row| row.id.clone());
        self.scroll = 0;
    }

    fn select_last(&mut self) {
        self.selected = self.filtered().last().map(|row| row.id.clone());
        self.clamp_selection();
    }

    fn set_filter(&mut self, filter: Filter) {
        if self.filter == filter {
            return;
        }
        self.filter = filter;
        self.after_filter_change();
    }

    fn after_filter_change(&mut self) {
        if self
            .selected
            .as_ref()
            .is_some_and(|id| self.filtered().iter().any(|row| &row.id == id))
        {
            self.clamp_selection();
        } else {
            self.select_first();
        }
        self.window.request_redraw();
    }

    fn filtered(&self) -> Vec<&AgentRow> {
        self.snapshot
            .rows
            .iter()
            .filter(|row| self.filter.matches(row))
            .collect()
    }

    fn filters(&self) -> [Filter; 4] {
        [
            Filter::All,
            Filter::Attention,
            Filter::Running,
            Filter::Finished,
        ]
    }

    fn selected_row(&self) -> Option<&AgentRow> {
        let id = self.selected.as_ref()?;
        self.snapshot.rows.iter().find(|row| &row.id == id)
    }

    fn focus_selected(&self) {
        if let Some(pane) = self.selected_row().and_then(|row| row.pane) {
            let _ = self.proxy.send_event(UserEvent::FocusAgentPane { pane });
        }
    }

    fn refresh(&mut self) {
        let _ = self.proxy.send_event(UserEvent::RefreshAgents);
        self.refreshing_until = Some(Instant::now() + Duration::from_secs(2));
        self.window.request_redraw();
    }

    /// First activation arms the action, the second one within four seconds
    /// actually terminates the process.
    fn kill_selected(&mut self) {
        let Some((id, pid)) = self
            .selected_row()
            .and_then(|row| Some((row.id.clone(), row.pid?)))
        else {
            self.notice = Some(Notice {
                text: "This agent has no process to kill".into(),
                at: Instant::now(),
                error: true,
            });
            self.kill_armed = None;
            self.window.request_redraw();
            return;
        };

        let now = Instant::now();
        let armed = self.kill_armed.as_ref().is_some_and(|(armed, at)| {
            *armed == id && now.duration_since(*at) <= Duration::from_secs(4)
        });
        if armed {
            self.kill_armed = None;
            let _ = self.proxy.send_event(UserEvent::KillAgent { pid });
        } else {
            self.kill_armed = Some((id, now));
        }
        self.window.request_redraw();
    }

    pub fn on_kill_requested(&mut self, pid: u32) {
        self.notice = Some(Notice {
            text: format!("Stopping PID {pid}…"),
            at: Instant::now(),
            error: false,
        });
        self.window.request_redraw();
    }

    pub fn on_agent_killed(&mut self, pid: u32, result: &Result<String, String>) {
        let (text, error) = match result {
            Ok(message) => (format!("PID {pid}: {message}"), false),
            Err(message) => (format!("PID {pid}: {message}"), true),
        };
        self.notice = Some(Notice {
            text,
            at: Instant::now(),
            error,
        });
        self.window.request_redraw();
    }

    fn max_scroll(&self, config: &Config) -> usize {
        let layout = self.layout(config);
        let rows = self.filtered().len();
        let visible = (layout.list_rows.height / layout.row_height)
            .floor()
            .max(1.0) as usize;
        rows.saturating_sub(visible)
    }

    fn clamp_selection(&mut self) {
        let rows = self.filtered();
        if rows.is_empty() {
            self.selected = None;
            self.scroll = 0;
            return;
        }
        let index = self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id))
            .unwrap_or(0);
        self.selected = Some(rows[index].id.clone());

        // The scroll limit depends on the window size only, so a cheap
        // conservative estimate is fine here. Drawing performs the final clamp.
        let page = 10;
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll + page {
            self.scroll = index + 1 - page;
        }
    }

    fn row_at(&self, position: PhysicalPosition<f64>, config: &Config) -> Option<AgentId> {
        let layout = self.layout(config);
        if !layout
            .list_rows
            .contains(position.x as f32, position.y as f32)
        {
            return None;
        }
        let offset = (position.y as f32 - layout.list_rows.y) / layout.row_height;
        let index = self.scroll + offset.floor().max(0.0) as usize;
        self.filtered().get(index).map(|row| row.id.clone())
    }

    fn layout(&self, _config: &Config) -> Layout {
        let size = self.window.inner_size();
        let width = size.width.max(1) as f32;
        let height = size.height.max(1) as f32;
        let scale = self.renderer.scale_factor() as f32;
        let padding = (18.0 * scale).round();
        let header_height = (64.0 * scale).round();
        let footer_height = (32.0 * scale).round();
        let close_size = (26.0 * scale).round();
        let body_top = header_height;
        let body_bottom = (height - footer_height).max(body_top + 1.0);
        let list_width = (width * 0.54).clamp((320.0 * scale).min(width), width);
        let list = Rect::new(0.0, body_top, list_width, body_bottom - body_top);
        let detail = Rect::new(
            list_width,
            body_top,
            (width - list_width).max(1.0),
            body_bottom - body_top,
        );

        let tab_height = (34.0 * scale).round();
        let tab_width = ((list_width - padding * 2.0) / 4.0).max(1.0);
        let tabs = std::array::from_fn(|index| {
            Rect::new(
                padding + index as f32 * tab_width,
                body_top + padding * 0.5,
                tab_width,
                tab_height,
            )
        });

        let list_rows = Rect::new(
            list.x,
            tabs[0].bottom() + 4.0 * scale,
            list.width,
            (body_bottom - tabs[0].bottom() - 4.0 * scale).max(1.0),
        );

        let close = Rect::new(
            (width - padding - close_size).max(0.0),
            ((header_height - close_size) / 2.0).max(0.0),
            close_size,
            close_size,
        );
        let refresh_width = (96.0 * scale).max(76.0);
        let refresh = Rect::new(
            (close.x - refresh_width - 8.0 * scale).max(padding),
            close.y,
            refresh_width,
            close_size,
        );
        let kill_width = (172.0 * scale).max(132.0);
        let kill_height = (26.0 * scale).max(22.0);

        Layout {
            header: Rect::new(0.0, 0.0, width, header_height),
            footer: Rect::new(0.0, body_bottom, width, footer_height),
            list,
            detail,
            close,
            refresh,
            kill: Rect::new(
                (detail.right() - padding - kill_width).max(detail.x + padding),
                (body_bottom - 44.0 * scale).max(detail.y + padding),
                kill_width,
                kill_height,
            ),
            tabs,
            list_rows,
            padding,
            row_height: (58.0 * scale).round(),
        }
    }

    fn draw(&mut self, config: &Config) {
        let theme = &config.theme;
        let scale = self.renderer.scale_factor() as f32;
        let ui_font = self.renderer.physical(config.ui_font_size);
        let title_font = (ui_font * 1.35).max(15.0);
        let layout = self.layout(config);
        let width = self.window.inner_size().width.max(1) as f32;
        let height = self.window.inner_size().height.max(1) as f32;

        let mut quads = Vec::new();
        let mut labels = Vec::new();

        quads.push(Quad::new(
            0.0,
            0.0,
            width,
            height,
            to_linear(theme.background, 1.0),
        ));
        quads.push(Quad::new(
            0.0,
            0.0,
            width,
            layout.header.height,
            to_linear(theme.sidebar_background, 1.0),
        ));
        quads.push(Quad::new(
            0.0,
            layout.header.bottom() - 1.0,
            width,
            1.0,
            to_linear(theme.border, 1.0),
        ));
        quads.push(Quad::new(
            0.0,
            layout.footer.y,
            width,
            1.0,
            to_linear(theme.border, 1.0),
        ));

        labels.push(Label {
            text: "AGENT MODE".into(),
            x: layout.padding,
            y: 8.0 * scale,
            width: (layout.refresh.x - layout.padding * 2.0).max(1.0),
            font_size: title_font,
            color: theme.sidebar_text,
            bold: true,
        });
        let attention = self.snapshot.attention_count();
        let running = self.snapshot.active_count();
        let subtitle = if self.snapshot.rows.is_empty() {
            "Watching for agents on this machine…".to_owned()
        } else {
            format!(
                "{} running · {} need attention · {} detected",
                running,
                attention,
                self.snapshot.rows.len()
            )
        };
        labels.push(Label {
            text: subtitle,
            x: layout.padding,
            y: 32.0 * scale,
            width: (layout.refresh.x - layout.padding * 2.0).max(1.0),
            font_size: ui_font * 0.82,
            color: if attention > 0 {
                status_color(theme, &AgentState::Attention)
            } else {
                theme.sidebar_text_dim
            },
            bold: false,
        });

        labels.push(Label {
            text: "×".into(),
            x: layout.close.x + (layout.close.width - ui_font * 0.6) / 2.0,
            y: layout.close.y + (layout.close.height - ui_font * 1.35) / 2.0,
            width: layout.close.width,
            font_size: ui_font * 1.2,
            color: theme.sidebar_text_dim,
            bold: false,
        });

        let refreshing = self
            .refreshing_until
            .is_some_and(|until| Instant::now() < until);
        quads.push(Quad::rounded(
            layout.refresh.x,
            layout.refresh.y,
            layout.refresh.width,
            layout.refresh.height,
            4.0 * scale,
            to_linear(
                if refreshing {
                    theme.sidebar_active
                } else {
                    theme.background
                },
                1.0,
            ),
        ));
        quads.push(Quad::new(
            layout.refresh.x,
            layout.refresh.bottom() - 1.0 * scale,
            layout.refresh.width,
            1.0 * scale,
            to_linear(theme.border, 1.0),
        ));
        labels.push(Label {
            text: if refreshing {
                "REFRESHING".into()
            } else {
                "REFRESH".into()
            },
            x: layout.refresh.x + 10.0 * scale,
            y: layout.refresh.y + (layout.refresh.height - ui_font * 1.35) / 2.0,
            width: (layout.refresh.width - 16.0 * scale).max(1.0),
            font_size: ui_font * 0.72,
            color: if refreshing {
                theme.accent
            } else {
                theme.sidebar_text
            },
            bold: true,
        });

        // Body backgrounds.
        quads.push(Quad::new(
            layout.list.x,
            layout.list.y,
            layout.list.width,
            layout.list.height,
            to_linear(theme.sidebar_background, 0.55),
        ));
        quads.push(Quad::new(
            layout.detail.x,
            layout.detail.y,
            1.0,
            layout.detail.height,
            to_linear(theme.border, 1.0),
        ));

        self.draw_filters(&layout, theme, ui_font, scale, &mut quads, &mut labels);
        self.draw_list(&layout, theme, ui_font, scale, &mut quads, &mut labels);
        self.draw_detail(&layout, theme, ui_font, scale, &mut quads, &mut labels);

        let (hint, hint_color) = match &self.notice {
            Some(notice) => (
                notice.text.clone(),
                if notice.error {
                    theme.ansi[1]
                } else {
                    theme.accent
                },
            ),
            None => {
                let updated = self
                    .snapshot
                    .generated
                    .map(|generated| elapsed_label(generated.elapsed()))
                    .unwrap_or_else(|| "never".into());
                (
                    format!(
                        "↑↓ select · Enter focus · R refresh · X kill · Tab filter · Esc close · updated {updated}"
                    ),
                    theme.sidebar_text_dim,
                )
            }
        };
        labels.push(Label {
            text: hint,
            x: layout.padding,
            y: layout.footer.y + 7.0 * scale,
            width: (width - layout.padding * 2.0).max(1.0),
            font_size: ui_font * 0.76,
            color: hint_color,
            bold: false,
        });

        let scene = Scene {
            panes: Vec::new(),
            labels,
            quads,
        };
        if let Err(error) = self.renderer.render(&scene, theme) {
            log::error!("agent window render failed: {error:#}");
        }
    }

    fn draw_filters(
        &self,
        layout: &Layout,
        theme: &Theme,
        font: f32,
        scale: f32,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let counts = [
            self.snapshot.rows.len(),
            self.snapshot
                .rows
                .iter()
                .filter(|row| row.state.needs_attention() && (!row.seen || row.is_blocked()))
                .count(),
            self.snapshot
                .rows
                .iter()
                .filter(|row| !row.state.is_finished())
                .count(),
            self.snapshot
                .rows
                .iter()
                .filter(|row| row.state.is_finished())
                .count(),
        ];

        for (index, (filter, rect)) in self.filters().iter().zip(layout.tabs.iter()).enumerate() {
            let active = *filter == self.filter;
            quads.push(Quad::rounded(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                4.0 * scale,
                to_linear(
                    if active {
                        theme.sidebar_active
                    } else {
                        theme.background
                    },
                    1.0,
                ),
            ));
            if active {
                quads.push(Quad::new(
                    rect.x,
                    rect.bottom() - 2.0 * scale,
                    rect.width,
                    2.0 * scale,
                    to_linear(theme.accent, 1.0),
                ));
            }
            labels.push(Label {
                text: format!("{} {}", filter.label(), counts[index]),
                x: rect.x + 7.0 * scale,
                y: rect.y + (rect.height - font * 1.35) / 2.0,
                width: (rect.width - 12.0 * scale).max(1.0),
                font_size: font * 0.72,
                color: if active {
                    theme.sidebar_text
                } else {
                    theme.sidebar_text_dim
                },
                bold: active,
            });
        }
    }

    fn draw_list(
        &mut self,
        layout: &Layout,
        theme: &Theme,
        font: f32,
        scale: f32,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let rows: Vec<AgentRow> = self.filtered().into_iter().cloned().collect();
        if rows.is_empty() {
            labels.push(Label {
                text: if self.snapshot.rows.is_empty() {
                    "No agents detected yet"
                } else {
                    "No agents match this filter"
                }
                .into(),
                x: layout.list.x + layout.padding,
                y: layout.list.y + layout.padding * 2.0,
                width: (layout.list.width - layout.padding * 2.0).max(1.0),
                font_size: font,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            return;
        }

        let visible = (layout.list_rows.height / layout.row_height)
            .floor()
            .max(1.0) as usize;
        self.scroll = self.scroll.min(rows.len().saturating_sub(visible));
        let selected_index = self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id));
        if let Some(index) = selected_index {
            if index < self.scroll {
                self.scroll = index;
            } else if index >= self.scroll + visible {
                self.scroll = index + 1 - visible;
            }
        }

        for (offset, row) in rows.iter().enumerate().skip(self.scroll).take(visible) {
            let y = layout.list_rows.y + (offset - self.scroll) as f32 * layout.row_height;
            let rect = Rect::new(
                layout.list.x + 6.0 * scale,
                y,
                (layout.list.width - 12.0 * scale).max(1.0),
                layout.row_height - 4.0 * scale,
            );
            let selected = self.selected.as_ref() == Some(&row.id);
            let hovered = self.hovered.as_ref() == Some(&row.id);
            if selected || hovered {
                quads.push(Quad::rounded(
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    5.0 * scale,
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
                quads.push(Quad::rounded(
                    rect.x,
                    rect.y,
                    3.0 * scale,
                    rect.height,
                    1.5 * scale,
                    to_linear(theme.accent, 1.0),
                ));
            }

            let color = status_color(theme, &row.state);
            let dot = (8.0 * scale).max(6.0);
            quads.push(Quad::rounded(
                rect.x + 13.0 * scale,
                rect.y + 16.0 * scale,
                dot,
                dot,
                dot / 2.0,
                to_linear(color, 1.0),
            ));
            quads.push(Quad::rounded(
                rect.x + 28.0 * scale,
                rect.y + 10.0 * scale,
                (20.0 * scale).max(16.0),
                (20.0 * scale).max(16.0),
                4.0 * scale,
                to_linear(theme.background, 1.0),
            ));
            labels.push(Label {
                text: row.kind.badge().into(),
                x: rect.x + 34.0 * scale,
                y: rect.y + 11.0 * scale,
                width: 16.0 * scale,
                font_size: font * 0.82,
                color,
                bold: true,
            });

            let text_x = rect.x + 56.0 * scale;
            let text_width = (rect.right() - text_x - 10.0 * scale).max(1.0);
            let name = row.display_name();
            labels.push(Label {
                text: name,
                x: text_x,
                y: rect.y + 7.0 * scale,
                width: text_width,
                font_size: font,
                color: theme.sidebar_text,
                bold: true,
            });

            let state_text = row.state.label();
            labels.push(Label {
                text: state_text.into(),
                x: text_x,
                y: rect.y + 25.0 * scale,
                width: text_width,
                font_size: font * 0.72,
                color,
                bold: false,
            });

            let detail = row
                .message
                .as_deref()
                .or(row.title.as_deref())
                .map(str::to_owned)
                .or_else(|| {
                    row.cwd
                        .as_deref()
                        .and_then(Path::file_name)
                        .map(|name| name.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| "No details".into());
            let detail = truncate(&detail, text_width - 110.0 * scale, font * 0.72);
            labels.push(Label {
                text: detail,
                x: text_x,
                y: rect.y + 40.0 * scale,
                width: (text_width - 100.0 * scale).max(1.0),
                font_size: font * 0.7,
                color: theme.sidebar_text_dim,
                bold: false,
            });

            let right = if let Some(progress) = row.progress {
                format!("{progress}%")
            } else if row.is_external() {
                "external".into()
            } else if let Some(pane) = row.pane {
                format!("pane {}", pane.0)
            } else {
                String::new()
            };
            labels.push(Label {
                text: right,
                x: rect.right() - 70.0 * scale,
                y: rect.y + 7.0 * scale,
                width: 60.0 * scale,
                font_size: font * 0.68,
                color: theme.sidebar_text_dim,
                bold: false,
            });

            if !row.subagents.is_empty() {
                labels.push(Label {
                    text: format!("{} subagent(s)", row.subagents.len()),
                    x: rect.right() - 96.0 * scale,
                    y: rect.y + 25.0 * scale,
                    width: 86.0 * scale,
                    font_size: font * 0.66,
                    color: theme.accent,
                    bold: false,
                });
            }
        }
    }

    fn draw_detail(
        &mut self,
        layout: &Layout,
        theme: &Theme,
        font: f32,
        scale: f32,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let Some(row) = self.selected_row().cloned() else {
            labels.push(Label {
                text: "Select an agent to see its details".into(),
                x: layout.detail.x + layout.padding,
                y: layout.detail.y + layout.padding,
                width: (layout.detail.width - layout.padding * 2.0).max(1.0),
                font_size: font,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            return;
        };

        let x = layout.detail.x + layout.padding;
        let width = (layout.detail.width - layout.padding * 2.0).max(1.0);
        let mut y = layout.detail.y + layout.padding;

        labels.push(Label {
            text: row.display_name(),
            x,
            y,
            width,
            font_size: font * 1.25,
            color: theme.sidebar_text,
            bold: true,
        });
        y += font * 1.6;

        let color = status_color(theme, &row.state);
        quads.push(Quad::rounded(
            x,
            y + 2.0 * scale,
            (7.0 * scale).max(5.0),
            (7.0 * scale).max(5.0),
            3.5 * scale,
            to_linear(color, 1.0),
        ));
        labels.push(Label {
            text: row.state.label().into(),
            x: x + 12.0 * scale,
            y,
            width,
            font_size: font * 0.85,
            color,
            bold: true,
        });
        y += font * 1.55;

        let message = row
            .message
            .clone()
            .or_else(|| row.title.clone())
            .unwrap_or_else(|| "No message reported".into());
        labels.push(Label {
            text: truncate(&message, width, font),
            x,
            y,
            width,
            font_size: font * 0.86,
            color: theme.sidebar_text,
            bold: false,
        });
        y += font * 1.75;

        if let Some(progress) = row.progress {
            let bar_width = width;
            quads.push(Quad::rounded(
                x,
                y,
                bar_width,
                6.0 * scale,
                3.0 * scale,
                to_linear(theme.border, 1.0),
            ));
            quads.push(Quad::rounded(
                x,
                y,
                bar_width * (progress as f32 / 100.0),
                6.0 * scale,
                3.0 * scale,
                to_linear(theme.accent, 1.0),
            ));
            y += 16.0 * scale;
        }

        y += 4.0 * scale;
        let mut meta = Vec::new();
        meta.push(("Source", row.source.label().to_owned()));
        if let Some(pid) = row.pid {
            meta.push(("PID", pid.to_string()));
        }
        if let Some(pane) = row.pane {
            meta.push(("Pane", pane.0.to_string()));
        }
        if let Some(tab) = &row.tab_title {
            meta.push(("Session", tab.clone()));
        }
        if let Some(label) = &row.pane_label {
            meta.push(("Terminal", label.clone()));
        }
        if row.started > 0 {
            meta.push(("Uptime", duration_since(row.started)));
        }
        if let Some(cwd) = &row.cwd {
            meta.push(("Directory", cwd.display().to_string()));
        }

        for (key, value) in meta {
            labels.push(Label {
                text: key.into(),
                x,
                y,
                width: (width * 0.29).max(1.0),
                font_size: font * 0.76,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            labels.push(Label {
                text: truncate(&value, width * 0.69, font * 0.76),
                x: x + width * 0.3,
                y,
                width: (width * 0.7).max(1.0),
                font_size: font * 0.76,
                color: theme.sidebar_text,
                bold: false,
            });
            y += font * 1.55;
        }

        if !row.subagents.is_empty() {
            y += font * 0.5;
            labels.push(Label {
                text: format!("SUBAGENTS ({})", row.subagents.len()),
                x,
                y,
                width,
                font_size: font * 0.74,
                color: theme.sidebar_text_dim,
                bold: true,
            });
            y += font * 1.45;
            for child in row.subagents.iter().take(6) {
                let color = status_color(theme, &child.state);
                quads.push(Quad::rounded(
                    x,
                    y + 3.0 * scale,
                    (6.0 * scale).max(4.0),
                    (6.0 * scale).max(4.0),
                    3.0 * scale,
                    to_linear(color, 1.0),
                ));
                let label = child.message.clone().unwrap_or_else(|| child.id.clone());
                let progress = child
                    .progress
                    .map(|progress| format!(" · {progress}%"))
                    .unwrap_or_default();
                labels.push(Label {
                    text: truncate(
                        &format!("{} · {}{}", child.kind.label(), label, progress),
                        width - 14.0 * scale,
                        font * 0.74,
                    ),
                    x: x + 12.0 * scale,
                    y,
                    width: (width - 12.0 * scale).max(1.0),
                    font_size: font * 0.74,
                    color: theme.sidebar_text_dim,
                    bold: false,
                });
                y += font * 1.4;
            }
        }

        let action_y = (layout.footer.y - 44.0 * scale).max(y + 8.0 * scale);
        if let Some(pane) = row.pane {
            let focus_width = if row.pid.is_some() {
                (layout.kill.x - x - 8.0 * scale).max(1.0)
            } else {
                (180.0 * scale).max(140.0)
            };
            quads.push(Quad::rounded(
                x,
                action_y,
                focus_width,
                (26.0 * scale).max(22.0),
                4.0 * scale,
                to_linear(theme.accent, 0.18),
            ));
            labels.push(Label {
                text: format!("ENTER · focus pane {}", pane.0),
                x: x + 10.0 * scale,
                y: action_y + 5.0 * scale,
                width: (focus_width - 12.0 * scale).max(1.0),
                font_size: font * 0.72,
                color: theme.accent,
                bold: true,
            });
        } else {
            labels.push(Label {
                text: "External process · not focusable".into(),
                x,
                y: action_y,
                width: (width - 190.0 * scale).max(1.0),
                font_size: font * 0.78,
                color: theme.sidebar_text_dim,
                bold: false,
            });
        }

        if let Some(pid) = row.pid {
            let armed = self
                .kill_armed
                .as_ref()
                .is_some_and(|(id, at)| id == &row.id && at.elapsed() <= Duration::from_secs(4));
            let color = theme.ansi[1];
            quads.push(Quad::rounded(
                layout.kill.x,
                layout.kill.y,
                layout.kill.width,
                layout.kill.height,
                4.0 * scale,
                to_linear(color, if armed { 0.32 } else { 0.14 }),
            ));
            labels.push(Label {
                text: if armed {
                    format!("CONFIRM KILL · PID {pid}")
                } else {
                    format!("KILL PID {pid}")
                },
                x: layout.kill.x + 9.0 * scale,
                y: layout.kill.y + 5.0 * scale,
                width: (layout.kill.width - 16.0 * scale).max(1.0),
                font_size: font * 0.7,
                color,
                bold: true,
            });
        }
    }
}

fn status_color(theme: &Theme, state: &AgentState) -> Rgb {
    match state {
        AgentState::Working => theme.accent,
        AgentState::Blocked(_) | AgentState::Attention => theme.ansi[3],
        AgentState::Error => theme.ansi[1],
        AgentState::Done => theme.ansi[2],
        AgentState::Idle | AgentState::Detected => theme.sidebar_text_dim,
    }
}

fn truncate(text: &str, max_width: f32, font_size: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    // Labels are shaped with a real font later; this approximation is only
    // used to avoid obvious overflow in the list.
    let char_width = (font_size * 0.56).max(1.0);
    let max_chars = (max_width / char_width).floor().max(1.0) as usize;
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

fn elapsed_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 1 {
        "just now".into()
    } else if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else {
        format!("{}h ago", seconds / 3600)
    }
}

fn duration_since(started: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(started);
    let seconds = now.saturating_sub(started);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}
