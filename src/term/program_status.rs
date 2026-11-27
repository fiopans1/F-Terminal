//! `OSC 7501` program status support.
//!
//! Coding agents such as Claude Code, Codex and OpenCode can report whether
//! they are working, waiting for the user, done or failed. The protocol is
//! opt-in for the program: before reporting, it asks the terminal whether it
//! understands the sequence. F-Terminal therefore has to both parse reports
//! and answer the feature-detection query.
//!
//! The scanner is deliberately small: it walks bytes that the VT parser is
//! going to see anyway, and only allocates when a complete OSC sequence has
//! been received.

use std::collections::HashMap;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};

const FEATURE_REPLY_BODY: &str =
    "?:states=idle,working,done,blocked,error:kinds=permission,question,auth";
const MAX_OSC_BYTES: usize = 4096;
const MAX_RECORDS: usize = 256;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG_DECODED: usize = 2048;
const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE_DECODED: usize = 192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    Blocked,
    Done,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Permission,
    Question,
    Auth,
}

impl BlockKind {
    fn from_name(value: &str) -> Option<Self> {
        match value {
            "permission" => Some(Self::Permission),
            "question" => Some(Self::Question),
            "auth" => Some(Self::Auth),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramRecord {
    pub id: String,
    pub app: Option<String>,
    pub state: ProgramState,
    pub kind: Option<BlockKind>,
    pub progress: Option<u8>,
    pub title: Option<String>,
    pub message: Option<String>,
    pub touched: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationSource {
    Osc9,
    Osc777,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramNotification {
    pub title: Option<String>,
    pub message: Option<String>,
    pub source: NotificationSource,
}

#[derive(Default)]
pub struct ProgramStatus {
    records: HashMap<String, ProgramRecord>,
    scanner: OscScanner,
    replies: Vec<String>,
    notifications: Vec<ProgramNotification>,
    cwd: Option<PathBuf>,
    revision: u64,
    tick: u64,
}

impl ProgramStatus {
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut frames = Vec::new();
        let mut resets = 0;
        self.scanner.feed(bytes, &mut frames, &mut resets);

        if resets > 0 {
            self.records.clear();
            self.revision = self.revision.wrapping_add(1);
        }

        for (frame, bel) in frames {
            self.handle_frame(&frame, bel);
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn has_records(&self) -> bool {
        !self.records.is_empty()
    }

    pub fn take_replies(&mut self) -> Vec<String> {
        std::mem::take(&mut self.replies)
    }

    pub fn take_notifications(&mut self) -> Vec<ProgramNotification> {
        std::mem::take(&mut self.notifications)
    }

    /// A working directory reported by the shell through `OSC 7` or the
    /// ConEmu/Windows Terminal `OSC 9;9` sequence.
    pub fn take_cwd(&mut self) -> Option<PathBuf> {
        self.cwd.take()
    }

    /// Records in display order (root first). Orphan child records are
    /// included after the roots, with their inherited `app` filled in.
    pub fn snapshot(&self) -> Vec<ProgramRecord> {
        let mut records: Vec<ProgramRecord> = self.records.values().cloned().collect();
        for record in &mut records {
            if record.app.is_none() {
                record.app = self.inherited_app(&record.id);
            }
        }
        records.sort_by(|a, b| {
            a.id.is_empty()
                .cmp(&b.id.is_empty())
                .reverse()
                .then_with(|| b.touched.cmp(&a.touched))
                .then_with(|| a.id.cmp(&b.id))
        });
        records
    }

    /// Drop transient states after the attached process exits or a shell
    /// prompt starts. Finished work stays visible until the user sees it.
    pub fn clear_active(&mut self) {
        let before = self.records.len();
        self.records
            .retain(|_, record| matches!(record.state, ProgramState::Done | ProgramState::Error));
        if self.records.len() != before {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn inherited_app(&self, id: &str) -> Option<String> {
        if let Some(record) = self.records.get(id)
            && record.app.is_some()
        {
            return record.app.clone();
        }

        let mut parent = id;
        while let Some((next, _)) = parent.rsplit_once('/') {
            if let Some(record) = self.records.get(next)
                && let Some(app) = &record.app
            {
                return Some(app.clone());
            }
            if next.is_empty() {
                break;
            }
            parent = next;
        }
        self.records.get("").and_then(|record| record.app.clone())
    }

    fn handle_frame(&mut self, frame: &[u8], bel: bool) {
        let Ok(body) = std::str::from_utf8(frame) else {
            return;
        };

        if let Some(rest) = body.strip_prefix("7501;") {
            self.handle_program_status(rest, bel);
        } else if let Some(rest) = body.strip_prefix("7;") {
            self.cwd = parse_file_uri(rest);
        } else if let Some(rest) = body.strip_prefix("9;9;") {
            self.cwd = parse_native_cwd(rest);
        } else if let Some(rest) = body.strip_prefix("9;") {
            if !rest.starts_with("4;") {
                self.push_notification(ProgramNotification {
                    title: Some("Terminal".into()),
                    message: clean_notification_text(rest),
                    source: NotificationSource::Osc9,
                });
            }
        } else if let Some(rest) = body.strip_prefix("777;") {
            let mut parts = rest.splitn(3, ';');
            let kind = parts.next().unwrap_or("");
            if kind == "notify" {
                let title = parts.next().and_then(clean_notification_text);
                let message = parts.next().and_then(clean_notification_text);
                self.push_notification(ProgramNotification {
                    title,
                    message,
                    source: NotificationSource::Osc777,
                });
            }
        } else if body == "133;A" {
            self.clear_active();
        }
    }

    fn push_notification(&mut self, notification: ProgramNotification) {
        self.notifications.push(notification);
        if self.notifications.len() > 16 {
            self.notifications.remove(0);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    fn handle_program_status(&mut self, body: &str, bel: bool) {
        let body = body.trim();
        if body.is_empty() {
            return;
        }
        if body == "?" || body.starts_with("?:") {
            let terminator = if bel { "\x07" } else { "\x1b\\" };
            self.replies
                .push(format!("\x1b]7501;{FEATURE_REPLY_BODY}{terminator}"));
            return;
        }

        let mut state = None;
        let mut clear = false;
        let mut id = String::new();
        let mut kind = None;
        let mut progress = None;
        let mut app = None;
        let mut title = None;
        let mut message = None;

        for pair in body.split(':') {
            let pair = pair.trim();
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "state" => {
                    clear = false;
                    state = match value {
                        "idle" => Some(ProgramState::Idle),
                        "working" => Some(ProgramState::Working),
                        "blocked" => Some(ProgramState::Blocked),
                        "done" => Some(ProgramState::Done),
                        "error" => Some(ProgramState::Error),
                        "clear" => {
                            clear = true;
                            None
                        }
                        _ => None,
                    };
                }
                "id" => {
                    if !valid_id(value) {
                        return;
                    }
                    id = value.to_owned();
                }
                "kind" => kind = BlockKind::from_name(value),
                "progress" => progress = value.parse::<u8>().ok().filter(|value| *value <= 100),
                "app" => {
                    if valid_app(value) {
                        app = Some(value.to_owned());
                    }
                }
                "title" => match decode_text(value, MAX_TITLE_ENCODED, MAX_TITLE_DECODED) {
                    Ok(text) => title = text,
                    Err(()) => return,
                },
                "msg" => match decode_text(value, MAX_MSG_ENCODED, MAX_MSG_DECODED) {
                    Ok(text) => message = text,
                    Err(()) => return,
                },
                _ => {}
            }
        }

        let Some(state) = state else {
            if clear {
                self.clear_record(&id);
            }
            return;
        };

        if clear {
            self.clear_record(&id);
            return;
        }

        self.tick = self.tick.wrapping_add(1);
        let touched = self.tick;
        if !self.records.contains_key(&id)
            && self.records.len() >= MAX_RECORDS
            && let Some(oldest) = self
                .records
                .iter()
                .min_by_key(|(_, record)| record.touched)
                .map(|(id, _)| id.clone())
        {
            self.records.remove(&oldest);
        }

        let next = ProgramRecord {
            id,
            app,
            state,
            kind: kind.filter(|_| state == ProgramState::Blocked),
            progress: progress
                .filter(|_| matches!(state, ProgramState::Working | ProgramState::Blocked)),
            title,
            message,
            touched,
        };

        // Heartbeats often repeat the same visible state. Refresh the ordering
        // timestamp without invalidating the whole agent snapshot.
        if let Some(existing) = self.records.get_mut(&next.id)
            && existing.app == next.app
            && existing.state == next.state
            && existing.kind == next.kind
            && existing.progress == next.progress
            && existing.title == next.title
            && existing.message == next.message
        {
            existing.touched = touched;
            return;
        }

        self.records.insert(next.id.clone(), next);
        self.revision = self.revision.wrapping_add(1);
    }

    fn clear_record(&mut self, id: &str) {
        if id.is_empty() {
            if !self.records.is_empty() {
                self.records.clear();
                self.revision = self.revision.wrapping_add(1);
            }
            return;
        }

        let prefix = format!("{id}/");
        let before = self.records.len();
        self.records
            .retain(|key, _| key != id && !key.starts_with(&prefix));
        if self.records.len() != before {
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

fn parse_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let (_, path) = rest.split_once('/')?;
    let decoded = percent_decode(&format!("/{path}"))?;
    uri_path_to_native(&decoded)
}

#[cfg(not(windows))]
fn parse_native_cwd(value: &str) -> Option<PathBuf> {
    let path = PathBuf::from(value.trim());
    path.is_absolute().then_some(path)
}

#[cfg(windows)]
fn parse_native_cwd(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let path = PathBuf::from(value);
    if !value.starts_with('/') && path.is_absolute() {
        return Some(path);
    }

    // Git Bash/MSYS2 and Cygwin report POSIX paths even on Windows.
    let unix = value.replace('\\', "/");
    let rest = unix.strip_prefix('/')?;
    let rest = rest.strip_prefix("cygdrive/").unwrap_or(rest);
    let (drive, tail) = rest.split_once('/')?;
    if drive.len() != 1 || !drive.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let candidate = PathBuf::from(format!(
        "{}:\\{}",
        drive.to_ascii_uppercase(),
        tail.replace('/', "\\")
    ));
    candidate.exists().then_some(candidate)
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
        {
            output.push(high << 4 | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(not(windows))]
fn uri_path_to_native(path: &str) -> Option<PathBuf> {
    path.starts_with('/').then(|| PathBuf::from(path))
}

#[cfg(windows)]
fn uri_path_to_native(path: &str) -> Option<PathBuf> {
    let path = path.replace('/', "\\");
    // file://HOST/C:/Users/... arrives as /C:/Users/...
    let rest = path.strip_prefix('\\')?;
    let bytes = rest.as_bytes();
    let drive = bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic();
    drive.then(|| PathBuf::from(rest))
}

fn clean_notification_text(text: &str) -> Option<String> {
    let text: String = text
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn valid_app(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-'))
}

fn valid_id(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let mut depth = 0;
    for segment in value.split('/') {
        depth += 1;
        if segment.is_empty()
            || segment.len() > 32
            || !segment.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
            })
        {
            return false;
        }
    }
    depth <= 8
}

fn decode_text(
    encoded: &str,
    max_encoded: usize,
    max_decoded: usize,
) -> Result<Option<String>, ()> {
    if encoded.is_empty() {
        return Ok(None);
    }
    if encoded.len() > max_encoded {
        return Err(());
    }
    let decoded = STANDARD
        .decode(encoded)
        .or_else(|_| STANDARD_NO_PAD.decode(encoded))
        .map_err(|_| ())?;
    if decoded.len() > max_decoded {
        return Err(());
    }
    let text = String::from_utf8(decoded).map_err(|_| ())?;
    if text
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{202a}'..='\u{202e}'))
    {
        return Err(());
    }
    Ok(Some(text))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ScanState {
    #[default]
    Ground,
    Esc,
    Osc,
    OscEsc,
}

#[derive(Default)]
struct OscScanner {
    state: ScanState,
    buffer: Vec<u8>,
    overflow: bool,
}

impl OscScanner {
    fn feed(&mut self, mut bytes: &[u8], frames: &mut Vec<(Vec<u8>, bool)>, resets: &mut usize) {
        if self.state == ScanState::Ground {
            let Some(start) = bytes.iter().position(|byte| *byte == 0x1b) else {
                return;
            };
            bytes = &bytes[start..];
        }
        for &byte in bytes {
            match self.state {
                ScanState::Ground => {
                    if byte == 0x1b {
                        self.state = ScanState::Esc;
                    }
                }
                ScanState::Esc => match byte {
                    b']' => {
                        self.state = ScanState::Osc;
                        self.buffer.clear();
                        self.overflow = false;
                    }
                    b'c' => {
                        self.state = ScanState::Ground;
                        *resets += 1;
                    }
                    0x1b => {}
                    _ => self.state = ScanState::Ground,
                },
                ScanState::Osc => match byte {
                    0x07 => self.finish(frames, true),
                    0x1b => self.state = ScanState::OscEsc,
                    _ => {
                        if self.buffer.len() < MAX_OSC_BYTES {
                            self.buffer.push(byte);
                        } else {
                            self.overflow = true;
                        }
                    }
                },
                ScanState::OscEsc => match byte {
                    b'\\' => self.finish(frames, false),
                    0x1b => {}
                    _ => {
                        self.state = ScanState::Ground;
                        self.buffer.clear();
                        self.overflow = false;
                    }
                },
            }
        }
    }

    fn finish(&mut self, frames: &mut Vec<(Vec<u8>, bool)>, bel: bool) {
        if !self.overflow {
            frames.push((std::mem::take(&mut self.buffer), bel));
        } else {
            self.buffer.clear();
        }
        self.overflow = false;
        self.state = ScanState::Ground;
    }
}

#[cfg(test)]
#[path = "../tests/term/program_status.rs"]
mod tests;
