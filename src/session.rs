//! Session persistence: open tabs, splits, working directories, window
//! geometry and terminal scrollback.
//!
//! Metadata lives in `session.json`; scrollback is stored as one ANSI file per
//! pane so startup only has to parse the small JSON and each pane can load its
//! history when it is actually spawned. Writes happen on a dedicated worker
//! thread: the UI only snapshots cheap metadata and, for full saves, clones the
//! alacritty grids.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use serde::{Deserialize, Serialize};

use crate::config::config_path;
use crate::term::GridSize;
use crate::term::color::Theme;

pub const SESSION_VERSION: u32 = 1;
const MAX_SESSION_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_SCROLLBACK_BYTES: u64 = 16 * 1024 * 1024;

/// Everything needed to recreate the main window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    #[serde(default = "current_version")]
    pub version: u32,
    pub window: Option<SavedWindow>,
    pub active_tab: usize,
    pub tabs: Vec<SavedTab>,
    pub collapsed_tabs: Vec<usize>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            window: None,
            active_tab: 0,
            tabs: Vec::new(),
            collapsed_tabs: Vec::new(),
        }
    }
}

fn current_version() -> u32 {
    SESSION_VERSION
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedWindow {
    pub width: f32,
    pub height: f32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTab {
    pub title: Option<String>,
    pub focused: usize,
    pub layout: SavedLayout,
    pub panes: Vec<SavedPane>,
}

impl Default for SavedTab {
    fn default() -> Self {
        Self {
            title: None,
            focused: 0,
            layout: SavedLayout::Leaf { index: 0 },
            panes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPane {
    pub key: u64,
    pub profile_id: String,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SavedLayout {
    Leaf {
        index: usize,
    },
    Split {
        direction: SavedDirection,
        ratio: f32,
        first: Box<SavedLayout>,
        second: Box<SavedLayout>,
    },
}

impl Default for SavedLayout {
    fn default() -> Self {
        Self::Leaf { index: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedDirection {
    Right,
    Down,
}

/// A terminal grid cloned on the UI thread and converted to ANSI by the writer.
pub struct ScrollbackSource {
    pub key: u64,
    pub grid: Grid<Cell>,
    pub size: GridSize,
    pub theme: Theme,
}

pub struct SaveJob {
    pub state: SessionState,
    /// Every pane key referenced by `state`. Files outside this set are stale.
    pub keys: Vec<u64>,
    pub scrollbacks: Vec<ScrollbackSource>,
}

struct Queue {
    job: Option<SaveJob>,
    stop: bool,
}

/// Background writer. Only the latest queued job matters, so a slow disk never
/// builds an unbounded backlog while the terminal is producing output.
pub struct SessionWriter {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl SessionWriter {
    pub fn new() -> Option<Self> {
        let root = session_root()?;
        if let Err(error) = fs::create_dir_all(&root) {
            log::warn!(
                "could not create session directory {}: {error}",
                root.display()
            );
            return None;
        }

        let queue = Arc::new((
            Mutex::new(Queue {
                job: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let thread_queue = queue.clone();
        let handle = std::thread::Builder::new()
            .name("session-writer".into())
            .spawn(move || writer_loop(root, thread_queue))
            .map_err(|error| log::warn!("could not start the session writer: {error}"))
            .ok()?;

        Some(Self {
            queue,
            handle: Some(handle),
        })
    }

    pub fn request(&self, job: SaveJob) {
        let (lock, condvar) = &*self.queue;
        if let Ok(mut queue) = lock.lock() {
            if let Some(pending) = queue.job.as_mut() {
                // Keep every pane that changed between requests. Replacing the
                // whole job would drop scrollback for panes that were queued
                // while the writer was busy.
                pending.state = job.state;
                pending.keys = job.keys;
                for source in job.scrollbacks {
                    if let Some(existing) = pending
                        .scrollbacks
                        .iter_mut()
                        .find(|existing| existing.key == source.key)
                    {
                        *existing = source;
                    } else {
                        pending.scrollbacks.push(source);
                    }
                }
            } else {
                queue.job = Some(job);
            }
            condvar.notify_one();
        }
    }
}

impl Drop for SessionWriter {
    fn drop(&mut self) {
        let (lock, condvar) = &*self.queue;
        if let Ok(mut queue) = lock.lock() {
            queue.stop = true;
            condvar.notify_one();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn writer_loop(root: PathBuf, queue: Arc<(Mutex<Queue>, Condvar)>) {
    loop {
        let job = {
            let (lock, condvar) = &*queue;
            let mut guard = match lock.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            loop {
                if let Some(job) = guard.job.take() {
                    break Some(job);
                }
                if guard.stop {
                    break None;
                }
                guard = match condvar.wait(guard) {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
            }
        };

        let Some(job) = job else {
            break;
        };
        if let Err(error) = write_job(&root, job) {
            log::warn!("session save failed: {error:#}");
        }
    }
}

fn write_job(root: &Path, job: SaveJob) -> anyhow::Result<()> {
    let scrollback_dir = root.join("session-scrollback");
    fs::create_dir_all(&scrollback_dir)?;

    for source in job.scrollbacks {
        let Some(ansi) = ansi_from_grid(&source.grid, source.size, source.theme) else {
            continue;
        };
        if ansi.is_empty() {
            continue;
        }
        let path = scrollback_dir.join(format!("{:016x}.ansi", source.key));
        write_atomic(&path, ansi.as_bytes())?;
    }

    let metadata = serde_json::to_vec_pretty(&job.state)?;
    write_atomic(&root.join("session.json"), &metadata)?;

    let keep: HashSet<u64> = job.keys.into_iter().collect();
    if let Ok(entries) = fs::read_dir(&scrollback_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("ansi") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(key) = u64::from_str_radix(stem, 16) else {
                continue;
            };
            if !keep.contains(&key) {
                let _ = fs::remove_file(&path);
            }
        }
    }

    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        let _ = file.sync_all();
    }
    fs::rename(&temp, path)
}

pub fn session_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("F_TERMINAL_SESSION_DIR") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    let config = config_path()?;
    config.parent().map(Path::to_path_buf)
}

pub fn session_path() -> Option<PathBuf> {
    session_root().map(|root| root.join("session.json"))
}

pub fn scrollback_path(key: u64) -> Option<PathBuf> {
    session_root().map(|root| {
        root.join("session-scrollback")
            .join(format!("{key:016x}.ansi"))
    })
}

pub fn load() -> Option<SessionState> {
    let path = session_path()?;
    let metadata = fs::metadata(&path).ok()?;
    if metadata.len() > MAX_SESSION_BYTES {
        log::warn!(
            "ignoring oversized session file {} ({} bytes)",
            path.display(),
            metadata.len()
        );
        return None;
    }

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("could not read {}: {error}", path.display());
            return None;
        }
    };
    match serde_json::from_str::<SessionState>(&text) {
        Ok(state) if state.version == SESSION_VERSION => Some(state),
        Ok(state) => {
            log::warn!(
                "ignoring session file version {} (expected {})",
                state.version,
                SESSION_VERSION
            );
            None
        }
        Err(error) => {
            log::warn!("could not parse {}: {error}", path.display());
            None
        }
    }
}

/// Read a pane's saved scrollback. Called only when that pane is spawned, so
/// inactive tabs do not pay the I/O cost at startup.
pub fn read_scrollback(path: &Path) -> Option<Vec<u8>> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > MAX_SCROLLBACK_BYTES {
        log::warn!(
            "ignoring oversized scrollback file {} ({} bytes)",
            path.display(),
            metadata.len()
        );
        return None;
    }
    match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            log::warn!("could not read {}: {error}", path.display());
            None
        }
    }
}

/// Turn a cloned terminal grid into the ANSI stream replayed before the shell
/// starts. Only text, colors and line breaks are emitted; OSC notifications,
/// titles and clipboard requests never reach the new terminal.
pub fn ansi_from_grid(grid: &Grid<Cell>, size: GridSize, theme: Theme) -> Option<String> {
    let columns = size.cols.min(grid.columns());
    if columns == 0 {
        return None;
    }

    let top = grid.topmost_line().0;
    let bottom = grid.bottommost_line().0;
    let mut output = String::new();

    for line in top..=bottom {
        let row = &grid[Line(line)];
        let mut last = 0;
        for column in 0..columns {
            if !is_default_blank(&row[Column(column)]) {
                last = column + 1;
            }
        }
        if last == 0 {
            output.push_str("\x1b[0m\r\n");
            continue;
        }

        let mut current_fg = None;
        let mut current_bg = None;
        let mut current_flags = Flags::empty();
        for column in 0..last {
            let cell = &row[Column(column)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                output.push(' ');
                continue;
            }

            let (fg, bg) = if cell.flags.contains(Flags::INVERSE) {
                (cell.bg, cell.fg)
            } else {
                (cell.fg, cell.bg)
            };
            if current_fg != Some(fg) || current_bg != Some(bg) || current_flags != cell.flags {
                write_sgr(&mut output, fg, bg, cell.flags, theme);
                current_fg = Some(fg);
                current_bg = Some(bg);
                current_flags = cell.flags;
            }

            if cell.flags.contains(Flags::HIDDEN) {
                output.push(' ');
            } else {
                output.push(cell.c);
                if let Some(zerowidth) = cell.zerowidth() {
                    for character in zerowidth {
                        output.push(*character);
                    }
                }
            }
        }
        output.push_str("\x1b[0m\r\n");
    }

    Some(output)
}

fn is_default_blank(cell: &Cell) -> bool {
    cell.c == ' '
        && cell.zerowidth().is_none()
        && cell.flags.is_empty()
        && cell.fg == Color::Named(NamedColor::Foreground)
        && cell.bg == Color::Named(NamedColor::Background)
}

fn write_sgr(output: &mut String, fg: Color, bg: Color, flags: Flags, theme: Theme) {
    output.push_str("\x1b[0m");
    if flags.contains(Flags::BOLD) {
        output.push_str("\x1b[1m");
    }
    if flags.contains(Flags::DIM) {
        output.push_str("\x1b[2m");
    }
    if flags.contains(Flags::ITALIC) {
        output.push_str("\x1b[3m");
    }
    if flags.intersects(Flags::ALL_UNDERLINES) {
        output.push_str("\x1b[4m");
    }
    if flags.contains(Flags::STRIKEOUT) {
        output.push_str("\x1b[9m");
    }
    if fg != Color::Named(NamedColor::Foreground) {
        push_color(output, fg, false, theme);
    }
    if bg != Color::Named(NamedColor::Background) {
        push_color(output, bg, true, theme);
    }
}

fn push_color(output: &mut String, color: Color, background: bool, theme: Theme) {
    let layer = if background { 48 } else { 38 };
    match color {
        Color::Spec(rgb) => {
            let _ = write!(output, "\x1b[{layer};2;{};{};{}m", rgb.r, rgb.g, rgb.b);
        }
        Color::Indexed(index) => {
            if let Some(rgb) = theme.ansi.get(index as usize) {
                let _ = write!(output, "\x1b[{layer};2;{};{};{}m", rgb.r, rgb.g, rgb.b);
            } else {
                let _ = write!(output, "\x1b[{layer};5;{index}m");
            }
        }
        Color::Named(named) => {
            if let Some(rgb) = named_rgb(named, theme) {
                let _ = write!(output, "\x1b[{layer};2;{};{};{}m", rgb.r, rgb.g, rgb.b);
            } else if background {
                output.push_str("\x1b[49m");
            } else {
                output.push_str("\x1b[39m");
            }
        }
    }
}

fn named_rgb(named: NamedColor, theme: Theme) -> Option<alacritty_terminal::vte::ansi::Rgb> {
    let index = match named {
        NamedColor::Black => 0,
        NamedColor::Red => 1,
        NamedColor::Green => 2,
        NamedColor::Yellow => 3,
        NamedColor::Blue => 4,
        NamedColor::Magenta => 5,
        NamedColor::Cyan => 6,
        NamedColor::White => 7,
        NamedColor::BrightBlack => 8,
        NamedColor::BrightRed => 9,
        NamedColor::BrightGreen => 10,
        NamedColor::BrightYellow => 11,
        NamedColor::BrightBlue => 12,
        NamedColor::BrightMagenta => 13,
        NamedColor::BrightCyan => 14,
        NamedColor::BrightWhite => 15,
        NamedColor::Foreground => return Some(theme.foreground),
        NamedColor::Background => return Some(theme.background),
        NamedColor::Cursor => return Some(theme.cursor),
        _ => return None,
    };
    Some(theme.ansi[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::{Config as TermConfig, Term};

    fn grid_for(text: &str) -> (Grid<Cell>, GridSize) {
        let size = GridSize { cols: 40, lines: 4 };
        let mut term = Term::new(TermConfig::default(), &size, VoidListener);
        let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
        parser.advance(&mut term, text.as_bytes());
        (term.grid().clone(), size)
    }

    #[test]
    fn writes_session_metadata_and_scrollback() {
        let dir =
            std::env::temp_dir().join(format!("f-terminal-session-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let (grid, size) = grid_for("hello\r\n");
        let state = SessionState {
            version: SESSION_VERSION,
            window: None,
            active_tab: 0,
            tabs: vec![SavedTab {
                title: Some("saved".into()),
                focused: 0,
                layout: SavedLayout::Leaf { index: 0 },
                panes: vec![SavedPane {
                    key: 7,
                    profile_id: "windows:pwsh".into(),
                    cwd: Some(r"C:\github\api".into()),
                }],
            }],
            collapsed_tabs: Vec::new(),
        };
        let job = SaveJob {
            state: state.clone(),
            keys: vec![7],
            scrollbacks: vec![ScrollbackSource {
                key: 7,
                grid,
                size,
                theme: Theme::foundry(),
            }],
        };

        write_job(&dir, job).unwrap();
        let text = fs::read_to_string(dir.join("session.json")).unwrap();
        let parsed: SessionState = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, state);

        let scrollback = fs::read(dir.join("session-scrollback/0000000000000007.ansi")).unwrap();
        assert!(String::from_utf8_lossy(&scrollback).contains("hello"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn serializes_and_deserializes_a_split_layout() {
        let state = SessionState {
            version: SESSION_VERSION,
            window: Some(SavedWindow {
                width: 1200.0,
                height: 800.0,
                x: Some(20),
                y: Some(30),
                maximized: true,
            }),
            active_tab: 1,
            tabs: vec![SavedTab {
                title: Some("api".into()),
                focused: 1,
                layout: SavedLayout::Split {
                    direction: SavedDirection::Right,
                    ratio: 0.4,
                    first: Box::new(SavedLayout::Leaf { index: 0 }),
                    second: Box::new(SavedLayout::Leaf { index: 1 }),
                },
                panes: vec![
                    SavedPane {
                        key: 1,
                        profile_id: "windows:pwsh".into(),
                        cwd: Some(r"C:\github\api".into()),
                    },
                    SavedPane {
                        key: 2,
                        profile_id: "windows:wsl:Ubuntu".into(),
                        cwd: Some(r"\\wsl$\Ubuntu\home\me".into()),
                    },
                ],
            }],
            collapsed_tabs: vec![0],
        };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }

    #[test]
    fn scrollback_replays_text_and_colors() {
        let (grid, size) = grid_for("hello \x1b[31mred\x1b[0m\r\n");
        let ansi = ansi_from_grid(&grid, size, Theme::foundry()).unwrap();
        assert!(ansi.contains("hello"));
        assert!(ansi.contains("red"));
        assert!(ansi.contains("38;2;"));
        assert!(ansi.ends_with("\r\n"));
    }

    #[test]
    fn scrollback_keeps_text_attributes() {
        let (grid, size) = grid_for("\x1b[1;3;4mstyled\x1b[0m\r\n");
        let ansi = ansi_from_grid(&grid, size, Theme::foundry()).unwrap();
        assert!(ansi.contains("\x1b[1m"));
        assert!(ansi.contains("\x1b[3m"));
        assert!(ansi.contains("\x1b[4m"));
    }

    #[test]
    fn trailing_blank_cells_are_trimmed() {
        let (grid, size) = grid_for("ok");
        let ansi = ansi_from_grid(&grid, size, Theme::foundry()).unwrap();
        assert!(ansi.contains("ok"));
        assert!(!ansi.contains("ok                          "));
    }
}
