use crate::layout::Rect;
use crate::workspace::TabId;

/// Tabs shown in the sidebar. This is a lightweight view so the sidebar does
/// not need access to the whole workspace.
#[derive(Clone, Copy, Debug)]
pub struct TabSummary {
    pub id: TabId,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarMode {
    #[default]
    Terminals,
    Files,
}

#[derive(Clone, Debug)]
pub struct Sidebar {
    pub visible: bool,
    pub mode: SidebarMode,
    pub scroll: f32,
    pub hovered: Option<TabId>,
    pub hovered_file: Option<usize>,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            visible: true,
            mode: SidebarMode::Terminals,
            scroll: 0.0,
            hovered: None,
            hovered_file: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SidebarItem {
    pub tab: TabId,
    pub rect: Rect,
    pub label: Rect,
    pub close: Rect,
    pub is_active: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct FileItem {
    /// Index into `FileExplorer::rows`.
    pub row: usize,
    pub rect: Rect,
    pub label: Rect,
}

#[derive(Clone, Debug, Default)]
pub struct SidebarLayout {
    pub rect: Rect,
    pub list: Rect,
    pub header: Rect,
    pub new_button: Rect,
    pub collapse_button: Rect,
    pub reveal_button: Rect,
    pub mode_button: Rect,
    pub header_label: Rect,
    pub path_rect: Rect,
    pub items: Vec<SidebarItem>,
    pub file_items: Vec<FileItem>,
    /// X coordinate where the terminal area starts.
    pub content_x: f32,
    pub max_scroll: f32,
    pub item_height: f32,
    pub padding: f32,
    pub collapsed: bool,
    pub mode: SidebarMode,
}

impl SidebarLayout {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarHit {
    ToggleSidebar,
    ToggleMode,
    NewTerminal,
    RefreshFiles,
    Select(TabId),
    Close(TabId),
    File(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutSpec {
    pub window_width: f32,
    pub window_height: f32,
    pub scale: f32,
    pub width_logical: f32,
    pub file_count: usize,
}

/// Lay out the sidebar and its clickable regions. All inputs and outputs are
/// in physical pixels.
pub fn layout(
    state: &Sidebar,
    tabs: &[TabSummary],
    active: Option<TabId>,
    spec: LayoutSpec,
) -> SidebarLayout {
    let LayoutSpec {
        window_width,
        window_height,
        scale,
        width_logical,
        file_count,
    } = spec;
    if !state.visible {
        let padding = (8.0 * scale).round();
        let rail_width = (14.0 * scale).round().max(padding * 2.0 + 4.0);
        let rail = Rect::new(0.0, 0.0, rail_width, window_height);

        return SidebarLayout {
            rect: rail,
            reveal_button: rail,
            content_x: rail_width,
            padding,
            collapsed: true,
            mode: state.mode,
            ..SidebarLayout::default()
        };
    }

    let width = (width_logical * scale)
        .round()
        .min((window_width * 0.45).max(1.0))
        .max((120.0 * scale).min(window_width));
    let padding = (8.0 * scale).round();
    let header_height = (36.0 * scale).round();
    let item_height = if state.mode == SidebarMode::Files {
        (24.0 * scale).round()
    } else {
        (30.0 * scale).round()
    };
    let item_gap = (2.0 * scale).round().max(1.0);
    let close_size = (18.0 * scale).round();
    let button_size = (22.0 * scale).round();
    let collapse_size = (20.0 * scale).round();
    let path_height = if state.mode == SidebarMode::Files {
        (22.0 * scale).round()
    } else {
        0.0
    };

    let rect = Rect::new(0.0, 0.0, width, window_height);
    let header = Rect::new(0.0, 0.0, width, header_height);
    let mode_button = Rect::new(
        (width - padding - button_size).max(0.0),
        ((header_height - button_size) / 2.0).max(0.0),
        button_size,
        button_size,
    );
    let new_button = Rect::new(
        (mode_button.x - 4.0 * scale - button_size).max(0.0),
        ((header_height - button_size) / 2.0).max(0.0),
        button_size,
        button_size,
    );
    let collapse_button = Rect::new(
        padding,
        ((header_height - collapse_size) / 2.0).max(0.0),
        collapse_size,
        collapse_size,
    );
    let header_label = Rect::new(
        collapse_button.right() + 8.0 * scale,
        0.0,
        (new_button.x - collapse_button.right() - 8.0 * scale - padding).max(1.0),
        header_height,
    );
    let path_rect = Rect::new(
        padding,
        header_height,
        (width - padding * 2.0).max(1.0),
        path_height,
    );
    let list_top = header_height + path_height;
    let list = Rect::new(0.0, list_top, width, (window_height - list_top).max(0.0));

    let row_count = match state.mode {
        SidebarMode::Terminals => tabs.len(),
        SidebarMode::Files => file_count,
    };
    let content_height = row_count as f32 * item_height + padding * 2.0;
    let max_scroll = (content_height - list.height).max(0.0);
    let scroll = state.scroll.clamp(0.0, max_scroll);
    let start_y = list.y + padding - scroll;

    let mut items = Vec::with_capacity(tabs.len());
    let mut file_items = Vec::with_capacity(file_count);

    let row_rect = |index: usize| {
        let y = start_y + index as f32 * item_height;
        Rect::new(
            padding,
            y,
            (width - padding * 2.0).max(1.0),
            (item_height - item_gap).max(1.0),
        )
    };

    match state.mode {
        SidebarMode::Terminals => {
            for (index, tab) in tabs.iter().enumerate() {
                let item_rect = row_rect(index);

                // Fully clipped rows are skipped so quads never leave the list.
                if item_rect.y < list.y || item_rect.bottom() > list.bottom() {
                    continue;
                }

                let close = Rect::new(
                    (item_rect.right() - close_size - 6.0 * scale).max(item_rect.x),
                    item_rect.y + (item_rect.height - close_size) / 2.0,
                    close_size,
                    close_size,
                );
                let label = Rect::new(
                    item_rect.x + 10.0 * scale,
                    item_rect.y,
                    (close.x - item_rect.x - 14.0 * scale).max(1.0),
                    item_rect.height,
                );

                items.push(SidebarItem {
                    tab: tab.id,
                    rect: item_rect,
                    label,
                    close,
                    is_active: active == Some(tab.id),
                });
            }
        }
        SidebarMode::Files => {
            for index in 0..file_count {
                let item_rect = row_rect(index);

                if item_rect.y < list.y || item_rect.bottom() > list.bottom() {
                    continue;
                }

                let label = Rect::new(
                    item_rect.x + 8.0 * scale,
                    item_rect.y,
                    (item_rect.width - 12.0 * scale).max(1.0),
                    item_rect.height,
                );
                file_items.push(FileItem {
                    row: index,
                    rect: item_rect,
                    label,
                });
            }
        }
    }

    SidebarLayout {
        rect,
        list,
        header,
        new_button,
        collapse_button,
        reveal_button: Rect::default(),
        mode_button,
        header_label,
        path_rect,
        items,
        file_items,
        content_x: width,
        max_scroll,
        item_height,
        padding,
        collapsed: false,
        mode: state.mode,
    }
}

pub fn hit_test(layout: &SidebarLayout, x: f32, y: f32) -> Option<SidebarHit> {
    if layout.rect.width <= 0.0 {
        return None;
    }

    if layout.collapsed {
        return layout
            .reveal_button
            .contains(x, y)
            .then_some(SidebarHit::ToggleSidebar);
    }

    if layout.collapse_button.contains(x, y) {
        return Some(SidebarHit::ToggleSidebar);
    }

    if layout.mode_button.contains(x, y) {
        return Some(SidebarHit::ToggleMode);
    }

    if layout.new_button.contains(x, y) {
        return Some(match layout.mode {
            SidebarMode::Terminals => SidebarHit::NewTerminal,
            SidebarMode::Files => SidebarHit::RefreshFiles,
        });
    }

    if !layout.list.contains(x, y) {
        return None;
    }

    match layout.mode {
        SidebarMode::Terminals => {
            for item in &layout.items {
                if item.close.contains(x, y) {
                    return Some(SidebarHit::Close(item.tab));
                }
                if item.rect.contains(x, y) {
                    return Some(SidebarHit::Select(item.tab));
                }
            }
        }
        SidebarMode::Files => {
            for item in &layout.file_items {
                if item.rect.contains(x, y) {
                    return Some(SidebarHit::File(item.row));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(file_count: usize) -> LayoutSpec {
        LayoutSpec {
            window_width: 1000.0,
            window_height: 600.0,
            scale: 1.0,
            width_logical: 200.0,
            file_count,
        }
    }

    #[test]
    fn hit_test_finds_items_and_close_buttons() {
        let state = Sidebar::default();
        let tabs = [TabSummary { id: TabId(1) }, TabSummary { id: TabId(2) }];
        let layout = layout(&state, &tabs, Some(TabId(1)), spec(0));

        let first = layout.items[0];
        assert_eq!(
            hit_test(&layout, first.rect.x + 4.0, first.rect.y + 4.0),
            Some(SidebarHit::Select(TabId(1)))
        );
        assert_eq!(
            hit_test(&layout, first.close.x + 2.0, first.close.y + 2.0),
            Some(SidebarHit::Close(TabId(1)))
        );
    }

    #[test]
    fn collapse_reveal_and_mode_buttons_are_clickable() {
        let tabs = [TabSummary { id: TabId(1) }];
        let visible_state = Sidebar::default();
        let visible = layout(&visible_state, &tabs, Some(TabId(1)), spec(0));

        assert_eq!(
            hit_test(
                &visible,
                visible.collapse_button.x + 2.0,
                visible.collapse_button.y + 2.0
            ),
            Some(SidebarHit::ToggleSidebar)
        );
        assert_eq!(
            hit_test(
                &visible,
                visible.mode_button.x + 2.0,
                visible.mode_button.y + 2.0
            ),
            Some(SidebarHit::ToggleMode)
        );
        assert_eq!(visible.content_x, 200.0);

        let hidden_state = Sidebar {
            visible: false,
            ..Sidebar::default()
        };
        let hidden = layout(&hidden_state, &tabs, Some(TabId(1)), spec(0));

        assert!(hidden.collapsed);
        assert!(hidden.content_x > 0.0);
        assert_eq!(
            hit_test(&hidden, hidden.rect.x + 2.0, hidden.rect.y + 2.0),
            Some(SidebarHit::ToggleSidebar)
        );
    }

    #[test]
    fn files_mode_builds_file_rows() {
        let state = Sidebar {
            mode: SidebarMode::Files,
            ..Sidebar::default()
        };
        let layout = layout(&state, &[], None, spec(3));

        assert_eq!(layout.file_items.len(), 3);
        assert!(layout.items.is_empty());
        assert!(layout.path_rect.height > 0.0);

        let row = layout.file_items[1];
        assert_eq!(
            hit_test(&layout, row.rect.x + 4.0, row.rect.y + 4.0),
            Some(SidebarHit::File(1))
        );
        assert_eq!(
            hit_test(
                &layout,
                layout.new_button.x + 2.0,
                layout.new_button.y + 2.0
            ),
            Some(SidebarHit::RefreshFiles)
        );
    }
}
