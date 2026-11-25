use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::forge::ForgeTokens;
use crate::term::color::Theme;

/// How secondary windows share rendering resources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderMode {
    /// All windows share one GPU device and one text atlas.
    #[default]
    Shared,
    /// Every window owns its own renderer, device and text resources.
    Isolated,
}

/// Configuration loaded from `config.toml`.
#[derive(Clone, Debug)]
pub struct Config {
    /// Optional font family override. `None` means "pick a good monospace".
    pub font_family: Option<String>,
    /// Font size in logical points.
    pub font_size: f32,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// Padding around the terminal text area, in logical pixels. Zero keeps
    /// full-screen TUIs flush with the pane edges.
    pub padding: f32,
    /// Extra inset on the left side of the terminal grid, in logical pixels.
    pub padding_left: f32,
    /// Show the `user@host` prefix of the shell prompt.
    pub show_user_in_prompt: bool,
    /// Sidebar width in logical pixels.
    pub sidebar_width: f32,
    /// Sidebar label size in logical points.
    pub ui_font_size: f32,
    /// Optional shell override (path or name). `None` auto-detects profiles.
    pub shell: Option<String>,
    /// Treat Option/Alt as Meta (prefixed with ESC) instead of composing text.
    pub option_as_meta: bool,
    /// Color palette.
    pub theme: Theme,
    /// Scrollback for panes in the active session.
    pub scrollback_lines: usize,
    /// Scrollback for panes in hidden sessions.
    pub hidden_scrollback_lines: usize,
    /// Optional global scrollback budget. Zero disables it.
    pub scrollback_budget_mb: u64,
    /// Inline completion and popup master switch.
    pub completions: bool,
    pub completion_history: bool,
    pub completion_commands: bool,
    pub completion_paths: bool,
    pub completion_git: bool,
    /// Minimum number of typed characters before a ghost suggestion appears.
    pub completion_min_chars: usize,
    /// Shared or isolated renderer resources.
    pub render_mode: RenderMode,
    /// Restore tabs, splits, working directories and window geometry.
    pub restore_session: bool,
    /// Restore terminal scrollback along with the session layout.
    pub restore_scrollback: bool,
    /// Optional GitHub/GitLab tokens from the config file.
    pub forge_tokens: ForgeTokens,
    /// Agent discovery and Agent Mode settings.
    pub agents: AgentsConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentsConfig {
    /// Enable process-tree discovery and Agent Mode.
    pub enabled: bool,
    /// Include agents running outside F-Terminal (read-only).
    pub show_external: bool,
    /// Process scan interval in milliseconds.
    pub poll_interval_ms: u64,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_external: true,
            poll_interval_ms: 2_000,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 14.0,
            line_height: 1.3,
            padding: 0.0,
            padding_left: 8.0,
            show_user_in_prompt: true,
            sidebar_width: 210.0,
            ui_font_size: 12.5,
            shell: None,
            // On macOS Option is used for characters like @ and accents on
            // non-US layouts, so keep composition enabled by default there.
            option_as_meta: cfg!(not(target_os = "macos")),
            theme: Theme::foundry(),
            scrollback_lines: 6_000,
            hidden_scrollback_lines: 3_000,
            scrollback_budget_mb: 0,
            completions: true,
            completion_history: true,
            completion_commands: true,
            completion_paths: true,
            completion_git: true,
            completion_min_chars: 2,
            render_mode: RenderMode::Shared,
            restore_session: true,
            restore_scrollback: true,
            forge_tokens: ForgeTokens::default(),
            agents: AgentsConfig::default(),
        }
    }
}

impl Config {
    /// Load the unified config file, creating a template on first run.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            log::warn!("could not resolve the config path; using defaults");
            return Self::default();
        };

        match fs::read_to_string(&path) {
            Ok(text) => match Self::from_toml(&text) {
                Ok(config) => {
                    log::info!("loaded config from {}", path.display());
                    config
                }
                Err(error) => {
                    log::warn!(
                        "failed to parse {}; using defaults: {error}",
                        path.display()
                    );
                    Self::default()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match write_default_config(&path) {
                    Ok(()) => log::info!("created default config at {}", path.display()),
                    Err(error) => log::warn!(
                        "failed to create {}; using defaults: {error}",
                        path.display()
                    ),
                }
                Self::default()
            }
            Err(error) => {
                log::warn!("failed to read {}; using defaults: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        let file: FileConfig = toml::from_str(text)?;
        Ok(file.merge())
    }
}

#[derive(Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    appearance: FileAppearance,
    #[serde(default)]
    terminal: FileTerminal,
    #[serde(default)]
    shell: FileShell,
    #[serde(default)]
    render: FileRender,
    #[serde(default)]
    session: FileSession,
    #[serde(default)]
    forge: FileForge,
    #[serde(default)]
    agents: FileAgents,
}

#[derive(Deserialize, Default)]
struct FileAppearance {
    font_family: Option<String>,
    font_size: Option<f32>,
    line_height: Option<f32>,
    padding: Option<f32>,
    padding_left: Option<f32>,
    show_user_in_prompt: Option<bool>,
    sidebar_width: Option<f32>,
    ui_font_size: Option<f32>,
    option_as_meta: Option<bool>,
    theme: Option<String>,
}

#[derive(Deserialize, Default)]
struct FileTerminal {
    scrollback_lines: Option<usize>,
    hidden_scrollback_lines: Option<usize>,
    scrollback_budget_mb: Option<u64>,
    completions: Option<bool>,
    completion_history: Option<bool>,
    completion_commands: Option<bool>,
    completion_paths: Option<bool>,
    completion_git: Option<bool>,
    completion_min_chars: Option<usize>,
}

#[derive(Deserialize, Default)]
struct FileShell {
    default: Option<String>,
}

#[derive(Deserialize, Default)]
struct FileRender {
    mode: Option<String>,
}

#[derive(Deserialize, Default)]
struct FileSession {
    restore: Option<bool>,
    restore_scrollback: Option<bool>,
}

#[derive(Deserialize, Default)]
struct FileForge {
    #[serde(default)]
    github: FileForgeToken,
    #[serde(default)]
    gitlab: FileForgeToken,
}

#[derive(Deserialize, Default)]
struct FileForgeToken {
    token: Option<String>,
}

#[derive(Deserialize, Default)]
struct FileAgents {
    enabled: Option<bool>,
    show_external: Option<bool>,
    poll_interval_ms: Option<u64>,
}

impl FileConfig {
    fn merge(self) -> Config {
        let defaults = Config::default();
        let mut config = defaults.clone();

        if let Some(font_family) = self.appearance.font_family {
            let font_family = font_family.trim();
            config.font_family = (!font_family.is_empty()).then(|| font_family.to_owned());
        }
        config.font_size = number(self.appearance.font_size, defaults.font_size, 6.0, 48.0);
        config.line_height = number(self.appearance.line_height, defaults.line_height, 0.8, 2.5);
        config.padding = number(self.appearance.padding, defaults.padding, 0.0, 64.0);
        config.padding_left = number(
            self.appearance.padding_left,
            defaults.padding_left,
            0.0,
            64.0,
        );
        config.show_user_in_prompt = self
            .appearance
            .show_user_in_prompt
            .unwrap_or(defaults.show_user_in_prompt);
        config.sidebar_width = number(
            self.appearance.sidebar_width,
            defaults.sidebar_width,
            120.0,
            600.0,
        );
        config.ui_font_size = number(
            self.appearance.ui_font_size,
            defaults.ui_font_size,
            6.0,
            32.0,
        );
        if let Some(option_as_meta) = self.appearance.option_as_meta {
            config.option_as_meta = option_as_meta;
        }
        if let Some(theme) = self.appearance.theme {
            let normalized = theme.trim().to_ascii_lowercase();
            if let Some(palette) = Theme::from_name(&normalized) {
                config.theme = palette;
            } else {
                log::warn!("unknown theme {theme:?}; using foundry");
            }
        }

        config.scrollback_lines = self
            .terminal
            .scrollback_lines
            .unwrap_or(defaults.scrollback_lines)
            .clamp(500, 100_000);
        config.hidden_scrollback_lines = self
            .terminal
            .hidden_scrollback_lines
            .unwrap_or(defaults.hidden_scrollback_lines)
            .clamp(100, config.scrollback_lines);
        config.scrollback_budget_mb = self
            .terminal
            .scrollback_budget_mb
            .unwrap_or(defaults.scrollback_budget_mb)
            .min(4096);

        config.completions = self.terminal.completions.unwrap_or(defaults.completions);
        config.completion_history = self
            .terminal
            .completion_history
            .unwrap_or(defaults.completion_history);
        config.completion_commands = self
            .terminal
            .completion_commands
            .unwrap_or(defaults.completion_commands);
        config.completion_paths = self
            .terminal
            .completion_paths
            .unwrap_or(defaults.completion_paths);
        config.completion_git = self
            .terminal
            .completion_git
            .unwrap_or(defaults.completion_git);
        config.completion_min_chars = self
            .terminal
            .completion_min_chars
            .unwrap_or(defaults.completion_min_chars)
            .clamp(1, 8);

        if let Some(shell) = self.shell.default {
            let shell = shell.trim();
            config.shell = (!shell.is_empty()).then(|| shell.to_owned());
        }

        if let Some(mode) = self.render.mode {
            config.render_mode = match mode.trim().to_ascii_lowercase().as_str() {
                "shared" => RenderMode::Shared,
                "isolated" => RenderMode::Isolated,
                other => {
                    log::warn!("unknown render mode {other:?}; using isolated");
                    RenderMode::Isolated
                }
            };
        }

        config.restore_session = self
            .session
            .restore
            .unwrap_or(defaults.restore_session);
        config.restore_scrollback = self
            .session
            .restore_scrollback
            .unwrap_or(defaults.restore_scrollback);

        config.forge_tokens = ForgeTokens {
            github: clean_token(self.forge.github.token),
            gitlab: clean_token(self.forge.gitlab.token),
        };

        config.agents = AgentsConfig {
            enabled: self.agents.enabled.unwrap_or(defaults.agents.enabled),
            show_external: self
                .agents
                .show_external
                .unwrap_or(defaults.agents.show_external),
            poll_interval_ms: self
                .agents
                .poll_interval_ms
                .unwrap_or(defaults.agents.poll_interval_ms)
                .clamp(500, 10_000),
        };

        config
    }
}

fn number(value: Option<f32>, default: f32, min: f32, max: f32) -> f32 {
    value
        .filter(|value| value.is_finite())
        .unwrap_or(default)
        .clamp(min, max)
}

fn clean_token(token: Option<String>) -> Option<String> {
    token.and_then(|token| {
        let token = token.trim();
        (!token.is_empty()).then(|| token.to_owned())
    })
}

/// Persist a runtime font-size change in `config.toml`, keeping the rest of
/// the file (comments and other sections) untouched.
pub fn save_font_size(font_size: f32) -> std::io::Result<()> {
    let Some(path) = config_path() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not resolve the config path",
        ));
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        let _ = write_default_config(&path);
    }
    let text = fs::read_to_string(&path).unwrap_or_else(|_| default_template());
    fs::write(&path, update_font_size(&text, font_size))
}

fn update_font_size(text: &str, font_size: f32) -> String {
    let value = format!("font_size = {font_size:.1}");
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines = Vec::new();
    let mut in_appearance = false;
    let mut saw_appearance = false;
    let mut inserted = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("[appearance]") {
            saw_appearance = true;
            in_appearance = true;
            lines.push(line.to_owned());
            continue;
        }
        if in_appearance && trimmed.starts_with('[') && trimmed.ends_with(']') {
            if !inserted {
                lines.push(value.clone());
                inserted = true;
            }
            in_appearance = false;
        }
        if in_appearance
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "font_size")
        {
            lines.push(value.clone());
            inserted = true;
            continue;
        }
        lines.push(line.to_owned());
    }

    if saw_appearance && in_appearance && !inserted {
        lines.push(value.clone());
    } else if !saw_appearance {
        lines.push(String::new());
        lines.push("[appearance]".to_owned());
        lines.push(value);
    }

    let mut result = lines.join(newline);
    if !result.ends_with(newline) {
        result.push_str(newline);
    }
    result
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("F_TERMINAL_CONFIG") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }

    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("f-terminal").join("config.toml"))
    }

    #[cfg(not(windows))]
    {
        if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(base).join("f-terminal").join("config.toml"));
        }
        if let Some(base) = std::env::var_os("HOME") {
            return Some(
                PathBuf::from(base)
                    .join(".config")
                    .join("f-terminal")
                    .join("config.toml"),
            );
        }
        None
    }
}

fn write_default_config(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(default_template().as_bytes())
}

fn default_template() -> String {
    let option_as_meta = Config::default().option_as_meta;
    format!(
        r#"# F-Terminal configuration
# This file is created automatically on first run.

[appearance]
# font_family = "SF Mono"
font_size = 14.0
line_height = 1.3
# No inset around the terminal grid; keeps TUIs edge-to-edge.
padding = 0.0
# Extra left inset for terminal text (logical pixels).
padding_left = 8.0
# Set to false to hide the user@host prefix from the shell prompt.
show_user_in_prompt = true
sidebar_width = 210.0
ui_font_size = 12.5
option_as_meta = {option_as_meta}
# Temas: foundry, tokyo-night, catppuccin-mocha, dracula, nord, gruvbox-dark,
# one-dark, solarized-dark, github-dark, kanagawa.
theme = "foundry"

[terminal]
# History per pane in the active session.
scrollback_lines = 6000
# History per pane in hidden sessions.
hidden_scrollback_lines = 3000
# Global scrollback budget in MB. 0 disables it.
scrollback_budget_mb = 0
# Inline completion and popup.
completions = true
completion_history = true
completion_commands = true
completion_paths = true
completion_git = true
completion_min_chars = 2

[render]
# "shared" shares GPU/font resources between windows.
# "isolated" gives every window its own renderer.
mode = "shared"

[session]
# Restore tabs, splits, working directories and window geometry on launch.
restore = true
# Replay the previous scrollback in each pane.
restore_scrollback = true

[agents]
# Discover coding agents in panes and answer OSC 7501 status queries.
enabled = true
# Also list agents running outside F-Terminal (read-only).
show_external = true
# Process-tree scan interval in milliseconds (500-10000).
poll_interval_ms = 2000

[shell]
# default = "zsh"

[forge.github]
# token = "github_pat_..."

[forge.gitlab]
# token = "glpat-..."
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_uses_defaults() {
        let config = Config::from_toml("").unwrap();
        assert_eq!(config.padding, 0.0);
        assert_eq!(config.padding_left, 8.0);
        assert!(config.show_user_in_prompt);
        assert_eq!(config.scrollback_lines, 6_000);
        assert_eq!(config.hidden_scrollback_lines, 3_000);
        assert_eq!(config.scrollback_budget_mb, 0);
        assert_eq!(config.render_mode, RenderMode::Shared);
        assert!(config.completions);
        assert_eq!(config.completion_min_chars, 2);
        assert_eq!(config.theme.background, Theme::foundry().background);
        assert!(config.agents.enabled);
        assert!(config.agents.show_external);
        assert_eq!(config.agents.poll_interval_ms, 2_000);
        assert!(config.restore_session);
        assert!(config.restore_scrollback);
    }

    #[test]
    fn parses_scrollback_render_shell_and_tokens() {
        let text = r#"
[appearance]
font_size = 18.0
sidebar_width = 250.0
option_as_meta = true
show_user_in_prompt = false
theme = "dracula"
padding_left = 12.0

[terminal]
scrollback_lines = 10000
hidden_scrollback_lines = 10000
scrollback_budget_mb = 128
completions = false
completion_min_chars = 5

[render]
mode = "isolated"

[session]
restore = false
restore_scrollback = false

[agents]
enabled = false
show_external = false
poll_interval_ms = 100

[shell]
default = "bash"

[forge.github]
token = "ghp_test"

[forge.gitlab]
token = "glpat_test"
"#;
        let config = Config::from_toml(text).unwrap();
        assert_eq!(config.font_size, 18.0);
        assert_eq!(config.sidebar_width, 250.0);
        assert!(config.option_as_meta);
        assert!(!config.show_user_in_prompt);
        assert_eq!(config.theme.background, Theme::dracula().background);
        assert_eq!(config.padding_left, 12.0);
        assert_eq!(config.scrollback_lines, 10_000);
        assert_eq!(config.hidden_scrollback_lines, 10_000);
        assert_eq!(config.scrollback_budget_mb, 128);
        assert!(!config.completions);
        assert_eq!(config.completion_min_chars, 5);
        assert_eq!(config.render_mode, RenderMode::Isolated);
        assert!(!config.restore_session);
        assert!(!config.restore_scrollback);
        assert!(!config.agents.enabled);
        assert!(!config.agents.show_external);
        assert_eq!(config.agents.poll_interval_ms, 500);
        assert_eq!(config.shell.as_deref(), Some("bash"));
        assert_eq!(config.forge_tokens.github.as_deref(), Some("ghp_test"));
        assert_eq!(config.forge_tokens.gitlab.as_deref(), Some("glpat_test"));
    }

    #[test]
    fn clamps_and_repairs_invalid_values() {
        let text = r#"
[appearance]
font_size = 1.0

[terminal]
scrollback_lines = 100
hidden_scrollback_lines = 90000
scrollback_budget_mb = 9999

[render]
mode = "something-else"
"#;
        let config = Config::from_toml(text).unwrap();
        assert_eq!(config.font_size, 6.0);
        assert_eq!(config.scrollback_lines, 500);
        assert_eq!(config.hidden_scrollback_lines, 500);
        assert_eq!(config.scrollback_budget_mb, 4096);
        assert_eq!(config.render_mode, RenderMode::Isolated);
    }

    #[test]
    fn hidden_scrollback_never_drops_below_the_floor() {
        let text = r#"
[terminal]
scrollback_lines = 5000
hidden_scrollback_lines = 0
"#;
        let config = Config::from_toml(text).unwrap();
        assert_eq!(config.scrollback_lines, 5_000);
        assert_eq!(config.hidden_scrollback_lines, 100);
    }

    #[test]
    fn default_template_parses_to_the_defaults() {
        let config = Config::from_toml(&default_template()).unwrap();
        let defaults = Config::default();
        assert_eq!(config.font_size, defaults.font_size);
        assert_eq!(config.line_height, defaults.line_height);
        assert_eq!(config.padding, defaults.padding);
        assert_eq!(config.padding_left, defaults.padding_left);
        assert_eq!(config.show_user_in_prompt, defaults.show_user_in_prompt);
        assert_eq!(config.theme.background, defaults.theme.background);
        assert_eq!(config.sidebar_width, defaults.sidebar_width);
        assert_eq!(config.ui_font_size, defaults.ui_font_size);
        assert_eq!(config.option_as_meta, defaults.option_as_meta);
        assert_eq!(config.scrollback_lines, defaults.scrollback_lines);
        assert_eq!(
            config.hidden_scrollback_lines,
            defaults.hidden_scrollback_lines
        );
        assert_eq!(config.scrollback_budget_mb, defaults.scrollback_budget_mb);
        assert_eq!(config.completions, defaults.completions);
        assert_eq!(config.completion_min_chars, defaults.completion_min_chars);
        assert_eq!(config.render_mode, defaults.render_mode);
        assert_eq!(config.agents, defaults.agents);
        assert!(config.font_family.is_none());
        assert!(config.shell.is_none());
        assert!(config.forge_tokens.github.is_none());
        assert!(config.forge_tokens.gitlab.is_none());
    }

    #[test]
    fn updates_font_size_in_place() {
        let text =
            "[appearance]\nfont_size = 14.0\nui_font_size = 12.5\n\n[terminal]\nfont_size = 99.0\n";
        let updated = update_font_size(text, 16.0);
        assert!(updated.contains("[appearance]\nfont_size = 16.0\nui_font_size = 12.5"));
        assert!(updated.contains("[terminal]\nfont_size = 99.0"));
    }

    #[test]
    fn preserves_windows_line_endings_when_updating_font_size() {
        let updated = update_font_size("[appearance]\r\nfont_size = 14.0\r\n", 16.0);
        assert!(updated.contains("\r\nfont_size = 16.0\r\n"));
    }

    #[test]
    fn inserts_font_size_when_missing() {
        let updated = update_font_size("[appearance]\nui_font_size = 12.5\n", 18.0);
        assert!(updated.contains("[appearance]\nui_font_size = 12.5\nfont_size = 18.0"));
        let updated = update_font_size("[terminal]\nscrollback_lines = 100\n", 18.0);
        assert!(updated.ends_with("[appearance]\nfont_size = 18.0\n"));
    }

    #[test]
    fn writes_a_private_template_once() {
        let dir =
            std::env::temp_dir().join(format!("f-terminal-config-test-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = fs::remove_dir_all(&dir);

        write_default_config(&path).expect("config template should be created");
        let text = fs::read_to_string(&path).expect("config template should be readable");
        assert!(text.contains("[terminal]"));
        assert!(Config::from_toml(&text).is_ok());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "config file should be private");
        }

        // The template is only created once; an existing file is preserved.
        assert!(write_default_config(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
