//! Agent discovery and the data model shared by the sidebar and the Agent
//! Mode window.
//!
//! Two sources of truth are combined:
//!
//! * `OSC 7501` reports written to a pane's PTY (precise, but only available
//!   in agents that implement the protocol).
//! * A process-tree watcher (works with every agent, including Junie, and can
//!   also list agents running outside F-Terminal).
//!
//! Process scanning happens in a background thread and never blocks the UI.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use sysinfo::{
    Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, Signal, System, UpdateKind,
};
use winit::event_loop::EventLoopProxy;

use crate::event::UserEvent;
use crate::term::program_status::BlockKind;
use crate::workspace::PaneId;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    OpenCode,
    Junie,
    Other(String),
}

impl AgentKind {
    pub fn from_app(app: &str) -> Self {
        match app.trim().to_ascii_lowercase().as_str() {
            "claude-code" | "claude" | "claude_code" => Self::ClaudeCode,
            "codex" | "codex-cli" | "codex_cli" => Self::Codex,
            "opencode" | "open-code" | "open_code" => Self::OpenCode,
            "junie" | "jetbrains-junie" | "jetbrains_junie" => Self::Junie,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::ClaudeCode => "Claude Code".into(),
            Self::Codex => "Codex".into(),
            Self::OpenCode => "OpenCode".into(),
            Self::Junie => "Junie".into(),
            Self::Other(name) => {
                let mut chars = name.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => "Agent".into(),
                }
            }
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "C",
            Self::Codex => "X",
            Self::OpenCode => "O",
            Self::Junie => "J",
            Self::Other(_) => "A",
        }
    }

    pub fn command_name(&self) -> Option<&str> {
        match self {
            Self::ClaudeCode => Some("claude"),
            Self::Codex => Some("codex"),
            Self::OpenCode => Some("opencode"),
            Self::Junie => Some("junie"),
            Self::Other(name) => (!name.is_empty()).then_some(name.as_str()),
        }
    }

    pub fn launch_presets() -> [Self; 4] {
        [Self::ClaudeCode, Self::Codex, Self::OpenCode, Self::Junie]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Blocked(BlockKind),
    Attention,
    Done,
    Error,
    Idle,
    Detected,
}

impl AgentState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Blocked(BlockKind::Permission) => "Needs permission",
            Self::Blocked(BlockKind::Question) => "Needs input",
            Self::Blocked(BlockKind::Auth) => "Needs login",
            Self::Attention => "Needs attention",
            Self::Done => "Finished",
            Self::Error => "Error",
            Self::Idle => "Idle",
            Self::Detected => "Running",
        }
    }

    pub fn priority(&self, seen: bool) -> u8 {
        match self {
            Self::Blocked(_) => 7,
            Self::Attention => 6,
            Self::Error if !seen => 5,
            Self::Done if !seen => 4,
            Self::Working => 3,
            Self::Error | Self::Done => 2,
            Self::Idle => 1,
            Self::Detected => 0,
        }
    }

    pub fn needs_attention(&self) -> bool {
        matches!(
            self,
            Self::Blocked(_) | Self::Attention | Self::Error | Self::Done
        )
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Done | Self::Error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSource {
    Protocol,
    Process,
    Hook,
    External,
}

impl AgentSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Protocol => "OSC 7501",
            Self::Process => "Process",
            Self::Hook => "Junie hook",
            Self::External => "External",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AgentId {
    Terminal { pane: PaneId, record: String },
    External { pid: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentChild {
    pub id: String,
    pub kind: AgentKind,
    pub state: AgentState,
    pub message: Option<String>,
    pub progress: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRow {
    pub id: AgentId,
    pub kind: AgentKind,
    pub state: AgentState,
    pub message: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub pane: Option<PaneId>,
    pub tab_title: Option<String>,
    pub pane_label: Option<String>,
    pub pid: Option<u32>,
    pub started: u64,
    pub progress: Option<u8>,
    pub source: AgentSource,
    pub subagents: Vec<AgentChild>,
    pub seen: bool,
    pub memory_bytes: Option<u64>,
    pub cpu_tenths: Option<u16>,
    pub child_processes: u32,
    pub command: Option<String>,
    pub git_branch: Option<String>,
    pub git_diff: Option<(usize, usize)>,
    pub last_output: Option<Instant>,
    pub preview_lines: Vec<String>,
}

impl AgentRow {
    pub fn is_external(&self) -> bool {
        self.pane.is_none()
    }

    pub fn display_name(&self) -> String {
        self.kind.label()
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentSnapshot {
    pub rows: Vec<AgentRow>,
    pub generated: Option<Instant>,
}

impl PartialEq for AgentSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}

impl AgentSnapshot {
    pub fn attention_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.state.needs_attention() && !row.seen)
            .count()
    }

    pub fn active_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| !row.state.is_finished())
            .count()
    }

    pub fn blocked_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row.state, AgentState::Blocked(_) | AgentState::Attention))
            .count()
    }

    pub fn total_memory_bytes(&self) -> u64 {
        self.rows.iter().filter_map(|row| row.memory_bytes).sum()
    }

    pub fn total_cpu_tenths(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.cpu_tenths.map(u32::from))
            .sum()
    }

    pub fn total_subagents(&self) -> usize {
        self.rows.iter().map(|row| row.subagents.len()).sum()
    }

    pub fn total_git_diff(&self) -> (usize, usize) {
        let mut seen_cwds: HashSet<&Path> = HashSet::new();
        let mut additions = 0usize;
        let mut deletions = 0usize;
        for row in &self.rows {
            let Some((add, del)) = row.git_diff else {
                continue;
            };
            if let Some(cwd) = row.cwd.as_deref()
                && !seen_cwds.insert(cwd)
            {
                continue;
            }
            additions = additions.saturating_add(add);
            deletions = deletions.saturating_add(del);
        }
        (additions, deletions)
    }
}

#[derive(Clone, Debug)]
pub struct DetectedAgent {
    pub pane: Option<PaneId>,
    pub pid: u32,
    pub kind: AgentKind,
    pub cwd: Option<PathBuf>,
    pub started: u64,
    pub memory_bytes: u64,
    pub cpu_tenths: u16,
    pub child_processes: u32,
    pub command: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AgentWatchRequest {
    pub generation: u64,
    pub roots: Vec<(PaneId, u32)>,
    pub include_external: bool,
}

/// Spawn the process-tree watcher. One thread is enough for all panes and it
/// sleeps until the UI asks for another snapshot.
pub fn spawn_watcher(proxy: EventLoopProxy<UserEvent>) -> Option<Sender<AgentWatchRequest>> {
    let (sender, receiver) = mpsc::channel::<AgentWatchRequest>();
    let spawned = thread::Builder::new()
        .name("agent-watcher".into())
        .spawn(move || {
            let Ok(mut system) = System::new() else {
                log::warn!("failed to initialize sysinfo for the agent watcher");
                return;
            };
            while let Ok(mut request) = receiver.recv() {
                // Drop queued requests: only the newest process tree matters.
                while let Ok(newer) = receiver.try_recv() {
                    request = newer;
                }
                let result = scan(&mut system, &request);
                let _ = proxy.send_event(UserEvent::AgentsDetected {
                    generation: request.generation,
                    panes: result.0,
                    external: result.1,
                });
            }
        });

    match spawned {
        Ok(_) => Some(sender),
        Err(error) => {
            log::warn!("failed to start the agent watcher: {error}");
            None
        }
    }
}

/// Ask a detected agent process to exit, escalating to a hard kill when it
/// ignores `SIGTERM`. Runs on a worker thread because both the signal and the
/// wait are blocking.
pub fn terminate_process(pid: u32) -> Result<String, String> {
    let pid = Pid::from_u32(pid);
    let Ok(mut system) = System::new() else {
        return Err("could not inspect the running processes".into());
    };
    let refresh = |system: &mut System| {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
    };

    refresh(&mut system);
    let Some(process) = system.process(pid) else {
        return Err("the process is no longer running".into());
    };
    let sent = process
        .kill_with(Signal::Term)
        .unwrap_or_else(|| process.kill());
    if !sent {
        return Err("the system rejected the termination signal".into());
    }

    for _ in 0..20 {
        thread::sleep(Duration::from_millis(100));
        refresh(&mut system);
        match system.process(pid) {
            None => return Ok("terminated".into()),
            Some(process) if process.status() == ProcessStatus::Zombie => {
                return Ok("terminated".into());
            }
            Some(_) => {}
        }
    }

    refresh(&mut system);
    let Some(process) = system.process(pid) else {
        return Ok("terminated".into());
    };
    if !process.kill() {
        return Err("the process ignored SIGTERM and could not be force-killed".into());
    }
    thread::sleep(Duration::from_millis(300));
    Ok("force-killed".into())
}

fn scan(
    system: &mut System,
    request: &AgentWatchRequest,
) -> (Vec<DetectedAgent>, Vec<DetectedAgent>) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent() {
            children
                .entry(parent.as_u32())
                .or_default()
                .push(pid.as_u32());
        }
    }

    let roots: HashMap<u32, PaneId> = request
        .roots
        .iter()
        .map(|(pane, pid)| (*pid, *pane))
        .collect();

    let mut panes = Vec::new();
    let mut inside: HashSet<u32> = HashSet::new();
    for (root, pane) in &roots {
        let mut stack = vec![*root];
        while let Some(pid) = stack.pop() {
            if !inside.insert(pid) {
                continue;
            }
            let Some(process) = system.process(Pid::from_u32(pid)) else {
                continue;
            };
            if let Some(kind) = detect_process(process, true) {
                panes.push(DetectedAgent {
                    pane: Some(*pane),
                    pid,
                    kind,
                    cwd: None,
                    started: process.start_time(),
                    memory_bytes: 0,
                    cpu_tenths: 0,
                    child_processes: count_descendants(pid, &children),
                    command: format_command(process.cmd()),
                });
                // Descendants of an agent are its subagents or helper
                // processes, not separate top-level agents.
                continue;
            }
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().copied());
            }
        }
    }

    let mut external = Vec::new();
    if request.include_external {
        for (pid, process) in system.processes() {
            let pid = pid.as_u32();
            if inside.contains(&pid) || roots.contains_key(&pid) {
                continue;
            }
            if let Some(kind) = detect_process(process, false) {
                external.push(DetectedAgent {
                    pane: None,
                    pid,
                    kind,
                    cwd: None,
                    started: process.start_time(),
                    memory_bytes: 0,
                    cpu_tenths: 0,
                    child_processes: count_descendants(pid, &children),
                    command: format_command(process.cmd()),
                });
            }
        }
    }

    // `cwd`, memory and CPU usage are only needed for agents that are actually
    // displayed. Refreshing them for every process makes the global scan
    // substantially more expensive on macOS and Windows.
    let detected_pids: Vec<Pid> = panes
        .iter()
        .chain(external.iter())
        .map(|agent| Pid::from_u32(agent.pid))
        .collect();
    if !detected_pids.is_empty() {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&detected_pids),
            false,
            ProcessRefreshKind::nothing()
                .with_cwd(UpdateKind::Always)
                .with_memory()
                .with_cpu(),
        );
        for agent in panes.iter_mut().chain(external.iter_mut()) {
            if let Some(process) = system.process(Pid::from_u32(agent.pid)) {
                agent.cwd = process.cwd().map(Path::to_path_buf);
                agent.memory_bytes = process.memory();
                agent.cpu_tenths = cpu_to_tenths(process.cpu_usage());
            }
        }
    }

    panes.sort_by_key(|agent| (agent.pane.map(|pane| pane.0), agent.pid));
    external.sort_by_key(|agent| agent.pid);
    (panes, external)
}

fn count_descendants(root: u32, children: &HashMap<u32, Vec<u32>>) -> u32 {
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    if let Some(direct) = children.get(&root) {
        stack.extend(direct.iter().copied());
    }
    let mut count = 0u32;
    while let Some(pid) = stack.pop() {
        if pid == root || !visited.insert(pid) {
            continue;
        }
        count = count.saturating_add(1);
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    count
}

fn format_command(cmd: &[OsString]) -> Option<String> {
    let joined = cmd
        .iter()
        .map(|part| part.to_string_lossy())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        return None;
    }
    const MAX_LEN: usize = 240;
    if joined.chars().count() <= MAX_LEN {
        Some(joined)
    } else {
        let mut truncated: String = joined.chars().take(MAX_LEN - 1).collect();
        truncated.push('…');
        Some(truncated)
    }
}

fn cpu_to_tenths(usage: f32) -> u16 {
    if !usage.is_finite() || usage <= 0.0 {
        return 0;
    }
    (usage.clamp(0.0, 999.9) * 10.0).round() as u16
}

fn detect_process(process: &sysinfo::Process, allow_any_name: bool) -> Option<AgentKind> {
    // External scans see every process on the machine. Avoid allocating the
    // full command line unless the process name/executable could plausibly be
    // an agent or one of its common interpreters/wrappers. Processes inside a
    // pane always get the full command-line scan.
    if !allow_any_name {
        let name = process.name().to_string_lossy().to_ascii_lowercase();
        let exe = process
            .exe()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().to_ascii_lowercase());
        let direct = kind_from_binary(&name).is_some()
            || exe
                .as_deref()
                .is_some_and(|exe| kind_from_binary(exe).is_some());
        if !direct && !is_wrapper_name(&name) && !exe.as_deref().is_some_and(is_wrapper_name) {
            return None;
        }
    }

    let cmd: Vec<String> = process
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy().to_string())
        .collect();
    let kind = detect_from_parts(process.name(), process.exe(), &cmd)?;
    // `opencode serve --service` is the shared backend for all sessions, not
    // an interactive agent.
    (!is_service_process(&kind, &cmd)).then_some(kind)
}

fn is_wrapper_name(name: &str) -> bool {
    let name = name.strip_suffix(".exe").unwrap_or(name);
    matches!(
        name,
        "node"
            | "bun"
            | "deno"
            | "java"
            | "python"
            | "python3"
            | "pythonw"
            | "npx"
            | "pnpm"
            | "npm"
            | "yarn"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "cmd"
            | "powershell"
            | "pwsh"
            | "uv"
            | "uvx"
            | "ruby"
            | "perl"
    ) || name.starts_with("python")
        || name.starts_with("node")
}

fn is_service_process(kind: &AgentKind, cmd: &[String]) -> bool {
    matches!(kind, AgentKind::OpenCode) && cmd.iter().any(|part| part == "--service")
}

fn detect_from_parts(name: &OsStr, exe: Option<&Path>, cmd: &[String]) -> Option<AgentKind> {
    let name = name.to_string_lossy().to_ascii_lowercase();
    let exe = exe
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_ascii_lowercase());

    if name == "opencode-pty" || exe.as_deref() == Some("opencode-pty") {
        return None;
    }

    let candidates = [Some(name.as_str()), exe.as_deref()];
    for candidate in candidates.into_iter().flatten() {
        if let Some(kind) = kind_from_binary(candidate) {
            return Some(kind);
        }
    }

    for (index, token) in cmd.iter().enumerate() {
        let token = token.to_ascii_lowercase().replace('\\', "/");
        let basename = token.rsplit('/').next().unwrap_or(&token);
        // The PTY daemon lives under the same install directory as the
        // OpenCode CLI, but it is infrastructure rather than an agent.
        if basename.trim_end_matches(".exe") == "opencode-pty" {
            continue;
        }
        if (index == 0 || token.contains('/'))
            && let Some(kind) = kind_from_binary(basename)
        {
            return Some(kind);
        }
        if index > 0 && token == "opencode-ai" {
            return Some(AgentKind::OpenCode);
        }
        if token.contains("@anthropic-ai/claude-code") || token.contains("/claude-code/") {
            return Some(AgentKind::ClaudeCode);
        }
        if token.contains("@openai/codex") || token.contains("/codex-rs/") {
            return Some(AgentKind::Codex);
        }
        if token.contains("@opencode-ai/")
            || token.contains("/opencode-ai/")
            || token.contains("/.opencode/")
            || token.contains("/opencode/bin/")
        {
            return Some(AgentKind::OpenCode);
        }
        if token.contains("junie.jar")
            || token.contains("/org/jetbrains/junie")
            || token.contains("/junie/bin/")
        {
            return Some(AgentKind::Junie);
        }
    }

    None
}

fn kind_from_binary(name: &str) -> Option<AgentKind> {
    let name = name
        .strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".cmd"))
        .unwrap_or(name);
    match name {
        "claude" | "claude-code" => Some(AgentKind::ClaudeCode),
        "codex" | "codex-cli" => Some(AgentKind::Codex),
        "opencode" | "opencode-ai" => Some(AgentKind::OpenCode),
        "junie" | "junie-cli" => Some(AgentKind::Junie),
        "gemini" | "gemini-cli" => Some(AgentKind::Other("gemini".into())),
        "qwen" | "qwen-code" => Some(AgentKind::Other("qwen".into())),
        "aider" => Some(AgentKind::Other("aider".into())),
        "crush" => Some(AgentKind::Other("crush".into())),
        "goose" => Some(AgentKind::Other("goose".into())),
        "amp" => Some(AgentKind::Other("amp".into())),
        "cursor-agent" => Some(AgentKind::Other("cursor-agent".into())),
        "copilot" => Some(AgentKind::Other("copilot".into())),
        "droid" => Some(AgentKind::Other("droid".into())),
        "kiro" | "kiro-cli" => Some(AgentKind::Other("kiro".into())),
        _ => None,
    }
}

pub fn kind_from_protocol_app(app: &str) -> AgentKind {
    AgentKind::from_app(app)
}

/// Junie does not implement `OSC 7501` yet. Its hooks can append one line per
/// state change to this small log, which the app polls without any extra
/// process.
#[derive(Clone, Debug)]
pub struct HookEvent {
    pub pane: PaneId,
    pub state: AgentState,
    pub message: Option<String>,
}

pub struct HookLog {
    path: PathBuf,
    offset: u64,
}

impl HookLog {
    pub fn new() -> Self {
        let path = hook_log_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let offset = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        Self { path, offset }
    }

    pub fn poll(&mut self) -> Vec<HookEvent> {
        let Ok(metadata) = fs::metadata(&self.path) else {
            self.offset = 0;
            return Vec::new();
        };
        if metadata.len() < self.offset {
            self.offset = 0;
        }
        if metadata.len() > 2 * 1024 * 1024 {
            self.offset = metadata.len();
            return Vec::new();
        }
        if metadata.len() <= self.offset {
            return Vec::new();
        }

        let Ok(mut file) = fs::File::open(&self.path) else {
            self.offset = 0;
            return Vec::new();
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            self.offset = 0;
            return Vec::new();
        }
        let mut chunk = Vec::new();
        if file.read_to_end(&mut chunk).is_err() {
            return Vec::new();
        }
        let Some(last_newline) = chunk.iter().rposition(|byte| *byte == b'\n') else {
            return Vec::new();
        };
        let complete = &chunk[..=last_newline];
        self.offset += complete.len() as u64;
        let Ok(text) = std::str::from_utf8(complete) else {
            return Vec::new();
        };

        let mut events = Vec::new();
        for line in text.lines() {
            let mut parts = line.splitn(3, '|');
            let Some(pane) = parts
                .next()
                .and_then(|pane| pane.trim().parse::<u64>().ok())
            else {
                continue;
            };
            let Some(state) = parts.next().and_then(hook_state) else {
                continue;
            };
            let message = parts
                .next()
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_owned);
            events.push(HookEvent {
                pane: PaneId(pane),
                state,
                message,
            });
        }
        events
    }
}

fn hook_state(name: &str) -> Option<AgentState> {
    match name.trim().to_ascii_lowercase().as_str() {
        "working" | "prompt" => Some(AgentState::Working),
        "permission" | "permission_request" => Some(AgentState::Blocked(BlockKind::Permission)),
        "question" => Some(AgentState::Blocked(BlockKind::Question)),
        "auth" => Some(AgentState::Blocked(BlockKind::Auth)),
        "done" | "stop" => Some(AgentState::Done),
        "error" | "failure" => Some(AgentState::Error),
        "idle" => Some(AgentState::Idle),
        "end" | "clear" => Some(AgentState::Detected),
        _ => None,
    }
}

pub fn hook_log_path() -> PathBuf {
    if let Some(path) = std::env::var_os("F_TERMINAL_HOOK_LOG") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return PathBuf::from(home)
            .join(".f-terminal")
            .join("junie-events.log");
    }
    std::env::temp_dir().join("f-terminal-junie-events.log")
}

#[cfg(test)]
#[path = "tests/agents.rs"]
mod tests;
