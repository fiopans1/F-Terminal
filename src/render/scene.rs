use crate::layout::Rect;
use crate::render::quads::Quad;
use crate::term::color::Rgb;
use crate::term::frame::Frame;

/// Inline completion text drawn at a cell position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostText {
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub color: Rgb,
}

/// A terminal pane to render. `id` is only used to reuse the shaped text
/// buffer between frames.
pub struct PaneLayer<'a> {
    pub id: u64,
    /// Area available to the pane text, in physical pixels.
    pub content: Rect,
    pub frame: &'a Frame,
    /// Whether the text buffer needs to be reshaped.
    pub dirty: bool,
    /// Optional inline completion ghost text.
    pub ghost: Option<GhostText>,
}

/// A simple single-color label for the sidebar and other chrome.
pub struct Label {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub font_size: f32,
    pub color: Rgb,
    pub bold: bool,
}

/// Everything needed to draw one window frame.
pub struct Scene<'a> {
    pub panes: Vec<PaneLayer<'a>>,
    pub labels: Vec<Label>,
    pub quads: Vec<Quad>,
}
