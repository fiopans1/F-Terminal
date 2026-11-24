use alacritty_terminal::term::color::Colors;
pub use alacritty_terminal::vte::ansi::Rgb;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

/// Terminal color palette plus the special UI colors.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    /// ANSI colors 0..16 (normal + bright).
    pub ansi: [Rgb; 16],
    // Chrome (sidebar, borders, focus rings).
    pub sidebar_background: Rgb,
    pub sidebar_hover: Rgb,
    pub sidebar_active: Rgb,
    pub sidebar_text: Rgb,
    pub sidebar_text_dim: Rgb,
    pub accent: Rgb,
    pub border: Rgb,
    // Diff / code review colors.
    pub diff_add_background: Rgb,
    pub diff_remove_background: Rgb,
    pub diff_add_text: Rgb,
    pub diff_remove_text: Rgb,
    pub diff_hunk_text: Rgb,
    pub diff_meta_text: Rgb,
}

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

impl Theme {
    /// Resolve a theme preset by its configuration name.
    pub fn from_name(name: &str) -> Option<Self> {
        let normalized = name.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        match normalized.as_str() {
            "foundry" | "foundry-dark" | "fundicion" | "fundición" => Some(Self::foundry()),
            "tokyo-night" | "tokyo" => Some(Self::tokyo_night()),
            "catppuccin-mocha" | "catppuccin" => Some(Self::catppuccin_mocha()),
            "dracula" => Some(Self::dracula()),
            "nord" => Some(Self::nord()),
            "gruvbox-dark" | "gruvbox" => Some(Self::gruvbox_dark()),
            "one-dark" | "onedark" => Some(Self::one_dark()),
            "solarized-dark" | "solarized" => Some(Self::solarized_dark()),
            "github-dark" | "github" => Some(Self::github_dark()),
            "kanagawa" => Some(Self::kanagawa()),
            _ => None,
        }
    }

    /// F-Terminal's own palette: dark foundry iron with a warm brass accent.
    /// The accent is deliberately warm against the cool graphite surfaces so
    /// the app does not read as another blue-on-black IDE theme.
    pub const fn foundry() -> Self {
        Self {
            background: rgb(0x10, 0x17, 0x19),
            foreground: rgb(0xd6, 0xe2, 0xe2),
            cursor: rgb(0xe5, 0xb5, 0x67),
            selection: rgb(0x28, 0x48, 0x4b),
            ansi: [
                rgb(0x1b, 0x26, 0x29),
                rgb(0xd9, 0x7b, 0x73),
                rgb(0x8f, 0xbf, 0x9f),
                rgb(0xd8, 0xb3, 0x6a),
                rgb(0x6f, 0xa8, 0xb8),
                rgb(0xb5, 0x8a, 0xa5),
                rgb(0x7f, 0xb5, 0xa8),
                rgb(0xc9, 0xd6, 0xd6),
                rgb(0x4b, 0x5d, 0x60),
                rgb(0xe5, 0x8c, 0x82),
                rgb(0xa6, 0xd1, 0xaf),
                rgb(0xe8, 0xc7, 0x7e),
                rgb(0x85, 0xbc, 0xcb),
                rgb(0xc7, 0x9a, 0xb7),
                rgb(0x96, 0xc9, 0xbc),
                rgb(0xf0, 0xf6, 0xf6),
            ],
            sidebar_background: rgb(0x0c, 0x12, 0x14),
            sidebar_hover: rgb(0x17, 0x21, 0x24),
            sidebar_active: rgb(0x22, 0x31, 0x34),
            sidebar_text: rgb(0xd6, 0xe2, 0xe2),
            sidebar_text_dim: rgb(0x8f, 0xa3, 0xa4),
            accent: rgb(0xdf, 0xa8, 0x61),
            border: rgb(0x26, 0x36, 0x3a),
            diff_add_background: rgb(0x14, 0x25, 0x1f),
            diff_remove_background: rgb(0x2b, 0x18, 0x18),
            diff_add_text: rgb(0x8f, 0xbf, 0x9f),
            diff_remove_text: rgb(0xd9, 0x7b, 0x73),
            diff_hunk_text: rgb(0x6f, 0xa8, 0xb8),
            diff_meta_text: rgb(0x8f, 0xa3, 0xa4),
        }
    }

    /// Tokyo Night inspired dark palette.
    pub const fn tokyo_night() -> Self {
        Self {
            background: rgb(0x1a, 0x1b, 0x26),
            foreground: rgb(0xc0, 0xca, 0xf5),
            cursor: rgb(0xc0, 0xca, 0xf5),
            selection: rgb(0x33, 0x46, 0x7c),
            ansi: [
                rgb(0x15, 0x16, 0x1e),
                rgb(0xf7, 0x76, 0x8e),
                rgb(0x9e, 0xce, 0x6a),
                rgb(0xe0, 0xaf, 0x68),
                rgb(0x7a, 0xa2, 0xf7),
                rgb(0xbb, 0x9a, 0xf7),
                rgb(0x7d, 0xcf, 0xff),
                rgb(0xa9, 0xb1, 0xd6),
                rgb(0x41, 0x48, 0x68),
                rgb(0xf7, 0x76, 0x8e),
                rgb(0x9e, 0xce, 0x6a),
                rgb(0xe0, 0xaf, 0x68),
                rgb(0x7a, 0xa2, 0xf7),
                rgb(0xbb, 0x9a, 0xf7),
                rgb(0x7d, 0xcf, 0xff),
                rgb(0xc0, 0xca, 0xf5),
            ],
            sidebar_background: rgb(0x16, 0x16, 0x1e),
            sidebar_hover: rgb(0x1f, 0x23, 0x35),
            sidebar_active: rgb(0x24, 0x28, 0x3b),
            sidebar_text: rgb(0xc0, 0xca, 0xf5),
            sidebar_text_dim: rgb(0x56, 0x5f, 0x89),
            accent: rgb(0x7a, 0xa2, 0xf7),
            border: rgb(0x2a, 0x2e, 0x3f),
            diff_add_background: rgb(0x1b, 0x2b, 0x1f),
            diff_remove_background: rgb(0x2b, 0x1b, 0x1f),
            diff_add_text: rgb(0x9e, 0xce, 0x6a),
            diff_remove_text: rgb(0xf7, 0x76, 0x8e),
            diff_hunk_text: rgb(0x7a, 0xa2, 0xf7),
            diff_meta_text: rgb(0x56, 0x5f, 0x89),
        }
    }

    /// Catppuccin Mocha.
    pub const fn catppuccin_mocha() -> Self {
        Self {
            background: rgb(0x1e, 0x1e, 0x2e),
            foreground: rgb(0xcd, 0xd6, 0xf4),
            cursor: rgb(0xf5, 0xe0, 0xdc),
            selection: rgb(0x58, 0x5b, 0x70),
            ansi: [
                rgb(0x45, 0x47, 0x5a),
                rgb(0xf3, 0x8b, 0xa8),
                rgb(0xa6, 0xe3, 0xa1),
                rgb(0xf9, 0xe2, 0xaf),
                rgb(0x89, 0xb4, 0xfa),
                rgb(0xf5, 0xc2, 0xe7),
                rgb(0x94, 0xe2, 0xd5),
                rgb(0xba, 0xc2, 0xde),
                rgb(0x58, 0x5b, 0x70),
                rgb(0xf3, 0x8b, 0xa8),
                rgb(0xa6, 0xe3, 0xa1),
                rgb(0xf9, 0xe2, 0xaf),
                rgb(0x89, 0xb4, 0xfa),
                rgb(0xf5, 0xc2, 0xe7),
                rgb(0x94, 0xe2, 0xd5),
                rgb(0xa6, 0xad, 0xc8),
            ],
            sidebar_background: rgb(0x18, 0x18, 0x25),
            sidebar_hover: rgb(0x31, 0x32, 0x44),
            sidebar_active: rgb(0x45, 0x47, 0x5a),
            sidebar_text: rgb(0xcd, 0xd6, 0xf4),
            sidebar_text_dim: rgb(0x7f, 0x84, 0x9c),
            accent: rgb(0xcb, 0xa6, 0xf7),
            border: rgb(0x31, 0x32, 0x44),
            diff_add_background: rgb(0x1f, 0x35, 0x2c),
            diff_remove_background: rgb(0x3a, 0x22, 0x2b),
            diff_add_text: rgb(0xa6, 0xe3, 0xa1),
            diff_remove_text: rgb(0xf3, 0x8b, 0xa8),
            diff_hunk_text: rgb(0x89, 0xb4, 0xfa),
            diff_meta_text: rgb(0x7f, 0x84, 0x9c),
        }
    }

    /// Dracula.
    pub const fn dracula() -> Self {
        Self {
            background: rgb(0x28, 0x2a, 0x36),
            foreground: rgb(0xf8, 0xf8, 0xf2),
            cursor: rgb(0xf8, 0xf8, 0xf2),
            selection: rgb(0x44, 0x47, 0x5a),
            ansi: [
                rgb(0x21, 0x22, 0x2c),
                rgb(0xff, 0x55, 0x55),
                rgb(0x50, 0xfa, 0x7b),
                rgb(0xf1, 0xfa, 0x8c),
                rgb(0xbd, 0x93, 0xf9),
                rgb(0xff, 0x79, 0xc6),
                rgb(0x8b, 0xe9, 0xfd),
                rgb(0xf8, 0xf8, 0xf2),
                rgb(0x62, 0x72, 0xa4),
                rgb(0xff, 0x6e, 0x6e),
                rgb(0x69, 0xff, 0x94),
                rgb(0xff, 0xff, 0xa5),
                rgb(0xd6, 0xac, 0xff),
                rgb(0xff, 0x92, 0xdf),
                rgb(0xa4, 0xff, 0xff),
                rgb(0xff, 0xff, 0xff),
            ],
            sidebar_background: rgb(0x21, 0x22, 0x2c),
            sidebar_hover: rgb(0x34, 0x37, 0x46),
            sidebar_active: rgb(0x44, 0x47, 0x5a),
            sidebar_text: rgb(0xf8, 0xf8, 0xf2),
            sidebar_text_dim: rgb(0x62, 0x72, 0xa4),
            accent: rgb(0xbd, 0x93, 0xf9),
            border: rgb(0x44, 0x47, 0x5a),
            diff_add_background: rgb(0x1f, 0x35, 0x25),
            diff_remove_background: rgb(0x3a, 0x22, 0x28),
            diff_add_text: rgb(0x50, 0xfa, 0x7b),
            diff_remove_text: rgb(0xff, 0x55, 0x55),
            diff_hunk_text: rgb(0x8b, 0xe9, 0xfd),
            diff_meta_text: rgb(0x62, 0x72, 0xa4),
        }
    }

    /// Nord.
    pub const fn nord() -> Self {
        Self {
            background: rgb(0x2e, 0x34, 0x40),
            foreground: rgb(0xd8, 0xde, 0xe9),
            cursor: rgb(0xd8, 0xde, 0xe9),
            selection: rgb(0x43, 0x4c, 0x5e),
            ansi: [
                rgb(0x3b, 0x42, 0x52),
                rgb(0xbf, 0x61, 0x6a),
                rgb(0xa3, 0xbe, 0x8c),
                rgb(0xeb, 0xcb, 0x8b),
                rgb(0x81, 0xa1, 0xc1),
                rgb(0xb4, 0x8e, 0xad),
                rgb(0x88, 0xc0, 0xd0),
                rgb(0xe5, 0xe9, 0xf0),
                rgb(0x4c, 0x56, 0x6a),
                rgb(0xbf, 0x61, 0x6a),
                rgb(0xa3, 0xbe, 0x8c),
                rgb(0xeb, 0xcb, 0x8b),
                rgb(0x81, 0xa1, 0xc1),
                rgb(0xb4, 0x8e, 0xad),
                rgb(0x8f, 0xbc, 0xbb),
                rgb(0xec, 0xef, 0xf4),
            ],
            sidebar_background: rgb(0x29, 0x2e, 0x39),
            sidebar_hover: rgb(0x3b, 0x42, 0x52),
            sidebar_active: rgb(0x43, 0x4c, 0x5e),
            sidebar_text: rgb(0xd8, 0xde, 0xe9),
            sidebar_text_dim: rgb(0x61, 0x6e, 0x88),
            accent: rgb(0x88, 0xc0, 0xd0),
            border: rgb(0x43, 0x4c, 0x5e),
            diff_add_background: rgb(0x2f, 0x3d, 0x33),
            diff_remove_background: rgb(0x3d, 0x2f, 0x33),
            diff_add_text: rgb(0xa3, 0xbe, 0x8c),
            diff_remove_text: rgb(0xbf, 0x61, 0x6a),
            diff_hunk_text: rgb(0x81, 0xa1, 0xc1),
            diff_meta_text: rgb(0x61, 0x6e, 0x88),
        }
    }

    /// Gruvbox Dark.
    pub const fn gruvbox_dark() -> Self {
        Self {
            background: rgb(0x28, 0x28, 0x28),
            foreground: rgb(0xeb, 0xdb, 0xb2),
            cursor: rgb(0xeb, 0xdb, 0xb2),
            selection: rgb(0x50, 0x49, 0x45),
            ansi: [
                rgb(0x28, 0x28, 0x28),
                rgb(0xcc, 0x24, 0x1d),
                rgb(0x98, 0x97, 0x1a),
                rgb(0xd7, 0x99, 0x21),
                rgb(0x45, 0x85, 0x88),
                rgb(0xb1, 0x62, 0x86),
                rgb(0x68, 0x9d, 0x6a),
                rgb(0xa8, 0x99, 0x84),
                rgb(0x92, 0x83, 0x74),
                rgb(0xfb, 0x49, 0x34),
                rgb(0xb8, 0xbb, 0x26),
                rgb(0xfa, 0xbd, 0x2f),
                rgb(0x83, 0xa5, 0x98),
                rgb(0xd3, 0x86, 0x9b),
                rgb(0x8e, 0xc0, 0x7c),
                rgb(0xeb, 0xdb, 0xb2),
            ],
            sidebar_background: rgb(0x1d, 0x20, 0x21),
            sidebar_hover: rgb(0x32, 0x30, 0x2f),
            sidebar_active: rgb(0x3c, 0x38, 0x36),
            sidebar_text: rgb(0xeb, 0xdb, 0xb2),
            sidebar_text_dim: rgb(0x92, 0x83, 0x74),
            accent: rgb(0xfa, 0xbd, 0x2f),
            border: rgb(0x3c, 0x38, 0x36),
            diff_add_background: rgb(0x2a, 0x3a, 0x2a),
            diff_remove_background: rgb(0x3a, 0x2a, 0x2a),
            diff_add_text: rgb(0xb8, 0xbb, 0x26),
            diff_remove_text: rgb(0xfb, 0x49, 0x34),
            diff_hunk_text: rgb(0x83, 0xa5, 0x98),
            diff_meta_text: rgb(0x92, 0x83, 0x74),
        }
    }

    /// One Dark.
    pub const fn one_dark() -> Self {
        Self {
            background: rgb(0x28, 0x2c, 0x34),
            foreground: rgb(0xab, 0xb2, 0xbf),
            cursor: rgb(0x52, 0x8b, 0xff),
            selection: rgb(0x3e, 0x44, 0x51),
            ansi: [
                rgb(0x28, 0x2c, 0x34),
                rgb(0xe0, 0x6c, 0x75),
                rgb(0x98, 0xc3, 0x79),
                rgb(0xe5, 0xc0, 0x7b),
                rgb(0x61, 0xaf, 0xef),
                rgb(0xc6, 0x78, 0xdd),
                rgb(0x56, 0xb6, 0xc2),
                rgb(0xab, 0xb2, 0xbf),
                rgb(0x5c, 0x63, 0x70),
                rgb(0xe0, 0x6c, 0x75),
                rgb(0x98, 0xc3, 0x79),
                rgb(0xe5, 0xc0, 0x7b),
                rgb(0x61, 0xaf, 0xef),
                rgb(0xc6, 0x78, 0xdd),
                rgb(0x56, 0xb6, 0xc2),
                rgb(0xff, 0xff, 0xff),
            ],
            sidebar_background: rgb(0x21, 0x25, 0x2b),
            sidebar_hover: rgb(0x2c, 0x31, 0x3a),
            sidebar_active: rgb(0x3a, 0x3f, 0x4b),
            sidebar_text: rgb(0xab, 0xb2, 0xbf),
            sidebar_text_dim: rgb(0x5c, 0x63, 0x70),
            accent: rgb(0x61, 0xaf, 0xef),
            border: rgb(0x3a, 0x3f, 0x4b),
            diff_add_background: rgb(0x24, 0x34, 0x2a),
            diff_remove_background: rgb(0x38, 0x26, 0x2a),
            diff_add_text: rgb(0x98, 0xc3, 0x79),
            diff_remove_text: rgb(0xe0, 0x6c, 0x75),
            diff_hunk_text: rgb(0x61, 0xaf, 0xef),
            diff_meta_text: rgb(0x5c, 0x63, 0x70),
        }
    }

    /// Solarized Dark.
    pub const fn solarized_dark() -> Self {
        Self {
            background: rgb(0x00, 0x2b, 0x36),
            foreground: rgb(0x83, 0x94, 0x96),
            cursor: rgb(0x93, 0xa1, 0xa1),
            selection: rgb(0x07, 0x36, 0x42),
            ansi: [
                rgb(0x07, 0x36, 0x42),
                rgb(0xdc, 0x32, 0x2f),
                rgb(0x85, 0x99, 0x00),
                rgb(0xb5, 0x89, 0x00),
                rgb(0x26, 0x8b, 0xd2),
                rgb(0xd3, 0x36, 0x82),
                rgb(0x2a, 0xa1, 0x98),
                rgb(0xee, 0xe8, 0xd5),
                rgb(0x58, 0x6e, 0x75),
                rgb(0xcb, 0x4b, 0x16),
                rgb(0x58, 0x6e, 0x75),
                rgb(0x65, 0x7b, 0x83),
                rgb(0x83, 0x94, 0x96),
                rgb(0x6c, 0x71, 0xc4),
                rgb(0x93, 0xa1, 0xa1),
                rgb(0xfd, 0xf6, 0xe3),
            ],
            sidebar_background: rgb(0x00, 0x22, 0x2c),
            sidebar_hover: rgb(0x07, 0x36, 0x42),
            sidebar_active: rgb(0x0a, 0x4b, 0x5a),
            sidebar_text: rgb(0x93, 0xa1, 0xa1),
            sidebar_text_dim: rgb(0x58, 0x6e, 0x75),
            accent: rgb(0x26, 0x8b, 0xd2),
            border: rgb(0x07, 0x36, 0x42),
            diff_add_background: rgb(0x0b, 0x33, 0x2d),
            diff_remove_background: rgb(0x33, 0x1c, 0x24),
            diff_add_text: rgb(0x85, 0x99, 0x00),
            diff_remove_text: rgb(0xdc, 0x32, 0x2f),
            diff_hunk_text: rgb(0x26, 0x8b, 0xd2),
            diff_meta_text: rgb(0x58, 0x6e, 0x75),
        }
    }

    /// GitHub Dark.
    pub const fn github_dark() -> Self {
        Self {
            background: rgb(0x0d, 0x11, 0x17),
            foreground: rgb(0xc9, 0xd1, 0xd9),
            cursor: rgb(0x58, 0xa6, 0xff),
            selection: rgb(0x26, 0x4f, 0x78),
            ansi: [
                rgb(0x48, 0x4f, 0x58),
                rgb(0xff, 0x7b, 0x72),
                rgb(0x3f, 0xb9, 0x50),
                rgb(0xd2, 0x99, 0x22),
                rgb(0x58, 0xa6, 0xff),
                rgb(0xbc, 0x8c, 0xff),
                rgb(0x39, 0xc5, 0xcf),
                rgb(0xb1, 0xba, 0xc4),
                rgb(0x6e, 0x76, 0x81),
                rgb(0xff, 0xa1, 0x98),
                rgb(0x56, 0xd3, 0x64),
                rgb(0xe3, 0xb3, 0x41),
                rgb(0x79, 0xc0, 0xff),
                rgb(0xd2, 0xa8, 0xff),
                rgb(0x56, 0xd4, 0xdd),
                rgb(0xf0, 0xf6, 0xfc),
            ],
            sidebar_background: rgb(0x01, 0x04, 0x09),
            sidebar_hover: rgb(0x16, 0x1b, 0x22),
            sidebar_active: rgb(0x21, 0x26, 0x2d),
            sidebar_text: rgb(0xc9, 0xd1, 0xd9),
            sidebar_text_dim: rgb(0x8b, 0x94, 0x9e),
            accent: rgb(0x58, 0xa6, 0xff),
            border: rgb(0x30, 0x36, 0x3d),
            diff_add_background: rgb(0x12, 0x26, 0x1e),
            diff_remove_background: rgb(0x2d, 0x12, 0x14),
            diff_add_text: rgb(0x3f, 0xb9, 0x50),
            diff_remove_text: rgb(0xff, 0x7b, 0x72),
            diff_hunk_text: rgb(0x58, 0xa6, 0xff),
            diff_meta_text: rgb(0x8b, 0x94, 0x9e),
        }
    }

    /// Kanagawa.
    pub const fn kanagawa() -> Self {
        Self {
            background: rgb(0x1f, 0x1f, 0x28),
            foreground: rgb(0xdc, 0xd7, 0xba),
            cursor: rgb(0xc8, 0xc0, 0x93),
            selection: rgb(0x2d, 0x4f, 0x67),
            ansi: [
                rgb(0x09, 0x06, 0x18),
                rgb(0xc3, 0x40, 0x43),
                rgb(0x76, 0x94, 0x6a),
                rgb(0xc0, 0xa3, 0x6e),
                rgb(0x7e, 0x9c, 0xd8),
                rgb(0x95, 0x7f, 0xb8),
                rgb(0x6a, 0x95, 0x89),
                rgb(0xc8, 0xc0, 0x93),
                rgb(0x72, 0x71, 0x69),
                rgb(0xe8, 0x24, 0x24),
                rgb(0x98, 0xbb, 0x6c),
                rgb(0xe6, 0xc3, 0x84),
                rgb(0x7f, 0xb4, 0xca),
                rgb(0x93, 0x8a, 0xa9),
                rgb(0x7a, 0xa8, 0x9f),
                rgb(0xdc, 0xd7, 0xba),
            ],
            sidebar_background: rgb(0x16, 0x16, 0x1d),
            sidebar_hover: rgb(0x2a, 0x2a, 0x37),
            sidebar_active: rgb(0x36, 0x36, 0x46),
            sidebar_text: rgb(0xdc, 0xd7, 0xba),
            sidebar_text_dim: rgb(0x72, 0x71, 0x69),
            accent: rgb(0x7e, 0x9c, 0xd8),
            border: rgb(0x2a, 0x2a, 0x37),
            diff_add_background: rgb(0x22, 0x33, 0x27),
            diff_remove_background: rgb(0x33, 0x22, 0x27),
            diff_add_text: rgb(0x98, 0xbb, 0x6c),
            diff_remove_text: rgb(0xc3, 0x40, 0x43),
            diff_hunk_text: rgb(0x7e, 0x9c, 0xd8),
            diff_meta_text: rgb(0x72, 0x71, 0x69),
        }
    }

    /// Resolve a cell color, honoring OSC 4/10/11 overrides stored by `Term`.
    pub fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Named(named) => {
                if let Some(rgb) = overrides[named] {
                    return rgb;
                }
                self.named(named)
            }
            Color::Indexed(index) => {
                if let Some(rgb) = overrides[index as usize] {
                    return rgb;
                }
                self.indexed(index)
            }
            Color::Spec(rgb) => rgb,
        }
    }

    /// Resolve a color by the index used in OSC color queries.
    ///
    /// 0..=255 are palette entries, 256 foreground, 257 background,
    /// 258 cursor, 259..=266 dim ANSI colors, 267 bright foreground,
    /// 268 dim foreground.
    pub fn special(&self, index: usize) -> Rgb {
        match index {
            0..=255 => self.indexed(index as u8),
            256 => self.foreground,
            257 => self.background,
            258 => self.cursor,
            259..=266 => dim(self.ansi[index - 259], 0.66),
            267 => self.foreground,
            268 => dim(self.foreground, 0.66),
            _ => self.foreground,
        }
    }

    /// Raw 256-color palette lookup.
    pub fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi[index as usize],
            16..=231 => {
                const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
                let i = index as usize - 16;
                Rgb {
                    r: LEVELS[i / 36],
                    g: LEVELS[(i / 6) % 6],
                    b: LEVELS[i % 6],
                }
            }
            232..=255 => {
                let level = 8 + (index as u16 - 232) * 10;
                let level = level as u8;
                Rgb {
                    r: level,
                    g: level,
                    b: level,
                }
            }
        }
    }

    fn named(&self, named: NamedColor) -> Rgb {
        match named {
            NamedColor::Black => self.ansi[0],
            NamedColor::Red => self.ansi[1],
            NamedColor::Green => self.ansi[2],
            NamedColor::Yellow => self.ansi[3],
            NamedColor::Blue => self.ansi[4],
            NamedColor::Magenta => self.ansi[5],
            NamedColor::Cyan => self.ansi[6],
            NamedColor::White => self.ansi[7],
            NamedColor::BrightBlack => self.ansi[8],
            NamedColor::BrightRed => self.ansi[9],
            NamedColor::BrightGreen => self.ansi[10],
            NamedColor::BrightYellow => self.ansi[11],
            NamedColor::BrightBlue => self.ansi[12],
            NamedColor::BrightMagenta => self.ansi[13],
            NamedColor::BrightCyan => self.ansi[14],
            NamedColor::BrightWhite => self.ansi[15],
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            NamedColor::DimBlack => dim(self.ansi[0], 0.66),
            NamedColor::DimRed => dim(self.ansi[1], 0.66),
            NamedColor::DimGreen => dim(self.ansi[2], 0.66),
            NamedColor::DimYellow => dim(self.ansi[3], 0.66),
            NamedColor::DimBlue => dim(self.ansi[4], 0.66),
            NamedColor::DimMagenta => dim(self.ansi[5], 0.66),
            NamedColor::DimCyan => dim(self.ansi[6], 0.66),
            NamedColor::DimWhite => dim(self.ansi[7], 0.66),
            NamedColor::DimForeground => dim(self.foreground, 0.66),
        }
    }
}

fn dim(rgb: Rgb, factor: f32) -> Rgb {
    Rgb {
        r: (rgb.r as f32 * factor) as u8,
        g: (rgb.g as f32 * factor) as u8,
        b: (rgb.b as f32 * factor) as u8,
    }
}

/// Convert an sRGB byte color into linear RGBA for GPU rendering.
pub fn to_linear(rgb: Rgb, alpha: f32) -> [f32; 4] {
    fn channel(value: u8) -> f32 {
        let value = value as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    [channel(rgb.r), channel(rgb.g), channel(rgb.b), alpha]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_theme_aliases() {
        assert_eq!(
            Theme::from_name("foundry").map(|theme| theme.background),
            Some(Theme::foundry().background)
        );
        assert_eq!(
            Theme::from_name("tokyo_night").map(|theme| theme.background),
            Some(Theme::tokyo_night().background)
        );
        assert_eq!(
            Theme::from_name("Catppuccin Mocha").map(|theme| theme.background),
            Some(Theme::catppuccin_mocha().background)
        );
        assert!(Theme::from_name("dracula").is_some());
        assert!(Theme::from_name("nord").is_some());
        assert!(Theme::from_name("gruvbox").is_some());
        assert!(Theme::from_name("one-dark").is_some());
        assert!(Theme::from_name("solarized").is_some());
        assert!(Theme::from_name("github").is_some());
        assert!(Theme::from_name("kanagawa").is_some());
        assert!(Theme::from_name("does-not-exist").is_none());
    }

    #[test]
    fn themes_have_distinct_backgrounds_and_complete_ansi_palettes() {
        let themes = [
            Theme::foundry(),
            Theme::tokyo_night(),
            Theme::catppuccin_mocha(),
            Theme::dracula(),
            Theme::nord(),
            Theme::gruvbox_dark(),
            Theme::one_dark(),
            Theme::solarized_dark(),
            Theme::github_dark(),
            Theme::kanagawa(),
        ];
        for (index, theme) in themes.iter().enumerate() {
            assert_eq!(theme.ansi.len(), 16);
            for other in &themes[index + 1..] {
                assert_ne!(theme.background, other.background);
            }
        }
    }
}
