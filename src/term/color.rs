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
}

impl Theme {
    /// Tokyo Night inspired dark palette.
    pub const fn tokyo_night() -> Self {
        const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
            Rgb { r, g, b }
        }

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
