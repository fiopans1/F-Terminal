//! Shared visual language for F-Terminal's auxiliary windows.
//!
//! The app draws its own chrome, so these helpers are the equivalent of a
//! small design system: type scale, spacing, buttons, focus rings, empty
//! states and scrollbars. Keeping them here prevents Git, Code Review and
//! Agent Mode from drifting into three different products.

use crate::layout::Rect;
use crate::render::quads::Quad;
use crate::render::scene::Label;
use crate::term::color::{Rgb, Theme, to_linear};

/// Shared shell heights, in logical pixels.
pub const HEADER_HEIGHT: f32 = 56.0;
pub const FOOTER_HEIGHT: f32 = 28.0;
pub const GUTTER: f32 = 16.0;
pub const CLOSE_SIZE: f32 = 30.0;
pub const BUTTON_HEIGHT: f32 = 30.0;
pub const RADIUS: f32 = 6.0;

/// Convert a logical pixel value to the physical value used by the renderer.
pub fn px(value: f32, scale: f32) -> f32 {
    (value * scale).round()
}

/// A compact type scale with legibility floors. Web guidelines recommend
/// avoiding text below ~11 pt; the previous windows went down to ~8 pt.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub scale: f32,
    pub caption: f32,
    pub small: f32,
    pub body: f32,
    pub section: f32,
    pub title: f32,
}

impl Tokens {
    pub fn new(ui_font: f32, scale: f32) -> Self {
        let ui = ui_font.max(px(12.0, scale));
        Self {
            scale,
            caption: (ui * 0.9).max(px(11.0, scale)),
            small: (ui * 0.96).max(px(12.0, scale)),
            body: ui,
            section: (ui * 1.22).max(px(15.0, scale)),
            title: (ui * 1.48).max(px(18.0, scale)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

#[derive(Clone, Copy, Debug)]
pub struct ButtonSpec<'a> {
    pub rect: Rect,
    pub label: &'a str,
    pub kind: ButtonKind,
    pub enabled: bool,
    pub hovered: bool,
    pub focused: bool,
}

/// Draw a compact desktop button. Text is left-aligned with consistent
/// padding; primary, secondary, ghost and danger share the same geometry.
pub fn button(
    quads: &mut Vec<Quad>,
    labels: &mut Vec<Label>,
    spec: ButtonSpec<'_>,
    theme: &Theme,
    tokens: Tokens,
) {
    let ButtonSpec {
        rect,
        label,
        kind,
        enabled,
        hovered,
        focused,
    } = spec;
    if rect.width <= 1.0 || rect.height <= 1.0 {
        return;
    }

    let (background, text) = match (kind, enabled) {
        (_, false) => (theme.sidebar_background, theme.sidebar_text_dim),
        (ButtonKind::Primary, true) => (
            if hovered {
                mix(theme.accent, theme.foreground, 0.14)
            } else {
                theme.accent
            },
            theme.background,
        ),
        (ButtonKind::Secondary, true) => (
            if hovered {
                mix(theme.sidebar_active, theme.foreground, 0.08)
            } else {
                theme.sidebar_active
            },
            theme.sidebar_text,
        ),
        (ButtonKind::Ghost, true) => (
            if hovered {
                theme.sidebar_hover
            } else {
                theme.background
            },
            if hovered {
                theme.sidebar_text
            } else {
                theme.sidebar_text_dim
            },
        ),
        (ButtonKind::Danger, true) => (
            mix(
                theme.background,
                theme.ansi[1],
                if hovered { 0.24 } else { 0.14 },
            ),
            theme.ansi[1],
        ),
    };

    let radius = (RADIUS * tokens.scale).min(rect.height / 2.0);
    let draw_background = kind != ButtonKind::Ghost || hovered || !enabled;
    if draw_background {
        quads.push(Quad::rounded(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            radius,
            to_linear(background, 1.0),
        ));
    }
    if kind == ButtonKind::Ghost && !hovered {
        quads.push(Quad::new(
            rect.x,
            rect.bottom() - 1.0,
            rect.width,
            1.0,
            to_linear(theme.border, 1.0),
        ));
    }

    labels.push(Label {
        text: label.to_owned(),
        x: rect.x + px(11.0, tokens.scale),
        y: rect.y + (rect.height - tokens.small * 1.35) / 2.0,
        width: (rect.width - px(18.0, tokens.scale)).max(1.0),
        font_size: tokens.small,
        color: text,
        bold: kind == ButtonKind::Primary,
    });

    if focused {
        focus_ring(quads, rect, theme, tokens.scale);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IconButtonSpec<'a> {
    pub rect: Rect,
    pub glyph: &'a str,
    pub enabled: bool,
    pub hovered: bool,
    pub focused: bool,
    pub danger: bool,
}

/// A square icon button with a text glyph. The glyph is approximately
/// centered without shaping the text at layout time.
pub fn icon_button(
    quads: &mut Vec<Quad>,
    labels: &mut Vec<Label>,
    spec: IconButtonSpec<'_>,
    theme: &Theme,
    tokens: Tokens,
) {
    let IconButtonSpec {
        rect,
        glyph,
        enabled,
        hovered,
        focused,
        danger,
    } = spec;
    let color = if !enabled {
        theme.sidebar_text_dim
    } else if danger {
        theme.ansi[1]
    } else if hovered {
        theme.sidebar_text
    } else {
        theme.sidebar_text_dim
    };
    if hovered && enabled {
        quads.push(Quad::rounded(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            RADIUS * tokens.scale,
            to_linear(
                if danger {
                    mix(theme.background, theme.ansi[1], 0.16)
                } else {
                    theme.sidebar_hover
                },
                1.0,
            ),
        ));
    }
    labels.push(Label {
        text: glyph.to_owned(),
        x: rect.x + (rect.width - tokens.body * 0.55) / 2.0,
        y: rect.y + (rect.height - tokens.body * 1.35) / 2.0,
        width: rect.width,
        font_size: tokens.body,
        color,
        bold: false,
    });
    if focused {
        focus_ring(quads, rect, theme, tokens.scale);
    }
}

pub fn focus_ring(quads: &mut Vec<Quad>, rect: Rect, theme: &Theme, scale: f32) {
    let thickness = (1.0 * scale).max(1.0);
    let color = to_linear(theme.accent, 0.9);
    quads.push(Quad::new(rect.x, rect.y, rect.width, thickness, color));
    quads.push(Quad::new(
        rect.x,
        rect.bottom() - thickness,
        rect.width,
        thickness,
        color,
    ));
    quads.push(Quad::new(rect.x, rect.y, thickness, rect.height, color));
    quads.push(Quad::new(
        rect.right() - thickness,
        rect.y,
        thickness,
        rect.height,
        color,
    ));
}

pub fn status_dot(quads: &mut Vec<Quad>, x: f32, y: f32, size: f32, color: Rgb, halo: bool) {
    if halo {
        quads.push(Quad::rounded(
            x - size * 0.35,
            y - size * 0.35,
            size * 1.7,
            size * 1.7,
            size * 0.85,
            to_linear(color, 0.16),
        ));
    }
    quads.push(Quad::rounded(
        x,
        y,
        size,
        size,
        size / 2.0,
        to_linear(color, 1.0),
    ));
}

/// A minimal scrollbar. It only appears when the content is longer than the
/// viewport and is intentionally thin so it does not compete with the data.
pub fn scrollbar(
    quads: &mut Vec<Quad>,
    area: Rect,
    offset: usize,
    total: usize,
    visible: usize,
    theme: &Theme,
    scale: f32,
) {
    if total <= visible || visible == 0 || area.height <= 1.0 {
        return;
    }
    let width = (3.0 * scale).max(2.0);
    let x = area.right() - width - (3.0 * scale);
    let track = to_linear(theme.border, 0.55);
    quads.push(Quad::rounded(
        x,
        area.y + (4.0 * scale),
        width,
        (area.height - 8.0 * scale).max(1.0),
        width / 2.0,
        track,
    ));
    let max_offset = total.saturating_sub(visible);
    let fraction = (offset.min(max_offset) as f32 / max_offset as f32).clamp(0.0, 1.0);
    let thumb_height = (area.height * visible as f32 / total as f32)
        .max(24.0 * scale)
        .min(area.height - 8.0 * scale);
    let travel = (area.height - 8.0 * scale - thumb_height).max(0.0);
    quads.push(Quad::rounded(
        x,
        area.y + 4.0 * scale + travel * fraction,
        width,
        thumb_height,
        width / 2.0,
        to_linear(theme.accent, 0.7),
    ));
}

/// Static skeleton rows for a list that is still loading. No shimmer: motion
/// here would be decorative and distracting in a keyboard-first window.
pub fn skeleton_rows(quads: &mut Vec<Quad>, area: Rect, rows: usize, theme: &Theme, scale: f32) {
    let line = (10.0 * scale).max(8.0);
    let gap = (22.0 * scale).max(18.0);
    let x = area.x + px(GUTTER, scale);
    let max_width = (area.width - px(GUTTER * 2.0, scale)).max(1.0);
    for index in 0..rows {
        let y = area.y + px(12.0, scale) + index as f32 * gap;
        if y + line > area.bottom() - px(8.0, scale) {
            break;
        }
        let width = max_width * if index % 2 == 0 { 0.58 } else { 0.42 };
        quads.push(Quad::rounded(
            x,
            y,
            width,
            line,
            line / 2.0,
            to_linear(theme.sidebar_hover, 1.0),
        ));
        quads.push(Quad::rounded(
            x,
            y + line + px(4.0, scale),
            width * 0.55,
            (line * 0.7).max(5.0),
            line * 0.35,
            to_linear(theme.border, 0.8),
        ));
    }
}

/// Plain, action-oriented empty state. The small accent rail is the only
/// decorative element; the copy carries the direction.
pub fn empty_state(
    quads: &mut Vec<Quad>,
    labels: &mut Vec<Label>,
    area: Rect,
    title: &str,
    body: &str,
    theme: &Theme,
    tokens: Tokens,
) {
    let x = area.x + px(GUTTER, tokens.scale);
    let y = area.y + (area.height * 0.3).max(tokens.title * 2.0);
    let width = (area.width - px(GUTTER * 2.0, tokens.scale)).max(1.0);
    quads.push(Quad::new(
        x,
        y,
        (2.0 * tokens.scale).max(2.0),
        tokens.title * 1.15,
        to_linear(theme.accent, 0.75),
    ));
    labels.push(Label {
        text: title.to_owned(),
        x: x + px(12.0, tokens.scale),
        y,
        width,
        font_size: tokens.section,
        color: theme.sidebar_text,
        bold: true,
    });
    labels.push(Label {
        text: body.to_owned(),
        x: x + px(12.0, tokens.scale),
        y: y + tokens.section * 1.55,
        width,
        font_size: tokens.small,
        color: theme.sidebar_text_dim,
        bold: false,
    });
}

pub fn mix(a: Rgb, b: Rgb, amount: f32) -> Rgb {
    let amount = amount.clamp(0.0, 1.0);
    Rgb {
        r: (a.r as f32 + (b.r as f32 - a.r as f32) * amount) as u8,
        g: (a.g as f32 + (b.g as f32 - a.g as f32) * amount) as u8,
        b: (a.b as f32 + (b.b as f32 - a.b as f32) * amount) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_scale_keeps_legibility_floors_at_every_dpi() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let tokens = Tokens::new(12.5 * scale, scale);
            assert!(tokens.caption >= px(11.0, scale));
            assert!(tokens.small >= px(12.0, scale));
            assert!(tokens.body >= px(12.0, scale));
            assert!(tokens.section > tokens.body);
            assert!(tokens.title > tokens.section);
        }
    }

    #[test]
    fn mix_clamps_between_the_two_colors() {
        let a = Rgb {
            r: 10,
            g: 20,
            b: 30,
        };
        let b = Rgb {
            r: 210,
            g: 220,
            b: 230,
        };
        assert_eq!(mix(a, b, -1.0), a);
        assert_eq!(mix(a, b, 2.0), b);
        assert_eq!(mix(a, b, 0.5).r, 110);
    }
}
