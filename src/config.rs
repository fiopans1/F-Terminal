use crate::term::color::Theme;

/// Static configuration for the first iteration.
///
/// This will grow into a TOML file loaded from the platform config directory,
/// but keeping it as a plain struct already isolates every consumer from that
/// change.
#[derive(Clone, Debug)]
pub struct Config {
    /// Optional font family override. `None` means "pick a good monospace".
    pub font_family: Option<String>,
    /// Font size in logical points.
    pub font_size: f32,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// Padding around the terminal text area, in logical pixels.
    pub padding: f32,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 14.0,
            line_height: 1.3,
            padding: 10.0,
            sidebar_width: 210.0,
            ui_font_size: 12.5,
            shell: None,
            // On macOS Option is used for characters like @ and accents on
            // non-US layouts, so keep composition enabled by default there.
            option_as_meta: cfg!(not(target_os = "macos")),
            theme: Theme::tokyo_night(),
        }
    }
}
