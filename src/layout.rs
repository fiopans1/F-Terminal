use crate::workspace::{LayoutNode, PaneId, SplitDirection};

/// Rectangle in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Shrink the rectangle by `amount` on every side.
    pub fn inset(&self, amount: f32) -> Self {
        Self {
            x: self.x + amount,
            y: self.y + amount,
            width: (self.width - amount * 2.0).max(0.0),
            height: (self.height - amount * 2.0).max(0.0),
        }
    }
}
/// A pane as placed by the layout engine.
#[derive(Clone, Copy, Debug)]
pub struct PaneLayout {
    pub id: PaneId,
    /// Area assigned to the pane, including its internal padding.
    pub outer: Rect,
    /// Area available for the terminal grid.
    pub content: Rect,
    /// Status bar shown below the terminal grid.
    pub footer: Rect,
}

/// A draggable split divider.
#[derive(Clone, Copy, Debug)]
pub struct DividerLayout {
    pub split_id: u64,
    pub direction: SplitDirection,
    /// The whole region occupied by the split (both children and the gap).
    pub area: Rect,
    /// The thin rectangle that visually separates the panes.
    pub line: Rect,
}

#[derive(Debug, Default)]
pub struct LayoutResult {
    pub panes: Vec<PaneLayout>,
    pub dividers: Vec<DividerLayout>,
}

/// Compute pane rectangles for a tab's split tree.
pub fn compute_layout(
    root: &LayoutNode,
    area: Rect,
    gap: f32,
    padding: f32,
    left_padding: f32,
    footer_height: f32,
) -> LayoutResult {
    let mut result = LayoutResult::default();
    walk(
        root,
        area,
        gap.max(1.0),
        padding,
        left_padding.max(0.0),
        footer_height.max(0.0),
        &mut result,
    );
    result
}

fn walk(
    node: &LayoutNode,
    area: Rect,
    gap: f32,
    padding: f32,
    left_padding: f32,
    footer_height: f32,
    result: &mut LayoutResult,
) {
    match node {
        LayoutNode::Leaf(id) => {
            let padded = area.inset(padding);
            let left = left_padding.min((padded.width - 1.0).max(0.0));
            let content_width = (padded.width - left).max(1.0);
            let content_x = padded.x + left;
            let footer_height = footer_height
                .min(padded.height * 0.3)
                .min((padded.height - 1.0).max(0.0));
            let content_height = (padded.height - footer_height).max(1.0);
            result.panes.push(PaneLayout {
                id: *id,
                outer: area,
                content: Rect::new(content_x, padded.y, content_width, content_height),
                footer: Rect::new(
                    content_x,
                    padded.y + content_height,
                    content_width,
                    (padded.height - content_height).max(0.0),
                ),
            });
        }
        LayoutNode::Split(split) => {
            let (first_area, second_area, line) =
                split_rect(area, split.direction, split.ratio, gap);

            result.dividers.push(DividerLayout {
                split_id: split.id,
                direction: split.direction,
                area,
                line,
            });

            walk(
                &split.first,
                first_area,
                gap,
                padding,
                left_padding,
                footer_height,
                result,
            );
            walk(
                &split.second,
                second_area,
                gap,
                padding,
                left_padding,
                footer_height,
                result,
            );
        }
    }
}

fn split_rect(area: Rect, direction: SplitDirection, ratio: f32, gap: f32) -> (Rect, Rect, Rect) {
    let ratio = ratio.clamp(0.05, 0.95);

    match direction {
        // `first` is always the leading child, so left and right share the
        // same geometry: left/top first, right/bottom second.
        SplitDirection::Left | SplitDirection::Right => {
            let usable = (area.width - gap).max(1.0);
            let first_width = (usable * ratio).clamp(1.0, usable - 1.0);
            let second_width = (usable - first_width).max(1.0);

            (
                Rect::new(area.x, area.y, first_width, area.height),
                Rect::new(
                    area.x + first_width + gap,
                    area.y,
                    second_width,
                    area.height,
                ),
                Rect::new(area.x + first_width, area.y, gap, area.height),
            )
        }
        SplitDirection::Up | SplitDirection::Down => {
            let usable = (area.height - gap).max(1.0);
            let first_height = (usable * ratio).clamp(1.0, usable - 1.0);
            let second_height = (usable - first_height).max(1.0);

            (
                Rect::new(area.x, area.y, area.width, first_height),
                Rect::new(
                    area.x,
                    area.y + first_height + gap,
                    area.width,
                    second_height,
                ),
                Rect::new(area.x, area.y + first_height, area.width, gap),
            )
        }
    }
}

#[cfg(test)]
#[path = "tests/layout.rs"]
mod tests;
