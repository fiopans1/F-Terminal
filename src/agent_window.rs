use std::cmp::Reverse;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::agents::{AgentId, AgentKind, AgentRow, AgentSnapshot, AgentState};
use crate::config::Config;
use crate::event::UserEvent;
use crate::layout::Rect;
use crate::render::Renderer;
use crate::render::quads::Quad;
use crate::render::scene::{Label, Scene};
use crate::term::color::{Rgb, Theme, to_linear};
use crate::ui::{self, ButtonKind, ButtonSpec, IconButtonSpec, Tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filter {
    All,
    Attention,
    Running,
    Finished,
}

impl Filter {
    const ALL: [Self; 4] = [Self::All, Self::Attention, Self::Running, Self::Finished];

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

    fn prev(self) -> Self {
        match self {
            Self::All => Self::Finished,
            Self::Attention => Self::All,
            Self::Running => Self::Attention,
            Self::Finished => Self::Running,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OriginFilter {
    All,
    Internal,
    External,
}

impl OriginFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "Origen: Todos",
            Self::Internal => "Origen: Internos",
            Self::External => "Origen: Externos",
        }
    }

    fn matches(self, row: &AgentRow) -> bool {
        match self {
            Self::All => true,
            Self::Internal => !row.is_external(),
            Self::External => row.is_external(),
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Internal,
            Self::Internal => Self::External,
            Self::External => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortBy {
    Priority,
    Cpu,
    Memory,
    Uptime,
    Name,
}

impl SortBy {
    fn label(self) -> &'static str {
        match self {
            Self::Priority => "Orden: Prioridad",
            Self::Cpu => "Orden: CPU",
            Self::Memory => "Orden: RAM",
            Self::Uptime => "Orden: Tiempo",
            Self::Name => "Orden: Nombre",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Priority => Self::Cpu,
            Self::Cpu => Self::Memory,
            Self::Memory => Self::Uptime,
            Self::Uptime => Self::Name,
            Self::Name => Self::Priority,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorTab {
    Overview,
    Console,
    Subagents,
}

impl InspectorTab {
    const ALL: [Self; 3] = [Self::Overview, Self::Console, Self::Subagents];

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Resumen",
            Self::Console => "Consola",
            Self::Subagents => "Subagentes",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Overview => Self::Console,
            Self::Console => Self::Subagents,
            Self::Subagents => Self::Overview,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Overview => Self::Subagents,
            Self::Console => Self::Overview,
            Self::Subagents => Self::Console,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputFocus {
    None,
    Search,
    Prompt,
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
    launcher_bar: Option<Rect>,
    launcher_presets: [Rect; 4],
    kpi_bar: Rect,
    kpi_cards: [Rect; 4],
    footer: Rect,
    list: Rect,
    detail: Rect,
    close: Rect,
    refresh: Rect,
    mark_all: Rect,
    launch_toggle: Rect,
    search_box: Rect,
    search_clear: Rect,
    origin_chip: Rect,
    sort_chip: Rect,
    tabs: [Rect; 4],
    list_rows: Rect,
    inspector_tabs: [Rect; 3],
    detail_content: Rect,
    prompt_box: Rect,
    prompt_send: Rect,
    quick_yes: Rect,
    quick_enter: Rect,
    quick_ctrl_c: Rect,
    focus_action: Rect,
    git_action: Rect,
    review_action: Rect,
    copy_action: Rect,
    seen_action: Rect,
    kill: Rect,
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
    snapshot: Arc<AgentSnapshot>,
    filter: Filter,
    origin: OriginFilter,
    sort: SortBy,
    inspector: InspectorTab,
    input_focus: InputFocus,
    search: String,
    prompt: String,
    launcher_open: bool,
    selected: Option<AgentId>,
    hovered: Option<AgentId>,
    scroll: usize,
    console_scroll: usize,
    last_click: Option<(Instant, AgentId)>,
    close_requested: bool,
    last_tick: Instant,
    refreshing_until: Option<Instant>,
    kill_armed: Option<(AgentId, Instant)>,
    notice: Option<Notice>,
    clipboard: Option<arboard::Clipboard>,
}

impl AgentWindow {
    pub fn new(window: Arc<Window>, renderer: Renderer, proxy: EventLoopProxy<UserEvent>) -> Self {
        window.set_ime_allowed(true);
        Self {
            window,
            renderer,
            proxy,
            modifiers: ModifiersState::empty(),
            cursor: PhysicalPosition::new(0.0, 0.0),
            snapshot: Arc::new(AgentSnapshot::default()),
            filter: Filter::All,
            origin: OriginFilter::All,
            sort: SortBy::Priority,
            inspector: InspectorTab::Overview,
            input_focus: InputFocus::None,
            search: String::new(),
            prompt: String::new(),
            launcher_open: false,
            selected: None,
            hovered: None,
            scroll: 0,
            console_scroll: 0,
            last_click: None,
            close_requested: false,
            last_tick: Instant::now(),
            refreshing_until: None,
            kill_armed: None,
            notice: None,
            clipboard: None,
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

    pub fn apply_snapshot(&mut self, snapshot: Arc<AgentSnapshot>) {
        if Arc::ptr_eq(&self.snapshot, &snapshot) {
            return;
        }
        let selected = self.selected.clone();
        let changed = self.snapshot.rows != snapshot.rows;
        self.snapshot = snapshot;
        self.window.set_title(&format!(
            "Centro de Agentes — {} activos · {} atención · {}",
            self.snapshot.active_count(),
            self.snapshot.attention_count(),
            format_memory(self.snapshot.total_memory_bytes()),
        ));
        let filtered = self.filtered();
        let next = selected
            .as_ref()
            .filter(|id| filtered.iter().any(|row| &row.id == *id))
            .cloned()
            .or_else(|| filtered.first().map(|row| row.id.clone()));
        self.set_selected(next);
        if self
            .kill_armed
            .as_ref()
            .is_some_and(|(id, _)| !self.snapshot.rows.iter().any(|row| &row.id == id))
        {
            self.kill_armed = None;
        }
        self.clamp_selection();
        if changed {
            self.window.request_redraw();
        }
    }

    fn set_selected(&mut self, next: Option<AgentId>) {
        if self.selected != next {
            self.prompt.clear();
            self.console_scroll = 0;
        }
        self.selected = next;
    }

    pub fn tick(&mut self) {
        if self.window.is_minimized().unwrap_or(false) {
            return;
        }
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
            WindowEvent::Occluded(false) => self.window.request_redraw(),
            WindowEvent::RedrawRequested => self.draw(config),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => self.on_key(event, config),
            WindowEvent::Ime(Ime::Commit(text)) => self.on_ime(&text),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
                let hovered = self.row_at(position, config);
                if hovered != self.hovered {
                    self.hovered = hovered;
                    self.window.request_redraw();
                } else {
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
                let layout = self.layout(config);
                let over_detail = layout
                    .detail
                    .contains(self.cursor.x as f32, self.cursor.y as f32);
                if over_detail {
                    if self.inspector == InspectorTab::Console {
                        let total = self
                            .selected_row()
                            .map(|row| row.preview_lines.len())
                            .unwrap_or(0);
                        let capacity = self.console_capacity(config);
                        let max = total.saturating_sub(capacity);
                        if lines > 0.0 {
                            self.console_scroll =
                                (self.console_scroll + lines.abs().ceil() as usize).min(max);
                        } else {
                            self.console_scroll = self
                                .console_scroll
                                .saturating_sub(lines.abs().ceil() as usize);
                        }
                    }
                } else {
                    let max = self.max_scroll(config);
                    if lines > 0.0 {
                        self.scroll = self.scroll.saturating_sub(lines.abs().ceil() as usize);
                    } else {
                        self.scroll = (self.scroll + lines.abs().ceil() as usize).min(max);
                    }
                }
                self.window.request_redraw();
            }
            _ => {}
        }
    }

    fn on_ime(&mut self, text: &str) {
        if text.is_empty() || text.chars().any(char::is_control) {
            return;
        }
        match self.input_focus {
            InputFocus::Search => {
                let room = 256usize.saturating_sub(self.search.chars().count());
                if room > 0 {
                    self.search.extend(text.chars().take(room));
                    self.after_filter_change();
                }
            }
            InputFocus::Prompt => {
                let room = 4096usize.saturating_sub(self.prompt.chars().count());
                if room > 0 {
                    self.prompt.extend(text.chars().take(room));
                    self.window.request_redraw();
                }
            }
            InputFocus::None => {}
        }
    }

    fn on_key(&mut self, event: KeyEvent, _config: &Config) {
        if event.state != ElementState::Pressed {
            return;
        }
        let ctrl = self.modifiers.control_key();
        let alt = self.modifiers.alt_key();
        let shift = self.modifiers.shift_key();
        let super_key = self.modifiers.super_key();
        let command = if cfg!(target_os = "macos") {
            super_key && !ctrl
        } else {
            ctrl
        };
        let app_mod = if cfg!(target_os = "macos") {
            super_key && !ctrl
        } else {
            ctrl
        };
        if app_mod
            && !alt
            && matches!(&event.logical_key, Key::Character(ch) if ch.eq_ignore_ascii_case("w"))
        {
            self.close_requested = true;
            return;
        }

        if command && !alt && matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyF)) {
            self.input_focus = InputFocus::Search;
            self.window.request_redraw();
            return;
        }

        if self.input_focus != InputFocus::None {
            self.on_text_input_key(&event, command, alt);
            self.window.request_redraw();
            return;
        }

        if self.launcher_open {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.launcher_open = false;
                    self.window.request_redraw();
                    return;
                }
                Key::Character(ch) if !ctrl && !alt => {
                    let presets = AgentKind::launch_presets();
                    let idx = match ch.as_str() {
                        "1" => Some(0),
                        "2" => Some(1),
                        "3" => Some(2),
                        "4" => Some(3),
                        _ => None,
                    };
                    if let Some(index) = idx {
                        self.launch_preset(presets[index].clone());
                        self.window.request_redraw();
                        return;
                    }
                }
                _ => {}
            }
        }

        match event.logical_key {
            Key::Named(NamedKey::Escape) => {
                if !self.search.is_empty() {
                    self.search.clear();
                    self.after_filter_change();
                } else {
                    self.close_requested = true;
                }
            }
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
                    self.filter.prev()
                } else {
                    self.filter.next()
                };
                self.after_filter_change();
            }
            Key::Character(character) if !ctrl && !alt => match character.as_str() {
                "/" => self.input_focus = InputFocus::Search,
                "i" | "I" => self.focus_prompt(),
                "y" | "Y" => self.approve_selected(),
                "f" | "F" => {
                    self.filter = self.filter.next();
                    self.after_filter_change();
                }
                "s" | "S" => {
                    self.sort = self.sort.next();
                    self.after_filter_change();
                }
                "e" | "E" => {
                    self.origin = self.origin.next();
                    self.after_filter_change();
                }
                "[" => self.inspector = self.inspector.prev(),
                "]" => self.inspector = self.inspector.next(),
                "n" | "N" => self.launcher_open = !self.launcher_open,
                "g" | "G" => self.open_git_for_selected(),
                "v" | "V" => self.open_review_for_selected(),
                "c" | "C" => self.copy_selected_cwd(),
                "m" | "M" => self.mark_selected_seen(),
                "a" | "A" => self.mark_all_seen(),
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
        self.window.request_redraw();
    }

    fn on_text_input_key(&mut self, event: &KeyEvent, command: bool, alt: bool) {
        let paste = match event.physical_key {
            PhysicalKey::Code(KeyCode::KeyV) if command && !alt => true,
            PhysicalKey::Code(KeyCode::Insert) if self.modifiers.shift_key() => true,
            _ => false,
        };
        if paste {
            self.paste_input();
            return;
        }

        match (&self.input_focus, &event.logical_key) {
            (InputFocus::Search, Key::Named(NamedKey::Escape)) => {
                self.search.clear();
                self.input_focus = InputFocus::None;
                self.after_filter_change();
            }
            (InputFocus::Prompt, Key::Named(NamedKey::Escape)) => {
                self.prompt.clear();
                self.input_focus = InputFocus::None;
            }
            (InputFocus::Search, Key::Named(NamedKey::Backspace)) => {
                self.search.pop();
                self.after_filter_change();
            }
            (InputFocus::Search, Key::Named(NamedKey::Enter)) => {
                self.input_focus = InputFocus::None;
            }
            (InputFocus::Search, Key::Named(NamedKey::ArrowUp)) => self.move_selection(-1),
            (InputFocus::Search, Key::Named(NamedKey::ArrowDown)) => self.move_selection(1),
            (InputFocus::Search, Key::Named(NamedKey::Space)) => {
                if self.search.chars().count() < 256 {
                    self.search.push(' ');
                    self.after_filter_change();
                }
            }
            (InputFocus::Search, Key::Character(ch)) if !command && !alt => {
                let text = ch.as_str();
                if !text.is_empty()
                    && text.chars().all(|c| !c.is_control())
                    && self.search.chars().count() < 256
                {
                    self.search.push_str(text);
                    self.after_filter_change();
                }
            }
            (InputFocus::Prompt, Key::Named(NamedKey::Backspace)) => {
                self.prompt.pop();
            }
            (InputFocus::Prompt, Key::Named(NamedKey::Enter)) => {
                self.send_prompt();
            }
            (InputFocus::Prompt, Key::Named(NamedKey::Space)) => {
                if self.prompt.chars().count() < 4096 {
                    self.prompt.push(' ');
                }
            }
            (InputFocus::Prompt, Key::Character(ch)) if !command && !alt => {
                let text = ch.as_str();
                if !text.is_empty()
                    && text.chars().all(|c| !c.is_control())
                    && self.prompt.chars().count() < 4096
                {
                    self.prompt.push_str(text);
                }
            }
            _ => {}
        }
    }

    fn on_mouse(&mut self, state: ElementState, button: MouseButton, config: &Config) {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return;
        }
        let (mx, my) = (self.cursor.x as f32, self.cursor.y as f32);
        let layout = self.layout(config);
        self.input_focus = InputFocus::None;

        if layout.close.contains(mx, my) {
            self.close_requested = true;
            return;
        }
        if layout.refresh.contains(mx, my) {
            self.refresh();
            return;
        }
        if layout.mark_all.contains(mx, my) {
            self.mark_all_seen();
            return;
        }
        if layout.launch_toggle.contains(mx, my) {
            self.launcher_open = !self.launcher_open;
            self.input_focus = InputFocus::None;
            self.window.request_redraw();
            return;
        }

        if self.launcher_open {
            for (preset, rect) in AgentKind::launch_presets()
                .into_iter()
                .zip(layout.launcher_presets.iter())
            {
                if rect.contains(mx, my) {
                    self.launch_preset(preset);
                    return;
                }
            }
        }

        if layout.search_clear.contains(mx, my) && !self.search.is_empty() {
            self.search.clear();
            self.input_focus = InputFocus::Search;
            self.after_filter_change();
            return;
        }
        if layout.search_box.contains(mx, my) {
            self.input_focus = InputFocus::Search;
            self.window.request_redraw();
            return;
        }
        if layout.origin_chip.contains(mx, my) {
            self.origin = self.origin.next();
            self.after_filter_change();
            return;
        }
        if layout.sort_chip.contains(mx, my) {
            self.sort = self.sort.next();
            self.after_filter_change();
            return;
        }

        for (filter, rect) in Filter::ALL.iter().zip(layout.tabs.iter()) {
            if rect.contains(mx, my) {
                self.input_focus = InputFocus::None;
                self.set_filter(*filter);
                return;
            }
        }

        for (tab, rect) in InspectorTab::ALL.iter().zip(layout.inspector_tabs.iter()) {
            if rect.contains(mx, my) {
                self.input_focus = InputFocus::None;
                self.inspector = *tab;
                self.window.request_redraw();
                return;
            }
        }

        if self.selected_row().and_then(|row| row.pane).is_some() {
            if layout.prompt_box.contains(mx, my) {
                self.input_focus = InputFocus::Prompt;
                self.window.request_redraw();
                return;
            }
            if layout.prompt_send.contains(mx, my) {
                self.send_prompt();
                return;
            }
            if layout.quick_yes.contains(mx, my) {
                self.approve_selected();
                return;
            }
            if layout.quick_enter.contains(mx, my) {
                self.send_quick_bytes(b"\r".to_vec(), "Enter");
                return;
            }
            if layout.quick_ctrl_c.contains(mx, my) {
                self.send_quick_bytes(vec![0x03], "Ctrl+C");
                return;
            }
        }

        if layout.focus_action.contains(mx, my) {
            self.focus_selected();
            return;
        }
        if layout.git_action.contains(mx, my) {
            self.open_git_for_selected();
            return;
        }
        if layout.review_action.contains(mx, my) {
            self.open_review_for_selected();
            return;
        }
        if layout.copy_action.contains(mx, my) {
            self.copy_selected_cwd();
            return;
        }
        if layout.seen_action.contains(mx, my) {
            self.mark_selected_seen();
            return;
        }
        if self.selected_row().is_some_and(|row| row.pid.is_some()) && layout.kill.contains(mx, my)
        {
            self.kill_selected();
            return;
        }

        self.input_focus = InputFocus::None;

        let Some(id) = self.row_at(self.cursor, config) else {
            self.window.request_redraw();
            return;
        };
        let now = Instant::now();
        let double_click = self.last_click.as_ref().is_some_and(|(at, last)| {
            *last == id && now.duration_since(*at) < Duration::from_millis(400)
        });
        self.set_selected(Some(id.clone()));
        self.last_click = Some((now, id));
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
        let id = rows[next].id.clone();
        self.set_selected(Some(id));
        self.clamp_selection();
    }

    fn select_first(&mut self) {
        let id = self.filtered().first().map(|row| row.id.clone());
        self.set_selected(id);
        self.scroll = 0;
    }

    fn select_last(&mut self) {
        let id = self.filtered().last().map(|row| row.id.clone());
        self.set_selected(id);
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
        self.filtered_indices()
            .into_iter()
            .map(|index| &self.snapshot.rows[index])
            .collect()
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let query = self.search.trim().to_lowercase();
        let mut indices: Vec<usize> = self
            .snapshot
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                self.filter.matches(row) && self.origin.matches(row) && matches_search(row, &query)
            })
            .map(|(index, _)| index)
            .collect();

        let rows = &self.snapshot.rows;
        sort_indices(&mut indices, rows, self.sort);
        indices
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

    fn open_git_for_selected(&mut self) {
        let Some(cwd) = self.selected_row().and_then(|row| row.cwd.clone()) else {
            self.set_notice("Este agente no tiene directorio conocido".into(), true);
            return;
        };
        let _ = self.proxy.send_event(UserEvent::OpenGitManager { cwd });
        self.set_notice("Abriendo gestor Git…".into(), false);
    }

    fn open_review_for_selected(&mut self) {
        let Some(cwd) = self.selected_row().and_then(|row| row.cwd.clone()) else {
            self.set_notice("Este agente no tiene directorio conocido".into(), true);
            return;
        };
        let _ = self.proxy.send_event(UserEvent::OpenCodeReview { cwd });
        self.set_notice("Abriendo Code Review…".into(), false);
    }

    fn copy_selected_cwd(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let (value, kind) = if let Some(cwd) = &row.cwd {
            (cwd.display().to_string(), "ruta")
        } else if let Some(pid) = row.pid {
            (pid.to_string(), "PID")
        } else {
            self.set_notice("No hay ruta ni PID para copiar".into(), true);
            return;
        };
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(clipboard) = self.clipboard.as_mut()
            && clipboard.set_text(value.clone()).is_ok()
        {
            self.set_notice(format!("Copiado ({kind}): {value}"), false);
        } else {
            self.set_notice("No se pudo acceder al portapapeles".into(), true);
        }
    }

    fn send_prompt(&mut self) {
        let Some(pane) = self.selected_row().and_then(|row| row.pane) else {
            self.set_notice("Solo se puede enviar texto a agentes internos".into(), true);
            return;
        };
        let text = self.prompt.trim().to_owned();
        if text.is_empty() {
            self.send_quick_bytes(b"\r".to_vec(), "Enter");
            return;
        }
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(b'\r');
        self.prompt.clear();
        self.input_focus = InputFocus::None;
        let _ = self.proxy.send_event(UserEvent::SendAgentInput {
            pane,
            bytes,
            label: text,
        });
        self.window.request_redraw();
    }

    fn focus_prompt(&mut self) {
        if self.selected_row().and_then(|row| row.pane).is_none() {
            self.set_notice("Solo se puede responder a agentes internos".into(), true);
            return;
        }
        self.input_focus = InputFocus::Prompt;
        self.window.request_redraw();
    }

    fn paste_input(&mut self) {
        if self.input_focus == InputFocus::None {
            return;
        }
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let text = self
            .clipboard
            .as_mut()
            .and_then(|clipboard| clipboard.get_text().ok());
        let Some(text) = text else {
            self.set_notice("No se pudo leer el portapapeles".into(), true);
            return;
        };
        let text: String = text
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect();
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return;
        }

        match self.input_focus {
            InputFocus::Search => {
                let room = 256usize.saturating_sub(self.search.chars().count());
                if room > 0 {
                    self.search.extend(text.chars().take(room));
                    self.after_filter_change();
                }
            }
            InputFocus::Prompt => {
                let room = 4096usize.saturating_sub(self.prompt.chars().count());
                if room > 0 {
                    self.prompt.extend(text.chars().take(room));
                }
            }
            InputFocus::None => {}
        }
        self.window.request_redraw();
    }

    fn approve_selected(&mut self) {
        let (internal, blocked) = match self.selected_row() {
            Some(row) => (row.pane.is_some(), row.is_blocked()),
            None => (false, false),
        };
        if !internal {
            self.set_notice("Solo se puede aprobar a agentes internos".into(), true);
            return;
        }
        if !blocked {
            self.set_notice(
                "El agente no está bloqueado; usa I para enviar una respuesta".into(),
                false,
            );
            return;
        }
        self.send_quick_bytes(b"y\r".to_vec(), "Aprobar (y)");
    }

    fn send_quick_bytes(&mut self, bytes: Vec<u8>, label: &str) {
        let Some(pane) = self.selected_row().and_then(|row| row.pane) else {
            self.set_notice("Solo se puede controlar agentes internos".into(), true);
            return;
        };
        let _ = self.proxy.send_event(UserEvent::SendAgentInput {
            pane,
            bytes,
            label: label.to_owned(),
        });
        self.window.request_redraw();
    }

    fn launch_preset(&mut self, kind: AgentKind) {
        let cwd = self.selected_row().and_then(|row| row.cwd.clone());
        self.launcher_open = false;
        let _ = self.proxy.send_event(UserEvent::LaunchAgent { kind, cwd });
        self.window.request_redraw();
    }

    fn mark_selected_seen(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if !row.state.is_finished() || row.seen {
            self.set_notice("Este agente no tiene aviso pendiente".into(), false);
            return;
        }
        let id = row.id.clone();
        let _ = self.proxy.send_event(UserEvent::MarkAgentSeen { id });
    }

    fn mark_all_seen(&mut self) {
        let _ = self.proxy.send_event(UserEvent::MarkAllAgentsSeen);
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
            self.set_notice("Este agente no tiene un proceso que matar".into(), true);
            self.kill_armed = None;
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
        self.set_notice(format!("Deteniendo PID {pid}…"), false);
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
        self.set_notice(text, error);
    }

    pub fn on_action_notice(&mut self, text: String, error: bool) {
        self.set_notice(text, error);
    }

    fn set_notice(&mut self, text: String, error: bool) {
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

    fn console_capacity(&self, config: &Config) -> usize {
        let layout = self.layout(config);
        let scale = self.renderer.scale_factor() as f32;
        let font = self.renderer.physical(config.ui_font_size);
        let tokens = Tokens::new(font, scale);
        preview_capacity(layout.detail_content, tokens)
    }

    fn clamp_selection(&mut self) {
        let rows = self.filtered();
        if rows.is_empty() {
            self.set_selected(None);
            self.scroll = 0;
            return;
        }
        let index = self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id))
            .unwrap_or(0);
        let id = rows[index].id.clone();
        self.set_selected(Some(id));

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
        let gap = ui::px(8.0, scale);
        let header_height = ui::px(ui::HEADER_HEIGHT, scale);
        let footer_height = ui::px(ui::FOOTER_HEIGHT, scale);
        let close_size = ui::px(ui::CLOSE_SIZE, scale);

        let close = Rect::new(
            (width - padding - close_size).max(0.0),
            ((header_height - close_size) / 2.0).max(0.0),
            close_size,
            close_size,
        );
        let refresh_w = ui::px(106.0, scale);
        let refresh = Rect::new(
            (close.x - refresh_w - gap).max(padding),
            close.y,
            refresh_w,
            close_size,
        );
        let mark_w = ui::px(82.0, scale);
        let mark_all = Rect::new(
            (refresh.x - mark_w - gap).max(padding),
            close.y,
            mark_w,
            close_size,
        );
        let launch_w = ui::px(98.0, scale);
        let launch_toggle = Rect::new(
            (mark_all.x - launch_w - gap).max(padding),
            close.y,
            launch_w,
            close_size,
        );

        let mut cursor_y = header_height;
        let (launcher_bar, launcher_presets) = if self.launcher_open {
            let bar_h = ui::px(42.0, scale);
            let bar = Rect::new(0.0, cursor_y, width, bar_h);
            cursor_y += bar_h;
            let btn_h = ui::px(28.0, scale);
            let btn_y = bar.y + (bar_h - btn_h) / 2.0;
            let start_x = padding + ui::px(208.0, scale);
            let btn_w = ((width - start_x - padding - gap * 3.0) / 4.0)
                .clamp(ui::px(88.0, scale), ui::px(150.0, scale));
            let presets = std::array::from_fn(|i| {
                Rect::new(start_x + i as f32 * (btn_w + gap), btn_y, btn_w, btn_h)
            });
            (Some(bar), presets)
        } else {
            (None, [Rect::default(); 4])
        };

        let kpi_h = ui::px(58.0, scale);
        let kpi_bar = Rect::new(0.0, cursor_y, width, kpi_h);
        let card_w = ((width - padding * 2.0 - gap * 3.0) / 4.0).max(1.0);
        let kpi_cards = std::array::from_fn(|i| {
            Rect::new(
                padding + i as f32 * (card_w + gap),
                kpi_bar.y + ui::px(8.0, scale),
                card_w,
                kpi_h - ui::px(16.0, scale),
            )
        });
        cursor_y += kpi_h;

        let body_top = cursor_y;
        let body_bottom = (height - footer_height).max(body_top + 1.0);
        let list_width = (width * 0.46).clamp(ui::px(300.0, scale).min(width), width * 0.56);
        let list = Rect::new(0.0, body_top, list_width, body_bottom - body_top);
        let detail = Rect::new(
            list_width,
            body_top,
            (width - list_width).max(1.0),
            body_bottom - body_top,
        );

        let search_h = ui::px(30.0, scale);
        let search_box = Rect::new(
            padding,
            body_top + ui::px(10.0, scale),
            (list_width - padding * 2.0).max(1.0),
            search_h,
        );
        let clear_size = ui::px(22.0, scale);
        let search_clear = Rect::new(
            (search_box.right() - clear_size - ui::px(4.0, scale)).max(search_box.x),
            search_box.y + (search_h - clear_size) / 2.0,
            clear_size,
            clear_size,
        );

        let chip_h = ui::px(24.0, scale);
        let chip_y = search_box.bottom() + ui::px(6.0, scale);
        let chip_w = ((list_width - padding * 2.0 - gap) / 2.0).max(1.0);
        let origin_chip = Rect::new(padding, chip_y, chip_w, chip_h);
        let sort_chip = Rect::new(padding + chip_w + gap, chip_y, chip_w, chip_h);

        let tab_height = ui::px(28.0, scale);
        let tab_y = origin_chip.bottom() + ui::px(8.0, scale);
        let tab_width = ((list_width - padding * 2.0) / 4.0).max(1.0);
        let tabs = std::array::from_fn(|index| {
            Rect::new(
                padding + index as f32 * tab_width,
                tab_y,
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

        // Inspector sub-tabs
        let insp_tab_h = ui::px(28.0, scale);
        let insp_tab_y = detail.y + ui::px(10.0, scale);
        let insp_tab_w = ((detail.width - padding * 2.0 - gap * 2.0) / 3.0).max(1.0);
        let inspector_tabs = std::array::from_fn(|i| {
            Rect::new(
                detail.x + padding + i as f32 * (insp_tab_w + gap),
                insp_tab_y,
                insp_tab_w,
                insp_tab_h,
            )
        });

        // Bottom actions in inspector (2 compact rows: primary/tools + prompt bar above)
        let action_h = ui::px(ui::BUTTON_HEIGHT, scale);
        let bottom_row_y =
            (body_bottom - action_h - ui::px(10.0, scale)).max(insp_tab_y + insp_tab_h);
        let avail_w = (detail.width - padding * 2.0).max(1.0);
        let kill_w = ui::px(132.0, scale).min(avail_w * 0.36);
        let focus_w = (avail_w - kill_w - gap).max(1.0);
        let focus_action = Rect::new(detail.x + padding, bottom_row_y, focus_w, action_h);
        let kill = Rect::new(
            detail.right() - padding - kill_w,
            bottom_row_y,
            kill_w,
            action_h,
        );

        let tool_h = ui::px(26.0, scale);
        let tool_y = (bottom_row_y - tool_h - ui::px(6.0, scale)).max(insp_tab_y + insp_tab_h);
        let tool_w = ((avail_w - gap * 3.0) / 4.0).max(1.0);
        let git_action = Rect::new(detail.x + padding, tool_y, tool_w, tool_h);
        let review_action = Rect::new(git_action.right() + gap, tool_y, tool_w, tool_h);
        let copy_action = Rect::new(review_action.right() + gap, tool_y, tool_w, tool_h);
        let seen_action = Rect::new(copy_action.right() + gap, tool_y, tool_w, tool_h);

        // Quick reply / prompt input row above tools
        let prompt_h = ui::px(28.0, scale);
        let prompt_y = (tool_y - prompt_h - ui::px(6.0, scale)).max(insp_tab_y + insp_tab_h);
        let send_w = ui::px(74.0, scale);
        let prompt_box = Rect::new(
            detail.x + padding,
            prompt_y,
            (avail_w - send_w - gap).max(1.0),
            prompt_h,
        );
        let prompt_send = Rect::new(prompt_box.right() + gap, prompt_y, send_w, prompt_h);

        // Quick action chips above prompt (Sí, Enter, Ctrl+C)
        let quick_h = ui::px(24.0, scale);
        let quick_y = (prompt_y - quick_h - ui::px(6.0, scale)).max(insp_tab_y + insp_tab_h);
        let quick_w = ((avail_w - gap * 2.0) / 3.0).min(ui::px(110.0, scale));
        let quick_yes = Rect::new(detail.x + padding, quick_y, quick_w, quick_h);
        let quick_enter = Rect::new(quick_yes.right() + gap, quick_y, quick_w, quick_h);
        let quick_ctrl_c = Rect::new(quick_enter.right() + gap, quick_y, quick_w, quick_h);

        let content_top = inspector_tabs[0].bottom() + ui::px(10.0, scale);
        let detail_content = Rect::new(
            detail.x + padding,
            content_top,
            avail_w,
            (quick_y - content_top - ui::px(8.0, scale)).max(1.0),
        );

        Layout {
            header: Rect::new(0.0, 0.0, width, header_height),
            launcher_bar,
            launcher_presets,
            kpi_bar,
            kpi_cards,
            footer: Rect::new(0.0, body_bottom, width, footer_height),
            list,
            detail,
            close,
            refresh,
            mark_all,
            launch_toggle,
            search_box,
            search_clear,
            origin_chip,
            sort_chip,
            tabs,
            list_rows,
            inspector_tabs,
            detail_content,
            prompt_box,
            prompt_send,
            quick_yes,
            quick_enter,
            quick_ctrl_c,
            focus_action,
            git_action,
            review_action,
            copy_action,
            seen_action,
            kill,
            padding,
            row_height: ui::px(76.0, scale),
        }
    }

    fn draw(&mut self, config: &Config) {
        if self.window.is_minimized().unwrap_or(false) {
            return;
        }
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

        // Main background & shell dividers
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

        self.draw_header(&layout, theme, tokens, cursor, &mut quads, &mut labels);
        self.draw_launcher_bar(&layout, theme, tokens, cursor, &mut quads, &mut labels);
        self.draw_kpi_bar(&layout, theme, tokens, &mut quads, &mut labels);

        // Master/Detail pane surfaces
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

        self.draw_list_controls(&layout, theme, tokens, cursor, &mut quads, &mut labels);
        self.draw_filters(&layout, theme, tokens, cursor, &mut quads, &mut labels);
        self.draw_list(&layout, theme, tokens, cursor, &mut quads, &mut labels);
        self.draw_detail(&layout, theme, tokens, cursor, &mut quads, &mut labels);
        self.draw_footer(&layout, width, theme, tokens, &mut labels);

        let scene = Scene {
            panes: Vec::new(),
            labels,
            quads,
        };
        if let Err(error) = self.renderer.render(&scene, theme) {
            log::error!("agent window render failed: {error:#}");
        }
    }

    fn draw_header(
        &self,
        layout: &Layout,
        theme: &Theme,
        tokens: Tokens,
        cursor: (f32, f32),
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        let title_width = (layout.launch_toggle.x - layout.padding * 2.0).max(1.0);
        labels.push(Label {
            text: "Centro de Agentes".into(),
            x: layout.padding,
            y: ui::px(9.0, scale),
            width: title_width,
            font_size: tokens.title,
            color: theme.sidebar_text,
            bold: true,
        });

        let attention = self.snapshot.attention_count();
        let active = self.snapshot.active_count();
        let subtitle = if self.snapshot.rows.is_empty() {
            "Sin agentes detectados · pulsa + Lanzar o abre uno en la terminal".to_owned()
        } else {
            format!(
                "{} activos · {} atención · {} detectados · {} subagentes",
                active,
                attention,
                self.snapshot.rows.len(),
                self.snapshot.total_subagents()
            )
        };
        labels.push(Label {
            text: truncate(&subtitle, title_width, tokens.small),
            x: layout.padding,
            y: ui::px(34.0, scale),
            width: title_width,
            font_size: tokens.small,
            color: if attention > 0 {
                status_color(theme, &AgentState::Attention)
            } else {
                theme.sidebar_text_dim
            },
            bold: false,
        });

        ui::button(
            quads,
            labels,
            ButtonSpec {
                rect: layout.launch_toggle,
                label: if self.launcher_open {
                    "Cerrar +"
                } else {
                    "+ Lanzar"
                },
                kind: if self.launcher_open {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Primary
                },
                enabled: true,
                hovered: layout.launch_toggle.contains(cursor.0, cursor.1),
                focused: false,
            },
            theme,
            tokens,
        );

        let has_unseen = self
            .snapshot
            .rows
            .iter()
            .any(|row| row.state.is_finished() && !row.seen);
        ui::button(
            quads,
            labels,
            ButtonSpec {
                rect: layout.mark_all,
                label: "Vistos",
                kind: ButtonKind::Ghost,
                enabled: has_unseen,
                hovered: layout.mark_all.contains(cursor.0, cursor.1),
                focused: false,
            },
            theme,
            tokens,
        );

        let refreshing = self
            .refreshing_until
            .is_some_and(|until| Instant::now() < until);
        ui::button(
            quads,
            labels,
            ButtonSpec {
                rect: layout.refresh,
                label: if refreshing {
                    "Escaneando…"
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

        ui::icon_button(
            quads,
            labels,
            IconButtonSpec {
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
    }

    fn draw_launcher_bar(
        &self,
        layout: &Layout,
        theme: &Theme,
        tokens: Tokens,
        cursor: (f32, f32),
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let Some(bar) = layout.launcher_bar else {
            return;
        };
        let scale = tokens.scale;
        quads.push(Quad::new(
            bar.x,
            bar.y,
            bar.width,
            bar.height,
            to_linear(ui::mix(theme.sidebar_background, theme.accent, 0.08), 1.0),
        ));
        quads.push(Quad::new(
            bar.x,
            bar.bottom() - 1.0,
            bar.width,
            1.0,
            to_linear(theme.border, 1.0),
        ));
        let target_dir = self
            .selected_row()
            .and_then(|row| row.cwd.as_deref())
            .map(short_path)
            .unwrap_or_else(|| "el directorio actual".into());
        labels.push(Label {
            text: truncate(
                &format!("Lanzar en {target_dir}:"),
                ui::px(200.0, scale),
                tokens.caption,
            ),
            x: layout.padding,
            y: bar.y + (bar.height - tokens.caption * 1.35) / 2.0,
            width: ui::px(200.0, scale),
            font_size: tokens.caption,
            color: theme.sidebar_text,
            bold: true,
        });
        for (index, (preset, rect)) in AgentKind::launch_presets()
            .into_iter()
            .zip(layout.launcher_presets.iter())
            .enumerate()
        {
            let label = format!("{} · {}", index + 1, preset.label());
            ui::button(
                quads,
                labels,
                ButtonSpec {
                    rect: *rect,
                    label: &label,
                    kind: ButtonKind::Secondary,
                    enabled: true,
                    hovered: rect.contains(cursor.0, cursor.1),
                    focused: false,
                },
                theme,
                tokens,
            );
        }
    }

    fn draw_kpi_bar(
        &self,
        layout: &Layout,
        theme: &Theme,
        tokens: Tokens,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        quads.push(Quad::new(
            layout.kpi_bar.x,
            layout.kpi_bar.y,
            layout.kpi_bar.width,
            layout.kpi_bar.height,
            to_linear(theme.background, 1.0),
        ));
        quads.push(Quad::new(
            layout.kpi_bar.x,
            layout.kpi_bar.bottom() - 1.0,
            layout.kpi_bar.width,
            1.0,
            to_linear(theme.border, 1.0),
        ));

        let active = self.snapshot.active_count();
        let blocked = self.snapshot.blocked_count();
        let attention = self.snapshot.attention_count();
        let mem = format_memory(self.snapshot.total_memory_bytes());
        let cpu = format_cpu(self.snapshot.total_cpu_tenths());
        let (add, del) = self.snapshot.total_git_diff();
        let git_value = if add == 0 && del == 0 {
            "Limpio".into()
        } else {
            format!("+{add} -{del}")
        };
        let active_value = if blocked > 0 {
            format!("{active} · {blocked} bloq.")
        } else {
            active.to_string()
        };

        let cards: [(&str, String, Rgb); 4] = [
            (
                "ACTIVOS",
                active_value,
                if active > 0 {
                    theme.ansi[6]
                } else {
                    theme.sidebar_text_dim
                },
            ),
            (
                "ATENCIÓN",
                if attention == 0 {
                    "Todo al día".into()
                } else {
                    format!("{attention} pendiente(s)")
                },
                if attention > 0 {
                    theme.ansi[3]
                } else {
                    theme.ansi[2]
                },
            ),
            ("RECURSOS", format!("{cpu} · {mem}"), theme.sidebar_text),
            (
                "DIFF GIT",
                git_value,
                if add > 0 || del > 0 {
                    theme.accent
                } else {
                    theme.sidebar_text_dim
                },
            ),
        ];

        for ((title, value, value_color), rect) in cards.into_iter().zip(layout.kpi_cards.iter()) {
            quads.push(Quad::rounded(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                6.0 * scale,
                to_linear(theme.sidebar_background, 0.9),
            ));
            quads.push(Quad::rounded(
                rect.x,
                rect.y,
                (3.0 * scale).max(2.0),
                rect.height,
                1.5 * scale,
                to_linear(value_color, 0.85),
            ));
            let tx = rect.x + ui::px(10.0, scale);
            let tw = (rect.width - ui::px(14.0, scale)).max(1.0);
            labels.push(Label {
                text: title.into(),
                x: tx,
                y: rect.y + ui::px(5.0, scale),
                width: tw,
                font_size: tokens.caption * 0.88,
                color: theme.sidebar_text_dim,
                bold: true,
            });
            labels.push(Label {
                text: truncate(&value, tw, tokens.small),
                x: tx,
                y: rect.y + ui::px(20.0, scale),
                width: tw,
                font_size: tokens.small,
                color: value_color,
                bold: true,
            });
        }
    }

    fn draw_list_controls(
        &self,
        layout: &Layout,
        theme: &Theme,
        tokens: Tokens,
        cursor: (f32, f32),
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        let search_focused = self.input_focus == InputFocus::Search;
        quads.push(Quad::rounded(
            layout.search_box.x,
            layout.search_box.y,
            layout.search_box.width,
            layout.search_box.height,
            ui::RADIUS * scale,
            to_linear(theme.background, 1.0),
        ));
        if search_focused {
            ui::focus_ring(quads, layout.search_box, theme, scale);
        }
        let search_text = if self.search.is_empty() {
            if search_focused {
                "Escribe para filtrar…".to_owned()
            } else {
                "/ Buscar por nombre, rama, ruta, PID…".to_owned()
            }
        } else if search_focused {
            format!("{}|", self.search)
        } else {
            self.search.clone()
        };
        let text_w = (layout.search_box.width - ui::px(36.0, scale)).max(1.0);
        labels.push(Label {
            text: truncate(&search_text, text_w, tokens.small),
            x: layout.search_box.x + ui::px(10.0, scale),
            y: layout.search_box.y + (layout.search_box.height - tokens.small * 1.35) / 2.0,
            width: text_w,
            font_size: tokens.small,
            color: if self.search.is_empty() {
                theme.sidebar_text_dim
            } else {
                theme.sidebar_text
            },
            bold: false,
        });
        if !self.search.is_empty() {
            ui::icon_button(
                quads,
                labels,
                IconButtonSpec {
                    rect: layout.search_clear,
                    glyph: "×",
                    enabled: true,
                    hovered: layout.search_clear.contains(cursor.0, cursor.1),
                    focused: false,
                    danger: false,
                },
                theme,
                tokens,
            );
        }

        draw_chip(
            quads,
            labels,
            layout.origin_chip,
            self.origin.label(),
            self.origin != OriginFilter::All,
            layout.origin_chip.contains(cursor.0, cursor.1),
            theme,
            tokens,
        );
        draw_chip(
            quads,
            labels,
            layout.sort_chip,
            self.sort.label(),
            self.sort != SortBy::Priority,
            layout.sort_chip.contains(cursor.0, cursor.1),
            theme,
            tokens,
        );
    }

    fn draw_filters(
        &self,
        layout: &Layout,
        theme: &Theme,
        tokens: Tokens,
        cursor: (f32, f32),
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        let query = self.search.trim().to_lowercase();
        let mut counts = [0usize; 4];
        for row in &self.snapshot.rows {
            if !self.origin.matches(row) || !matches_search(row, &query) {
                continue;
            }
            counts[0] += 1;
            if Filter::Attention.matches(row) {
                counts[1] += 1;
            }
            if Filter::Running.matches(row) {
                counts[2] += 1;
            }
            if Filter::Finished.matches(row) {
                counts[3] += 1;
            }
        }

        for (index, (filter, rect)) in Filter::ALL.iter().zip(layout.tabs.iter()).enumerate() {
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
            let compact = rect.width < ui::px(92.0, scale);
            labels.push(Label {
                text: if compact {
                    filter.label().to_owned()
                } else {
                    format!("{} {}", filter.label(), counts[index])
                },
                x: rect.x + 7.0 * scale,
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
        let rows = self.filtered_indices();
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
                    "Sin agentes activos",
                    "Pulsa '+ Lanzar' (tecla N) para iniciar Claude, Codex, OpenCode o Junie.",
                )
            } else {
                (
                    "Sin coincidencias",
                    "Ajusta la búsqueda, el filtro de estado o el filtro de origen.",
                )
            };
            ui::empty_state(quads, labels, layout.list_rows, title, body, theme, tokens);
            return;
        }

        let visible = (layout.list_rows.height / layout.row_height)
            .floor()
            .max(1.0) as usize;
        self.scroll = self.scroll.min(rows.len().saturating_sub(visible));
        let selected_index = self.selected.as_ref().and_then(|id| {
            rows.iter()
                .position(|index| &self.snapshot.rows[*index].id == id)
        });
        if let Some(index) = selected_index {
            if index < self.scroll {
                self.scroll = index;
            } else if index >= self.scroll + visible {
                self.scroll = index + 1 - visible;
            }
        }

        for (offset, index) in rows.iter().enumerate().skip(self.scroll).take(visible) {
            let row = &self.snapshot.rows[*index];
            let y = layout.list_rows.y + (offset - self.scroll) as f32 * layout.row_height;
            let rect = Rect::new(
                layout.list.x + 8.0 * scale,
                y,
                (layout.list.width - 18.0 * scale).max(1.0),
                layout.row_height - 6.0 * scale,
            );
            let selected = self.selected.as_ref() == Some(&row.id);
            let hovered =
                self.hovered.as_ref() == Some(&row.id) || rect.contains(cursor.0, cursor.1);
            quads.push(Quad::rounded(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                6.0 * scale,
                to_linear(
                    if selected {
                        theme.sidebar_active
                    } else if hovered {
                        theme.sidebar_hover
                    } else {
                        ui::mix(theme.sidebar_background, theme.background, 0.35)
                    },
                    1.0,
                ),
            ));
            let color = status_color(theme, &row.state);
            if selected {
                quads.push(Quad::rounded(
                    rect.x,
                    rect.y,
                    (3.0 * scale).max(2.0),
                    rect.height,
                    1.5 * scale,
                    to_linear(theme.accent, 1.0),
                ));
            } else if row.is_blocked() {
                quads.push(Quad::rounded(
                    rect.x,
                    rect.y,
                    (2.0 * scale).max(2.0),
                    rect.height,
                    1.0 * scale,
                    to_linear(color, 0.9),
                ));
            }

            let dot = (8.0 * scale).max(6.0);
            ui::status_dot(
                quads,
                rect.x + 14.0 * scale,
                rect.y + 18.0 * scale,
                dot,
                color,
                matches!(
                    row.state,
                    AgentState::Working | AgentState::Blocked(_) | AgentState::Attention
                ),
            );
            quads.push(Quad::rounded(
                rect.x + 28.0 * scale,
                rect.y + 10.0 * scale,
                (22.0 * scale).max(18.0),
                (22.0 * scale).max(18.0),
                5.0 * scale,
                to_linear(theme.background, 1.0),
            ));
            labels.push(Label {
                text: row.kind.badge().into(),
                x: rect.x + 34.0 * scale,
                y: rect.y + 12.0 * scale,
                width: 18.0 * scale,
                font_size: tokens.caption,
                color,
                bold: true,
            });

            let text_x = rect.x + 58.0 * scale;
            let meta_width = 112.0 * scale;
            let text_width = (rect.right() - text_x - meta_width - 8.0 * scale).max(1.0);

            // Line 1: Agent Name · State
            let headline = format!("{} · {}", row.display_name(), row.state.label());
            labels.push(Label {
                text: truncate(&headline, text_width, tokens.body),
                x: text_x,
                y: rect.y + 8.0 * scale,
                width: text_width,
                font_size: tokens.small,
                color: theme.sidebar_text,
                bold: true,
            });

            // Line 2: Message / Title / Latest preview line
            let summary = row
                .message
                .as_deref()
                .or(row.title.as_deref())
                .or_else(|| row.preview_lines.last().map(String::as_str))
                .unwrap_or("En ejecución sin mensaje");
            labels.push(Label {
                text: truncate(summary, text_width + meta_width * 0.4, tokens.caption),
                x: text_x,
                y: rect.y + 27.0 * scale,
                width: text_width + meta_width * 0.4,
                font_size: tokens.caption,
                color: if row.is_blocked() {
                    color
                } else {
                    theme.sidebar_text_dim
                },
                bold: false,
            });

            // Line 3: Repo/Dir · Branch · Subagents · Uptime
            let mut footer_parts = Vec::new();
            if let Some(dir) = row
                .cwd
                .as_deref()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().to_string())
            {
                footer_parts.push(dir);
            }
            if let Some(branch) = &row.git_branch {
                let diff_suffix = row
                    .git_diff
                    .filter(|(a, d)| *a > 0 || *d > 0)
                    .map(|(a, d)| format!(" (+{a} -{d})"))
                    .unwrap_or_default();
                footer_parts.push(format!("⎇ {branch}{diff_suffix}"));
            }
            if !row.subagents.is_empty() {
                footer_parts.push(format!("{} sub", row.subagents.len()));
            } else if row.child_processes > 0 {
                footer_parts.push(format!("+{} proc", row.child_processes));
            }
            if row.started > 0 {
                footer_parts.push(duration_since(row.started));
            }
            let meta_line = if footer_parts.is_empty() {
                row.source.label().to_owned()
            } else {
                footer_parts.join(" · ")
            };
            labels.push(Label {
                text: truncate(
                    &meta_line,
                    rect.right() - text_x - 10.0 * scale,
                    tokens.caption,
                ),
                x: text_x,
                y: rect.y + 46.0 * scale,
                width: (rect.right() - text_x - 10.0 * scale).max(1.0),
                font_size: tokens.caption * 0.92,
                color: theme.sidebar_text_dim,
                bold: false,
            });

            // Right column: telemetry or pane badge
            let right_top = match (row.cpu_tenths, row.memory_bytes) {
                (Some(cpu), Some(mem)) if mem > 0 => {
                    format!("{} · {}", format_cpu(u32::from(cpu)), format_memory(mem))
                }
                (_, Some(mem)) if mem > 0 => format_memory(mem),
                _ if row.is_external() => "externo".into(),
                _ => row
                    .pane
                    .map(|pane| format!("pane {}", pane.0))
                    .unwrap_or_default(),
            };
            labels.push(Label {
                text: truncate(&right_top, meta_width - 8.0 * scale, tokens.caption),
                x: rect.right() - meta_width,
                y: rect.y + 8.0 * scale,
                width: meta_width - 8.0 * scale,
                font_size: tokens.caption * 0.92,
                color: theme.sidebar_text_dim,
                bold: false,
            });

            let right_mid = if let Some(progress) = row.progress {
                format!("{progress}%")
            } else if row.is_external() {
                String::new()
            } else {
                row.pane
                    .map(|pane| format!("pane {}", pane.0))
                    .unwrap_or_default()
            };
            labels.push(Label {
                text: right_mid,
                x: rect.right() - meta_width,
                y: rect.y + 26.0 * scale,
                width: meta_width - 8.0 * scale,
                font_size: tokens.caption * 0.92,
                color: if row.progress.is_some() {
                    theme.accent
                } else {
                    theme.sidebar_text_dim
                },
                bold: row.progress.is_some(),
            });

            // Optional micro progress bar along card bottom
            if let Some(progress) = row.progress {
                let bar_y = rect.bottom() - 3.0 * scale;
                let bar_w = rect.width - 12.0 * scale;
                quads.push(Quad::rounded(
                    rect.x + 6.0 * scale,
                    bar_y,
                    bar_w,
                    2.5 * scale,
                    1.25 * scale,
                    to_linear(theme.border, 0.8),
                ));
                quads.push(Quad::rounded(
                    rect.x + 6.0 * scale,
                    bar_y,
                    bar_w * (f32::from(progress.min(100)) / 100.0),
                    2.5 * scale,
                    1.25 * scale,
                    to_linear(theme.ansi[6], 1.0),
                ));
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
                "Inspector de Agente",
                "Selecciona un agente para inspeccionar telemetría, consola en vivo, subagentes y enviar comandos.",
                theme,
                tokens,
            );
            return;
        };

        // Inspector tabs
        for (tab, rect) in InspectorTab::ALL.iter().zip(layout.inspector_tabs.iter()) {
            let active = *tab == self.inspector;
            let hovered = rect.contains(cursor.0, cursor.1);
            quads.push(Quad::rounded(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                5.0 * scale,
                to_linear(
                    if active {
                        theme.sidebar_active
                    } else if hovered {
                        theme.sidebar_hover
                    } else {
                        theme.sidebar_background
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
            let text = if *tab == InspectorTab::Subagents && !row.subagents.is_empty() {
                format!("{} ({})", tab.label(), row.subagents.len())
            } else {
                tab.label().to_owned()
            };
            labels.push(Label {
                text: truncate(&text, rect.width - 12.0 * scale, tokens.caption),
                x: rect.x + ui::px(10.0, scale),
                y: rect.y + (rect.height - tokens.caption * 1.35) / 2.0,
                width: (rect.width - 12.0 * scale).max(1.0),
                font_size: tokens.caption,
                color: if active {
                    theme.sidebar_text
                } else {
                    theme.sidebar_text_dim
                },
                bold: active,
            });
        }

        // Main inspector content area by tab
        match self.inspector {
            InspectorTab::Overview => {
                self.draw_inspector_overview(layout, &row, theme, tokens, quads, labels);
            }
            InspectorTab::Console => {
                self.draw_inspector_console(layout, &row, theme, tokens, quads, labels);
            }
            InspectorTab::Subagents => {
                self.draw_inspector_subagents(layout, &row, theme, tokens, quads, labels);
            }
        }

        // Quick-reply & command controls for internal agents
        let internal = row.pane.is_some();
        if internal {
            ui::button(
                quads,
                labels,
                ButtonSpec {
                    rect: layout.quick_yes,
                    label: "Aprobar (y)",
                    kind: ButtonKind::Secondary,
                    enabled: row.is_blocked(),
                    hovered: layout.quick_yes.contains(cursor.0, cursor.1),
                    focused: false,
                },
                theme,
                tokens,
            );
            ui::button(
                quads,
                labels,
                ButtonSpec {
                    rect: layout.quick_enter,
                    label: "Enviar Enter",
                    kind: ButtonKind::Secondary,
                    enabled: true,
                    hovered: layout.quick_enter.contains(cursor.0, cursor.1),
                    focused: false,
                },
                theme,
                tokens,
            );
            ui::button(
                quads,
                labels,
                ButtonSpec {
                    rect: layout.quick_ctrl_c,
                    label: "Ctrl+C",
                    kind: ButtonKind::Danger,
                    enabled: true,
                    hovered: layout.quick_ctrl_c.contains(cursor.0, cursor.1),
                    focused: false,
                },
                theme,
                tokens,
            );

            let prompt_focused = self.input_focus == InputFocus::Prompt;
            quads.push(Quad::rounded(
                layout.prompt_box.x,
                layout.prompt_box.y,
                layout.prompt_box.width,
                layout.prompt_box.height,
                ui::RADIUS * scale,
                to_linear(theme.sidebar_background, 1.0),
            ));
            if prompt_focused {
                ui::focus_ring(quads, layout.prompt_box, theme, scale);
            }
            let prompt_label = if self.prompt.is_empty() {
                if prompt_focused {
                    "Escribe comando o respuesta y pulsa Enter…".to_owned()
                } else {
                    "I · Enviar mensaje rápido al agente…".to_owned()
                }
            } else if prompt_focused {
                format!("{}|", self.prompt)
            } else {
                self.prompt.clone()
            };
            labels.push(Label {
                text: truncate(
                    &prompt_label,
                    layout.prompt_box.width - ui::px(16.0, scale),
                    tokens.caption,
                ),
                x: layout.prompt_box.x + ui::px(9.0, scale),
                y: layout.prompt_box.y + (layout.prompt_box.height - tokens.caption * 1.35) / 2.0,
                width: (layout.prompt_box.width - ui::px(16.0, scale)).max(1.0),
                font_size: tokens.caption,
                color: if self.prompt.is_empty() {
                    theme.sidebar_text_dim
                } else {
                    theme.sidebar_text
                },
                bold: false,
            });
            ui::button(
                quads,
                labels,
                ButtonSpec {
                    rect: layout.prompt_send,
                    label: "Enviar",
                    kind: ButtonKind::Primary,
                    enabled: true,
                    hovered: layout.prompt_send.contains(cursor.0, cursor.1),
                    focused: false,
                },
                theme,
                tokens,
            );
        }

        // Secondary tools row: Git, Review, Copiar, Visto
        let has_cwd = row.cwd.is_some();
        ui::button(
            quads,
            labels,
            ButtonSpec {
                rect: layout.git_action,
                label: "G · Git",
                kind: ButtonKind::Secondary,
                enabled: has_cwd,
                hovered: layout.git_action.contains(cursor.0, cursor.1),
                focused: false,
            },
            theme,
            tokens,
        );
        ui::button(
            quads,
            labels,
            ButtonSpec {
                rect: layout.review_action,
                label: "V · Review",
                kind: ButtonKind::Secondary,
                enabled: has_cwd,
                hovered: layout.review_action.contains(cursor.0, cursor.1),
                focused: false,
            },
            theme,
            tokens,
        );
        ui::button(
            quads,
            labels,
            ButtonSpec {
                rect: layout.copy_action,
                label: "C · Copiar",
                kind: ButtonKind::Secondary,
                enabled: has_cwd || row.pid.is_some(),
                hovered: layout.copy_action.contains(cursor.0, cursor.1),
                focused: false,
            },
            theme,
            tokens,
        );
        ui::button(
            quads,
            labels,
            ButtonSpec {
                rect: layout.seen_action,
                label: "M · Visto",
                kind: ButtonKind::Secondary,
                enabled: row.state.is_finished() && !row.seen,
                hovered: layout.seen_action.contains(cursor.0, cursor.1),
                focused: false,
            },
            theme,
            tokens,
        );

        // Primary focus & Danger kill row
        if let Some(pane) = row.pane {
            let label = format!("Enter · Enfocar pane {}", pane.0);
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
            ui::button(
                quads,
                labels,
                ButtonSpec {
                    rect: layout.focus_action,
                    label: "Proceso externo (sin pane)",
                    kind: ButtonKind::Secondary,
                    enabled: false,
                    hovered: false,
                    focused: false,
                },
                theme,
                tokens,
            );
        }

        if let Some(pid) = row.pid {
            let armed = self
                .kill_armed
                .as_ref()
                .is_some_and(|(id, at)| id == &row.id && at.elapsed() <= Duration::from_secs(4));
            let label = if armed {
                format!("Confirmar · {pid}")
            } else {
                format!("X · Matar {pid}")
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

    fn draw_inspector_overview(
        &self,
        layout: &Layout,
        row: &AgentRow,
        theme: &Theme,
        tokens: Tokens,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        let area = layout.detail_content;
        let x = area.x;
        let width = area.width;
        let mut y = area.y;

        // Title + State line
        let color = status_color(theme, &row.state);
        labels.push(Label {
            text: row.display_name(),
            x,
            y,
            width: width * 0.62,
            font_size: tokens.section,
            color: theme.sidebar_text,
            bold: true,
        });
        let source_badge = format!("[{}]", row.source.label());
        labels.push(Label {
            text: source_badge,
            x: x + width * 0.65,
            y: y + ui::px(3.0, scale),
            width: width * 0.35,
            font_size: tokens.caption,
            color: theme.sidebar_text_dim,
            bold: false,
        });
        y += tokens.section * 1.45;

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
        let state_text = if let Some(progress) = row.progress {
            format!("{} ({progress}%)", row.state.label())
        } else {
            row.state.label().to_owned()
        };
        labels.push(Label {
            text: state_text,
            x: x + ui::px(14.0, scale),
            y,
            width: (width - ui::px(14.0, scale)).max(1.0),
            font_size: tokens.body,
            color,
            bold: true,
        });
        y += tokens.body * 1.45;

        let message = row
            .message
            .as_deref()
            .or(row.title.as_deref())
            .unwrap_or("Sin mensaje de estado");
        labels.push(Label {
            text: truncate(message, width, tokens.small),
            x,
            y,
            width,
            font_size: tokens.small,
            color: theme.sidebar_text,
            bold: false,
        });
        y += tokens.small * 1.6;

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
                width * (f32::from(progress.min(100)) / 100.0),
                ui::px(5.0, scale),
                ui::px(2.5, scale),
                to_linear(theme.ansi[6], 1.0),
            ));
            y += ui::px(12.0, scale);
        }

        // 4 telemetry tiles
        let tile_gap = ui::px(6.0, scale);
        let tile_w = ((width - tile_gap * 3.0) / 4.0).max(1.0);
        let tile_h = ui::px(42.0, scale);
        if y + tile_h <= area.bottom() {
            let tiles: [(&str, String); 4] = [
                (
                    "CPU",
                    row.cpu_tenths
                        .map(|c| format_cpu(u32::from(c)))
                        .unwrap_or_else(|| "—".into()),
                ),
                (
                    "RAM",
                    row.memory_bytes
                        .filter(|m| *m > 0)
                        .map(format_memory)
                        .unwrap_or_else(|| "—".into()),
                ),
                (
                    "ACTIVIDAD",
                    row.last_output
                        .map(|t| elapsed_label(t.elapsed()))
                        .or_else(|| (row.started > 0).then(|| duration_since(row.started)))
                        .unwrap_or_else(|| "—".into()),
                ),
                (
                    "HIJOS / SUB",
                    format!("{} / {}", row.child_processes, row.subagents.len()),
                ),
            ];
            for (i, (label, val)) in tiles.into_iter().enumerate() {
                let tx = x + i as f32 * (tile_w + tile_gap);
                quads.push(Quad::rounded(
                    tx,
                    y,
                    tile_w,
                    tile_h,
                    5.0 * scale,
                    to_linear(theme.sidebar_background, 0.9),
                ));
                labels.push(Label {
                    text: label.into(),
                    x: tx + ui::px(8.0, scale),
                    y: y + ui::px(5.0, scale),
                    width: (tile_w - ui::px(12.0, scale)).max(1.0),
                    font_size: tokens.caption * 0.84,
                    color: theme.sidebar_text_dim,
                    bold: true,
                });
                labels.push(Label {
                    text: truncate(&val, tile_w - ui::px(12.0, scale), tokens.caption),
                    x: tx + ui::px(8.0, scale),
                    y: y + ui::px(20.0, scale),
                    width: (tile_w - ui::px(12.0, scale)).max(1.0),
                    font_size: tokens.caption,
                    color: theme.sidebar_text,
                    bold: true,
                });
            }
            y += tile_h + ui::px(10.0, scale);
        }

        // Metadata table
        let mut meta = Vec::new();
        if let Some(pid) = row.pid {
            meta.push(("PID", pid.to_string()));
        }
        if let Some(pane) = row.pane {
            let tab = row.tab_title.as_deref().unwrap_or("—");
            meta.push(("Pane / Tab", format!("pane {} · {tab}", pane.0)));
        }
        if let Some(branch) = &row.git_branch {
            let diff = row
                .git_diff
                .filter(|(a, d)| *a > 0 || *d > 0)
                .map(|(a, d)| format!(" (+{a} -{d})"))
                .unwrap_or_default();
            meta.push(("Git", format!("{branch}{diff}")));
        }
        if row.started > 0 {
            meta.push(("Duración", duration_since(row.started)));
        }
        if let Some(cwd) = &row.cwd {
            meta.push(("Directorio", cwd.display().to_string()));
        }
        if let Some(cmd) = &row.command {
            meta.push(("Comando", cmd.clone()));
        }

        let preview_reserve = if !row.preview_lines.is_empty() {
            ui::px(86.0, scale)
        } else {
            0.0
        };
        let meta_limit = (area.bottom() - preview_reserve).max(y);
        for (key, value) in meta {
            if y + tokens.caption * 1.55 > meta_limit {
                break;
            }
            labels.push(Label {
                text: key.into(),
                x,
                y,
                width: (width * 0.26).max(1.0),
                font_size: tokens.caption,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            labels.push(Label {
                text: truncate(&value, width * 0.72, tokens.caption),
                x: x + width * 0.28,
                y,
                width: (width * 0.72).max(1.0),
                font_size: tokens.caption,
                color: theme.sidebar_text,
                bold: false,
            });
            y += tokens.caption * 1.55;
        }

        // Mini live terminal preview at bottom of Overview
        let remaining_h = area.bottom() - y - ui::px(6.0, scale);
        if !row.preview_lines.is_empty() && remaining_h >= ui::px(52.0, scale) {
            y += ui::px(6.0, scale);
            let box_rect = Rect::new(x, y, width, (area.bottom() - y).max(1.0));
            draw_preview_box(
                quads,
                labels,
                box_rect,
                "SALIDA RECIENTE",
                &row.preview_lines,
                0,
                theme,
                tokens,
            );
        }
    }

    fn draw_inspector_console(
        &self,
        layout: &Layout,
        row: &AgentRow,
        theme: &Theme,
        tokens: Tokens,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let area = layout.detail_content;
        if row.pane.is_none() {
            ui::empty_state(
                quads,
                labels,
                area,
                "Consola no disponible",
                "Los procesos externos no pertenecen a un pane de F-Terminal; solo se puede ver telemetría del sistema.",
                theme,
                tokens,
            );
            return;
        }
        if row.preview_lines.is_empty() {
            ui::empty_state(
                quads,
                labels,
                area,
                "Sin salida visible todavía",
                "Cuando el agente escriba en la terminal, las últimas líneas aparecerán aquí en vivo.",
                theme,
                tokens,
            );
            return;
        }
        let header = row
            .last_output
            .map(|at| format!("CONSOLA · últ. salida {}", elapsed_label(at.elapsed())))
            .unwrap_or_else(|| "CONSOLA EN VIVO".into());
        draw_preview_box(
            quads,
            labels,
            area,
            &header,
            &row.preview_lines,
            self.console_scroll,
            theme,
            tokens,
        );
    }

    fn draw_inspector_subagents(
        &self,
        layout: &Layout,
        row: &AgentRow,
        theme: &Theme,
        tokens: Tokens,
        quads: &mut Vec<Quad>,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
        let area = layout.detail_content;
        let mut y = area.y;

        labels.push(Label {
            text: format!(
                "Árbol de procesos: {} descendiente(s) · {} subagente(s) registrados",
                row.child_processes,
                row.subagents.len()
            ),
            x: area.x,
            y,
            width: area.width,
            font_size: tokens.small,
            color: theme.sidebar_text,
            bold: true,
        });
        y += tokens.small * 1.7;

        if let Some(cmd) = &row.command {
            labels.push(Label {
                text: truncate(&format!("CMD: {cmd}"), area.width, tokens.caption),
                x: area.x,
                y,
                width: area.width,
                font_size: tokens.caption,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            y += tokens.caption * 1.7;
        }

        if row.subagents.is_empty() {
            let empty_rect = Rect::new(area.x, y, area.width, (area.bottom() - y).max(1.0));
            let (title, body) = if row.child_processes > 0 {
                (
                    format!("{} procesos hijo activos", row.child_processes),
                    "El agente no publica subagentes por OSC. Puedes inspectar el proceso raíz con su PID y comando; el detalle individual de los hijos no está disponible.".to_owned(),
                )
            } else {
                (
                    "Sin subagentes reportados".to_owned(),
                    "Cuando el agente publique subagentes vía protocolo (OSC), aparecerán aquí con su progreso individual.".to_owned(),
                )
            };
            ui::empty_state(quads, labels, empty_rect, &title, &body, theme, tokens);
            return;
        }

        let card_h = ui::px(48.0, scale);
        let gap = ui::px(6.0, scale);
        for child in &row.subagents {
            if y + card_h > area.bottom() {
                break;
            }
            quads.push(Quad::rounded(
                area.x,
                y,
                area.width,
                card_h,
                6.0 * scale,
                to_linear(theme.sidebar_background, 0.9),
            ));
            let color = status_color(theme, &child.state);
            ui::status_dot(
                quads,
                area.x + ui::px(10.0, scale),
                y + ui::px(14.0, scale),
                (7.0 * scale).max(5.0),
                color,
                matches!(child.state, AgentState::Working | AgentState::Blocked(_)),
            );
            let title = format!("{} · {}", child.kind.label(), child.state.label());
            labels.push(Label {
                text: truncate(&title, area.width - ui::px(90.0, scale), tokens.small),
                x: area.x + ui::px(24.0, scale),
                y: y + ui::px(7.0, scale),
                width: (area.width - ui::px(90.0, scale)).max(1.0),
                font_size: tokens.small,
                color: theme.sidebar_text,
                bold: true,
            });
            let detail = child.message.as_deref().unwrap_or(&child.id);
            labels.push(Label {
                text: truncate(detail, area.width - ui::px(34.0, scale), tokens.caption),
                x: area.x + ui::px(24.0, scale),
                y: y + ui::px(26.0, scale),
                width: (area.width - ui::px(34.0, scale)).max(1.0),
                font_size: tokens.caption,
                color: theme.sidebar_text_dim,
                bold: false,
            });
            if let Some(progress) = child.progress {
                labels.push(Label {
                    text: format!("{progress}%"),
                    x: area.right() - ui::px(52.0, scale),
                    y: y + ui::px(8.0, scale),
                    width: ui::px(44.0, scale),
                    font_size: tokens.caption,
                    color: theme.accent,
                    bold: true,
                });
            }
            y += card_h + gap;
        }
    }

    fn draw_footer(
        &self,
        layout: &Layout,
        width: f32,
        theme: &Theme,
        tokens: Tokens,
        labels: &mut Vec<Label>,
    ) {
        let scale = tokens.scale;
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
                let text = match self.input_focus {
                    InputFocus::Search => {
                        let count = self.filtered().len();
                        format!(
                            "Búsqueda activa · {count} resultado(s) · ↑↓ mover · Enter confirmar · Esc limpiar"
                        )
                    }
                    InputFocus::Prompt => {
                        "Mensaje rápido · Enter enviar al pane · Esc cancelar".into()
                    }
                    InputFocus::None if self.launcher_open => {
                        "Lanzador: 1 Claude · 2 Codex · 3 OpenCode · 4 Junie · Esc cerrar".into()
                    }
                    InputFocus::None => {
                        let approve = self
                            .selected_row()
                            .is_some_and(|row| row.is_blocked() && row.pane.is_some());
                        let prefix = if approve { "Y aprobar · " } else { "" };
                        format!(
                            "{prefix}/ buscar · I responder · N lanzar · S ordenar · E origen · [ ] pestañas · G git · V review · X matar · {updated}"
                        )
                    }
                };
                (text, theme.sidebar_text_dim)
            }
        };
        labels.push(Label {
            text: truncate(&hint, width - layout.padding * 2.0, tokens.caption),
            x: layout.padding,
            y: layout.footer.y + ui::px(6.0, scale),
            width: (width - layout.padding * 2.0).max(1.0),
            font_size: tokens.caption,
            color: hint_color,
            bold: false,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_chip(
    quads: &mut Vec<Quad>,
    labels: &mut Vec<Label>,
    rect: Rect,
    text: &str,
    active: bool,
    hovered: bool,
    theme: &Theme,
    tokens: Tokens,
) {
    let scale = tokens.scale;
    quads.push(Quad::rounded(
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        4.0 * scale,
        to_linear(
            if active {
                ui::mix(theme.sidebar_active, theme.accent, 0.16)
            } else if hovered {
                theme.sidebar_hover
            } else {
                theme.background
            },
            1.0,
        ),
    ));
    labels.push(Label {
        text: truncate(text, rect.width - ui::px(14.0, scale), tokens.caption),
        x: rect.x + ui::px(8.0, scale),
        y: rect.y + (rect.height - tokens.caption * 1.35) / 2.0,
        width: (rect.width - ui::px(14.0, scale)).max(1.0),
        font_size: tokens.caption * 0.94,
        color: if active {
            theme.accent
        } else {
            theme.sidebar_text_dim
        },
        bold: active,
    });
}

fn short_path(path: &Path) -> String {
    let file = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let parent = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    match (parent, file) {
        (Some(parent), Some(file)) => format!("{parent}{}{file}", std::path::MAIN_SEPARATOR),
        (None, Some(file)) => file,
        _ => path.display().to_string(),
    }
}

fn preview_capacity(rect: Rect, tokens: Tokens) -> usize {
    let line_h = tokens.caption * 1.42;
    let start_y = rect.y + ui::px(22.0, tokens.scale);
    (((rect.bottom() - start_y - ui::px(6.0, tokens.scale)) / line_h)
        .floor()
        .max(0.0)) as usize
}

#[allow(clippy::too_many_arguments)]
fn draw_preview_box(
    quads: &mut Vec<Quad>,
    labels: &mut Vec<Label>,
    rect: Rect,
    header: &str,
    lines: &[String],
    scroll: usize,
    theme: &Theme,
    tokens: Tokens,
) {
    let scale = tokens.scale;
    quads.push(Quad::rounded(
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        6.0 * scale,
        to_linear(
            ui::mix(theme.background, Rgb { r: 0, g: 0, b: 0 }, 0.28),
            1.0,
        ),
    ));
    let pad = ui::px(9.0, scale);
    let inner_w = (rect.width - pad * 2.0).max(1.0);

    let line_h = tokens.caption * 1.42;
    let start_y = rect.y + ui::px(22.0, scale);
    let max_lines = (((rect.bottom() - start_y - ui::px(6.0, scale)) / line_h)
        .floor()
        .max(0.0)) as usize;
    if max_lines == 0 {
        return;
    }

    let max_scroll = lines.len().saturating_sub(max_lines);
    let scroll = scroll.min(max_scroll);
    let end = lines.len() - scroll;
    let start = end.saturating_sub(max_lines);
    let header = if lines.len() > max_lines {
        format!("{header} · {}-{} de {}", start + 1, end, lines.len())
    } else {
        header.to_owned()
    };
    labels.push(Label {
        text: truncate(&header, inner_w, tokens.caption * 0.85),
        x: rect.x + pad,
        y: rect.y + ui::px(6.0, scale),
        width: inner_w,
        font_size: tokens.caption * 0.85,
        color: theme.sidebar_text_dim,
        bold: true,
    });

    for (idx, line) in lines[start..end].iter().enumerate() {
        labels.push(Label {
            text: truncate(line, inner_w, tokens.caption * 0.92),
            x: rect.x + pad,
            y: start_y + idx as f32 * line_h,
            width: inner_w,
            font_size: tokens.caption * 0.92,
            color: theme.foreground,
            bold: false,
        });
    }
}

fn matches_search(row: &AgentRow, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let contains = |value: &str| value.to_lowercase().contains(query);
    if contains(&row.display_name()) || contains(row.state.label()) {
        return true;
    }
    if row.message.as_deref().is_some_and(contains)
        || row.title.as_deref().is_some_and(contains)
        || row.tab_title.as_deref().is_some_and(contains)
        || row.git_branch.as_deref().is_some_and(contains)
        || row.command.as_deref().is_some_and(contains)
    {
        return true;
    }
    if let Some(cwd) = &row.cwd
        && contains(&cwd.to_string_lossy())
    {
        return true;
    }
    if let Some(pid) = row.pid
        && pid.to_string().contains(query)
    {
        return true;
    }
    false
}

fn sort_indices(indices: &mut [usize], rows: &[AgentRow], sort: SortBy) {
    match sort {
        SortBy::Priority => indices.sort_by_key(|&i| {
            let row = &rows[i];
            (
                Reverse(row.state.priority(row.seen)),
                row.is_external(),
                row.display_name(),
            )
        }),
        SortBy::Cpu => indices.sort_by_key(|&i| {
            let row = &rows[i];
            (
                Reverse(row.cpu_tenths.unwrap_or(0)),
                Reverse(row.memory_bytes.unwrap_or(0)),
                row.display_name(),
            )
        }),
        SortBy::Memory => indices.sort_by_key(|&i| {
            let row = &rows[i];
            (
                Reverse(row.memory_bytes.unwrap_or(0)),
                Reverse(row.cpu_tenths.unwrap_or(0)),
                row.display_name(),
            )
        }),
        SortBy::Uptime => indices.sort_by_key(|&i| {
            let row = &rows[i];
            let started = if row.started == 0 {
                u64::MAX
            } else {
                row.started
            };
            (started, row.display_name())
        }),
        SortBy::Name => indices.sort_by_key(|&i| {
            let row = &rows[i];
            (row.display_name(), row.pid.unwrap_or(u32::MAX))
        }),
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

fn format_memory(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes == 0 {
        "0 MB".into()
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{} MB", (bytes + MB / 2) / MB)
    } else {
        format!("{} KB", (bytes + KB / 2) / KB)
    }
}

fn format_cpu(tenths: u32) -> String {
    format!("{}.{}%", tenths / 10, tenths % 10)
}

fn truncate(text: &str, max_width: f32, font_size: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentSource;
    use crate::term::program_status::BlockKind;
    use crate::workspace::PaneId;
    use std::path::{Path, PathBuf};

    fn sample_row(
        pid: u32,
        kind: AgentKind,
        state: AgentState,
        cpu: u16,
        mem: u64,
        branch: Option<&str>,
    ) -> AgentRow {
        AgentRow {
            id: AgentId::External { pid },
            kind,
            state,
            message: Some("trabajando en refactor".into()),
            title: None,
            cwd: Some(PathBuf::from("/workspace/f-terminal")),
            pane: Some(PaneId(u64::from(pid))),
            tab_title: Some("dev".into()),
            pane_label: Some("pwsh".into()),
            pid: Some(pid),
            started: 1000 + u64::from(pid),
            progress: Some(45),
            source: AgentSource::Protocol,
            subagents: Vec::new(),
            seen: false,
            memory_bytes: Some(mem),
            cpu_tenths: Some(cpu),
            child_processes: 2,
            command: Some("claude --resume".into()),
            git_branch: branch.map(str::to_owned),
            git_diff: Some((12, 3)),
            last_output: None,
            preview_lines: vec!["running tests...".into()],
        }
    }

    #[test]
    fn search_matches_name_branch_pid_and_command() {
        let row = sample_row(
            4242,
            AgentKind::ClaudeCode,
            AgentState::Working,
            125,
            256 * 1024 * 1024,
            Some("feat/agents-v2"),
        );
        assert!(matches_search(&row, ""));
        assert!(matches_search(&row, "claude"));
        assert!(matches_search(&row, "agents-v2"));
        assert!(matches_search(&row, "4242"));
        assert!(matches_search(&row, "resume"));
        assert!(!matches_search(&row, "codex"));
    }

    #[test]
    fn sort_orders_by_priority_cpu_and_memory() {
        let rows = vec![
            sample_row(1, AgentKind::Codex, AgentState::Working, 50, 100, None),
            sample_row(
                2,
                AgentKind::ClaudeCode,
                AgentState::Blocked(BlockKind::Permission),
                10,
                500,
                None,
            ),
            sample_row(3, AgentKind::OpenCode, AgentState::Idle, 320, 200, None),
        ];

        let mut by_priority = vec![0, 1, 2];
        sort_indices(&mut by_priority, &rows, SortBy::Priority);
        assert_eq!(by_priority[0], 1);

        let mut by_cpu = vec![0, 1, 2];
        sort_indices(&mut by_cpu, &rows, SortBy::Cpu);
        assert_eq!(by_cpu, vec![2, 0, 1]);

        let mut by_mem = vec![0, 1, 2];
        sort_indices(&mut by_mem, &rows, SortBy::Memory);
        assert_eq!(by_mem, vec![1, 2, 0]);
    }

    #[test]
    fn filters_origin_and_search_combine() {
        let mut row = sample_row(
            7,
            AgentKind::ClaudeCode,
            AgentState::Done,
            10,
            1024,
            Some("main"),
        );
        row.seen = false;
        assert!(Filter::Finished.matches(&row));
        assert!(Filter::Attention.matches(&row));
        assert!(!Filter::Running.matches(&row));
        row.seen = true;
        assert!(Filter::Finished.matches(&row));
        assert!(!Filter::Attention.matches(&row));

        row.pane = None;
        assert!(OriginFilter::External.matches(&row));
        assert!(!OriginFilter::Internal.matches(&row));
        assert!(matches_search(&row, &"claude".to_lowercase()));
        assert!(!matches_search(&row, &"zzz".to_lowercase()));
    }

    #[test]
    fn short_path_keeps_the_parent_and_repo() {
        let path = Path::new("/workspace/f-terminal");
        assert_eq!(
            short_path(path),
            format!("workspace{}f-terminal", std::path::MAIN_SEPARATOR)
        );
    }

    #[test]
    fn formats_memory_and_cpu_cleanly() {
        assert_eq!(format_memory(0), "0 MB");
        assert_eq!(format_memory(512 * 1024), "512 KB");
        assert_eq!(format_memory(150 * 1024 * 1024), "150 MB");
        assert_eq!(format_memory(2 * 1024 * 1024 * 1024), "2.0 GB");
        assert_eq!(format_cpu(0), "0.0%");
        assert_eq!(format_cpu(124), "12.4%");
    }
}
