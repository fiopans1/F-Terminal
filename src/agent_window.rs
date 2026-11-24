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
use crate::ui::{self, ButtonKind, ButtonSpec, Tokens};

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
            Self::All => "Todos",
            Self::Attention => "Atención",
            Self::Running => "Activos",
            Self::Finished => "Terminados",
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
    focus_action: Rect,
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

    pub fn apply_config(&mut self, config: &Config) {
        self.renderer.refresh_font_metrics(config);
        self.window.request_redraw();
    }

    /// Paint once while the window is hidden, then show it. This mirrors the
    /// main window and avoids the white flash Windows shows before the first
    /// GPU frame.
    pub fn reveal(&mut self, config: &Config) {
        self.draw(config);
        self.window.set_visible(true);
        self.draw(config);
        self.focus();
        self.window.request_redraw();
    }

    pub fn apply_snapshot(&mut self, snapshot: AgentSnapshot) {
        let selected = self.selected.clone();
        let changed = self.snapshot != snapshot;
        let generated_changed = self.snapshot.generated != snapshot.generated;
        self.snapshot = snapshot;
        self.window.set_title(&format!(
            "Agentes — {} activos · {} requieren atención",
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

        if layout
            .focus_action
            .contains(self.cursor.x as f32, self.cursor.y as f32)
        {
            self.focus_selected();
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
                text: "Este agente no tiene un proceso que matar".into(),
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
            text: format!("Deteniendo PID {pid}…"),
            at: Instant::now(),
            error: false,
        });
        self.window.request_redraw();
    }

    pub fn on_agent_killed(&mut self, pid: u32, result: &Result<String, String>) {
        let (text, error) = match result {
            Ok(message) => {
                let message = match message.as_str() {
                    "terminated" => "terminado",
                    "force-killed" => "terminado a la fuerza",
                    other => other,
                };
                (format!("PID {pid}: {message}"), false)
            }
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
        let padding = ui::px(ui::GUTTER, scale);
        let header_height = ui::px(ui::HEADER_HEIGHT, scale);
        let footer_height = ui::px(ui::FOOTER_HEIGHT, scale);
        let close_size = ui::px(ui::CLOSE_SIZE, scale);
        let body_top = header_height;
        let body_bottom = (height - footer_height).max(body_top + 1.0);
        let list_width = (width * 0.52).clamp(ui::px(280.0, scale).min(width), width * 0.64);
        let list = Rect::new(0.0, body_top, list_width, body_bottom - body_top);
        let detail = Rect::new(
            list_width,
            body_top,
            (width - list_width).max(1.0),
            body_bottom - body_top,
        );

        let tab_height = ui::px(30.0, scale);
        let tab_width = ((list_width - padding * 2.0) / 4.0).max(1.0);
        let tabs = std::array::from_fn(|index| {
            Rect::new(
                padding + index as f32 * tab_width,
                body_top + ui::px(8.0, scale),
                tab_width,
                tab_height,
            )
        });

        let list_rows = Rect::new(
            list.x,
            tabs[0].bottom() + ui::px(8.0, scale),
            list.width,
            (body_bottom - tabs[0].bottom() - ui::px(8.0, scale)).max(1.0),
        );

        let close = Rect::new(
            (width - padding - close_size).max(0.0),
            ((header_height - close_size) / 2.0).max(0.0),
            close_size,
            close_size,
        );
        let refresh_width = ui::px(112.0, scale);
        let refresh = Rect::new(
            (close.x - refresh_width - ui::px(8.0, scale)).max(padding),
            close.y,
            refresh_width,
            close_size,
        );
        let action_height = ui::px(ui::BUTTON_HEIGHT, scale);
        let action_gap = ui::px(8.0, scale);
        let stacked = detail.width < ui::px(430.0, scale);
        let (focus_action, kill) = if stacked {
            let kill_y = (body_bottom - action_height - ui::px(12.0, scale))
                .max(detail.y + padding + action_height * 2.0);
            let action_width = (detail.width - padding * 2.0).max(1.0);
            (
                Rect::new(
                    detail.x + padding,
                    kill_y - action_height - action_gap,
                    action_width,
                    action_height,
                ),
                Rect::new(detail.x + padding, kill_y, action_width, action_height),
            )
        } else {
            let action_y = (body_bottom - action_height - ui::px(14.0, scale))
                .max(detail.y + padding + action_height);
            let kill_width = ui::px(170.0, scale).min((detail.width - padding * 3.0).max(1.0));
            (
                Rect::new(
                    detail.x + padding,
                    action_y,
                    ui::px(180.0, scale)
                        .min((detail.width - padding * 2.0 - kill_width - action_gap).max(1.0)),
                    action_height,
                ),
                Rect::new(
                    (detail.right() - padding - kill_width).max(detail.x + padding),
                    action_y,
                    kill_width,
                    action_height,
                ),
            )
        };

        Layout {
            header: Rect::new(0.0, 0.0, width, header_height),
            footer: Rect::new(0.0, body_bottom, width, footer_height),
            list,
            detail,
            close,
            refresh,
            focus_action,
            kill,
            tabs,
            list_rows,
            padding,
            row_height: ui::px(64.0, scale),
        }
    }

    fn draw(&mut self, config: &Config) {
        let theme = &config.theme;
        let scale = self.renderer.scale_factor() as f32;
        let ui_font = self.renderer.physical(config.ui_font_size);
        let tokens = Tokens::new(ui_font, scale);
        let layout = self.layout(config);
        let width = self.window.inner_size().width.max(1) as f32;
        let height = self.window.inner_size().height.max(1) as f32;
        let cursor = (self.cursor.x as f32, self.cursor.y as f32);

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
            text: "Agentes".into(),
            x: layout.padding,
            y: ui::px(9.0, scale),
            width: (layout.refresh.x - layout.padding * 2.0).max(1.0),
            font_size: tokens.title,
            color: theme.sidebar_text,
            bold: true,
        });
        let attention = self.snapshot.attention_count();
        let running = self.snapshot.active_count();
        let subtitle = if self.snapshot.rows.is_empty() {
            "Esperando a que aparezca un agente…".to_owned()
        } else {
            format!(
                "{} activos · {} requieren atención · {} detectados",
                running,
                attention,
                self.snapshot.rows.len()
            )
        };
        labels.push(Label {
            text: subtitle,
            x: layout.padding,
            y: ui::px(34.0, scale),
            width: (layout.refresh.x - layout.padding * 2.0).max(1.0),
            font_size: tokens.small,
            color: if attention > 0 {
                status_color(theme, &AgentState::Attention)
            } else {
                theme.sidebar_text_dim
            },
            bold: false,
        });

        ui::icon_button(
            &mut quads,
            &mut labels,
            ui::IconButtonSpec {
                rect: layout.close,
                glyph: "×",
                enabled: true,
                hovered: layout.close.contains(cursor.0, cursor.1),
                focused: false,
                danger: false,
            },
            theme,
            tokens,
        );

        let refreshing = self
            .refreshing_until
            .is_some_and(|until| Instant::now() < until);
        ui::button(
            &mut quads,
            &mut labels,
            ButtonSpec {
                rect: layout.refresh,
                label: if refreshing {
                    "Actualizando…"
                } else {
                    "Actualizar"
                },
                kind: ButtonKind::Ghost,
                enabled: !refreshing,
                hovered: layout.refresh.contains(cursor.0, cursor.1),
                focused: false,
            },
            theme,
            tokens,
        );

        // Body backgrounds.
        quads.push(Quad::new(
            layout.list.x,
            layout.list.y,
            layout.list.width,
            layout.list.height,
            to_linear(theme.sidebar_background, 0.72),
        ));
        quads.push(Quad::new(
            layout.detail.x,
            layout.detail.y,
            1.0,
            layout.detail.height,
            to_linear(theme.border, 1.0),
        ));

        self.draw_filters(&layout, theme, tokens, &mut quads, &mut labels);
        self.draw_list(&layout, theme, tokens, cursor, &mut quads, &mut labels);
        self.draw_detail(&layout, theme, tokens, cursor, &mut quads, &mut labels);

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
                    .unwrap_or_else(|| "nunca".into());
                (
                    format!(
                        "↑↓ seleccionar · Enter enfocar · R actualizar · X matar · Tab filtrar · Esc cerrar · actualizado {updated}"
                    ),
                    theme.sidebar_text_dim,
                )
            }
        };
        labels.push(Label {
            text: hint,
            x: layout.padding,
            y: layout.footer.y + ui::px(6.0, scale),
            width: (width - layout.padding * 2.0).max(1.0),
            font_size: tokens.caption,
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
        tokens: Tokens,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        let cursor = (self.cursor.x as f32, self.cursor.y as f32);
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
            let hovered = rect.contains(cursor.0, cursor.1);
            quads.push(Quad::rounded(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                4.0 * scale,
                to_linear(
                    if active {
                        theme.sidebar_active
                    } else if hovered {
                        theme.sidebar_hover
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
            let compact = rect.width < ui::px(96.0, scale);
            labels.push(Label {
                text: if compact {
                    filter.label().to_owned()
                } else {
                    format!("{} {}", filter.label(), counts[index])
                },
                x: rect.x + 8.0 * scale,
                y: rect.y + (rect.height - tokens.caption * 1.35) / 2.0,
                width: (rect.width - 10.0 * scale).max(1.0),
                font_size: tokens.caption,
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
        tokens: Tokens,
        cursor: (f32, f32),
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        let rows: Vec<AgentRow> = self.filtered().into_iter().cloned().collect();
        if rows.is_empty() {
            if self.snapshot.rows.is_empty()
                && self
                    .refreshing_until
                    .is_some_and(|until| Instant::now() < until)
            {
                ui::skeleton_rows(quads, layout.list_rows, 4, theme, scale);
                return;
            }
            let (title, body) = if self.snapshot.rows.is_empty() {
                (
                    "Sin agentes",
                    "Abre un agente en una terminal o pulsa Actualizar para escanear de nuevo.",
                )
            } else {
                (
                    "Sin resultados",
                    "No hay agentes en este filtro. Cambia el filtro o actualiza la lista.",
                )
            };
            ui::empty_state(quads, labels, layout.list, title, body, theme, tokens);
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
                layout.list.x + 8.0 * scale,
                y,
                (layout.list.width - 16.0 * scale).max(1.0),
                layout.row_height - 6.0 * scale,
            );
            let selected = self.selected.as_ref() == Some(&row.id);
            let hovered =
                self.hovered.as_ref() == Some(&row.id) || rect.contains(cursor.0, cursor.1);
            if selected || hovered {
                quads.push(Quad::rounded(
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    6.0 * scale,
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
                    (2.0 * scale).max(2.0),
                    rect.height,
                    1.0 * scale,
                    to_linear(theme.accent, 1.0),
                ));
            }

            let color = status_color(theme, &row.state);
            let dot = (8.0 * scale).max(6.0);
            ui::status_dot(
                quads,
                rect.x + 16.0 * scale,
                rect.y + 22.0 * scale,
                dot,
                color,
                matches!(
                    row.state,
                    AgentState::Working | AgentState::Blocked(_) | AgentState::Attention
                ),
            );
            quads.push(Quad::rounded(
                rect.x + 32.0 * scale,
                rect.y + 12.0 * scale,
                (22.0 * scale).max(18.0),
                (22.0 * scale).max(18.0),
                5.0 * scale,
                to_linear(theme.background, 1.0),
            ));
            labels.push(Label {
                text: row.kind.badge().into(),
                x: rect.x + 37.0 * scale,
                y: rect.y + 13.0 * scale,
                width: 18.0 * scale,
                font_size: tokens.caption,
                color,
                bold: true,
            });

            let text_x = rect.x + 64.0 * scale;
            let meta_width = 86.0 * scale;
            let text_width = (rect.right() - text_x - meta_width - 8.0 * scale).max(1.0);
            labels.push(Label {
                text: row.display_name(),
                x: text_x,
                y: rect.y + 9.0 * scale,
                width: text_width,
                font_size: tokens.body,
                color: theme.sidebar_text,
                bold: true,
            });

            labels.push(Label {
                text: row.state.label().into(),
                x: text_x,
                y: rect.y + 29.0 * scale,
                width: text_width,
                font_size: tokens.caption,
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
                .unwrap_or_else(|| "Sin detalles".into());
            let detail = truncate(&detail, text_width, tokens.caption);
            labels.push(Label {
                text: detail,
                x: text_x,
                y: rect.y + 46.0 * scale,
                width: text_width,
                font_size: tokens.caption,
                color: theme.sidebar_text_dim,
                bold: false,
            });

            let right = if let Some(progress) = row.progress {
                format!("{progress}%")
            } else if row.is_external() {
                "externo".into()
            } else if let Some(pane) = row.pane {
                format!("pane {}", pane.0)
            } else {
                String::new()
            };
            labels.push(Label {
                text: right,
                x: rect.right() - meta_width,
                y: rect.y + 9.0 * scale,
                width: meta_width - 8.0 * scale,
                font_size: tokens.caption,
                color: theme.sidebar_text_dim,
                bold: false,
            });

            if !row.subagents.is_empty() {
                labels.push(Label {
                    text: format!("{} sub", row.subagents.len()),
                    x: rect.right() - meta_width,
                    y: rect.y + 29.0 * scale,
                    width: meta_width - 8.0 * scale,
                    font_size: tokens.caption,
                    color: theme.accent,
                    bold: false,
                });
            }
        }

        ui::scrollbar(
            quads,
            layout.list_rows,
            self.scroll,
            rows.len(),
            visible,
            theme,
            scale,
        );
    }

    fn draw_detail(
        &mut self,
        layout: &Layout,
        theme: &Theme,
        tokens: Tokens,
        cursor: (f32, f32),
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        let Some(row) = self.selected_row().cloned() else {
            ui::empty_state(
                quads,
                labels,
                layout.detail,
                "Detalle del agente",
                "Selecciona un agente de la lista para ver su estado, proceso y acciones.",
                theme,
                tokens,
            );
            return;
        };

        let x = layout.detail.x + layout.padding;
        let width = (layout.detail.width - layout.padding * 2.0).max(1.0);
        let mut y = layout.detail.y + ui::px(12.0, scale);

        labels.push(Label {
            text: row.display_name(),
            x,
            y,
            width,
            font_size: tokens.section,
            color: theme.sidebar_text,
            bold: true,
        });
        y += tokens.section * 1.6;

        let color = status_color(theme, &row.state);
        ui::status_dot(
            quads,
            x,
            y + ui::px(4.0, scale),
            (8.0 * scale).max(6.0),
            color,
            matches!(
                row.state,
                AgentState::Working | AgentState::Blocked(_) | AgentState::Attention
            ),
        );
        labels.push(Label {
            text: row.state.label().into(),
            x: x + ui::px(14.0, scale),
            y,
            width,
            font_size: tokens.body,
            color,
            bold: true,
        });
        y += tokens.body * 1.6;

        let message = row
            .message
            .clone()
            .or_else(|| row.title.clone())
            .unwrap_or_else(|| "Sin mensaje".into());
        labels.push(Label {
            text: truncate(&message, width, tokens.small),
            x,
            y,
            width,
            font_size: tokens.small,
            color: theme.sidebar_text,
            bold: false,
        });
        y += tokens.small * 1.9;

        if let Some(progress) = row.progress {
            quads.push(Quad::rounded(
                x,
                y,
                width,
                ui::px(5.0, scale),
                ui::px(2.5, scale),
                to_linear(theme.border, 1.0),
            ));
            quads.push(Quad::rounded(
                x,
                y,
                width * (progress as f32 / 100.0),
                ui::px(5.0, scale),
                ui::px(2.5, scale),
                to_linear(theme.ansi[6], 1.0),
            ));
            y += ui::px(16.0, scale);
        }

        y += ui::px(6.0, scale);
        let content_bottom = layout.focus_action.y - ui::px(8.0, scale);
        let mut meta = Vec::new();
        meta.push(("Origen", row.source.label().to_owned()));
        if let Some(pid) = row.pid {
            meta.push(("PID", pid.to_string()));
        }
        if let Some(pane) = row.pane {
            meta.push(("Pane", pane.0.to_string()));
        }
        if let Some(tab) = &row.tab_title {
            meta.push(("Sesión", tab.clone()));
        }
        if let Some(label) = &row.pane_label {
            meta.push(("Terminal", label.clone()));
        }
        if row.started > 0 {
            meta.push(("Tiempo", duration_since(row.started)));
        }
        if let Some(cwd) = &row.cwd {
            meta.push(("Directorio", cwd.display().to_string()));
        }

        for (key, value) in meta {
            if y + tokens.caption * 1.65 > content_bottom {
                break;
            }
            labels.push(Label {
                text: key.into(),
                x,
                y,
                width: (width * 0.28).max(1.0),
                font_size: tokens.caption,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            labels.push(Label {
                text: truncate(&value, width * 0.7, tokens.caption),
                x: x + width * 0.3,
                y,
                width: (width * 0.7).max(1.0),
                font_size: tokens.caption,
                color: theme.sidebar_text,
                bold: false,
            });
            y += tokens.caption * 1.65;
        }

        if !row.subagents.is_empty() && y + tokens.caption * 2.3 <= content_bottom {
            y += tokens.caption * 0.7;
            labels.push(Label {
                text: format!("Subagentes ({})", row.subagents.len()),
                x,
                y,
                width,
                font_size: tokens.caption,
                color: theme.sidebar_text_dim,
                bold: true,
            });
            y += tokens.caption * 1.6;
            let max_children = (((content_bottom - y) / (tokens.caption * 1.55))
                .floor()
                .max(0.0) as usize)
                .min(6);
            for child in row.subagents.iter().take(max_children) {
                let color = status_color(theme, &child.state);
                ui::status_dot(
                    quads,
                    x,
                    y + ui::px(4.0, scale),
                    (6.0 * scale).max(4.0),
                    color,
                    false,
                );
                let label = child.message.clone().unwrap_or_else(|| child.id.clone());
                let progress = child
                    .progress
                    .map(|progress| format!(" · {progress}%"))
                    .unwrap_or_default();
                labels.push(Label {
                    text: truncate(
                        &format!("{} · {}{}", child.kind.label(), label, progress),
                        width - ui::px(14.0, scale),
                        tokens.caption,
                    ),
                    x: x + ui::px(12.0, scale),
                    y,
                    width: (width - ui::px(12.0, scale)).max(1.0),
                    font_size: tokens.caption,
                    color: theme.sidebar_text_dim,
                    bold: false,
                });
                y += tokens.caption * 1.55;
            }
        }

        if let Some(pane) = row.pane {
            let label = format!("Enter · enfocar pane {}", pane.0);
            ui::button(
                quads,
                labels,
                ButtonSpec {
                    rect: layout.focus_action,
                    label: &label,
                    kind: ButtonKind::Primary,
                    enabled: true,
                    hovered: layout.focus_action.contains(cursor.0, cursor.1),
                    focused: false,
                },
                theme,
                tokens,
            );
        } else {
            labels.push(Label {
                text: "Proceso externo · no se puede enfocar".into(),
                x: layout.focus_action.x,
                y: layout.focus_action.y + ui::px(6.0, scale),
                width: layout.focus_action.width,
                font_size: tokens.caption,
                color: theme.sidebar_text_dim,
                bold: false,
            });
        }

        if let Some(pid) = row.pid {
            let armed = self
                .kill_armed
                .as_ref()
                .is_some_and(|(id, at)| id == &row.id && at.elapsed() <= Duration::from_secs(4));
            let label = if armed {
                format!("Confirmar · PID {pid}")
            } else {
                format!("Matar PID {pid}")
            };
            ui::button(
                quads,
                labels,
                ButtonSpec {
                    rect: layout.kill,
                    label: &label,
                    kind: ButtonKind::Danger,
                    enabled: true,
                    hovered: layout.kill.contains(cursor.0, cursor.1),
                    focused: false,
                },
                theme,
                tokens,
            );
        }
    }
}

fn status_color(theme: &Theme, state: &AgentState) -> Rgb {
    match state {
        AgentState::Working => theme.ansi[6],
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
        "ahora".into()
    } else if seconds < 60 {
        format!("hace {seconds}s")
    } else if seconds < 3600 {
        format!("hace {}m", seconds / 60)
    } else {
        format!("hace {}h", seconds / 3600)
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
