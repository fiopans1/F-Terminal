use super::*;

fn spec(file_count: usize) -> LayoutSpec {
    LayoutSpec {
        window_width: 1000.0,
        window_height: 600.0,
        scale: 1.0,
        width_logical: 200.0,
        file_count,
        profile_count: 0,
        top_offset: 0.0,
    }
}

fn profile_spec(profile_count: usize) -> LayoutSpec {
    LayoutSpec {
        profile_count,
        ..spec(0)
    }
}

fn tabs() -> Vec<TabSummary> {
    vec![
        TabSummary {
            id: TabId(1),
            focused: PaneId(1),
            panes: vec![PaneId(1)],
        },
        TabSummary {
            id: TabId(2),
            focused: PaneId(2),
            panes: vec![PaneId(2), PaneId(3)],
        },
    ]
}

#[test]
fn builds_a_two_level_tree_and_hit_tests_it() {
    let state = Sidebar::default();
    let layout = layout(&state, &tabs(), Some(TabId(2)), spec(0));

    assert_eq!(layout.rows.len(), 4);
    let SidebarRow::Workspace(first) = layout.rows[0] else {
        panic!("expected workspace");
    };
    assert_eq!(first.tab, TabId(1));
    assert!(!first.is_active);
    assert_eq!(
        hit_test(
            &layout,
            first.rect.x + 4.0,
            first.rect.y + first.rect.height / 2.0
        ),
        Some(SidebarHit::Select(TabId(1)))
    );
    assert_eq!(
        hit_test(&layout, first.close.x + 2.0, first.close.y + 2.0),
        Some(SidebarHit::Close(TabId(1)))
    );

    let SidebarRow::Workspace(second) = layout.rows[1] else {
        panic!("expected workspace");
    };
    assert!(second.is_active);
    assert!(second.expanded);
    assert_eq!(
        hit_test(&layout, second.toggle.x + 2.0, second.toggle.y + 2.0),
        Some(SidebarHit::ToggleWorkspace(TabId(2)))
    );

    let SidebarRow::Pane(child) = layout.rows[2] else {
        panic!("expected pane");
    };
    assert_eq!(child.tab, TabId(2));
    assert_eq!(child.pane, PaneId(2));
    assert!(child.is_focused);
    assert_eq!(
        hit_test(&layout, child.label.x + 4.0, child.label.y + 4.0),
        Some(SidebarHit::SelectPane(TabId(2), PaneId(2)))
    );
    assert_eq!(
        hit_test(&layout, child.close.x + 2.0, child.close.y + 2.0),
        Some(SidebarHit::ClosePane(PaneId(2)))
    );
}

#[test]
fn collapses_workspace_children() {
    let mut state = Sidebar::default();
    state.collapsed_tabs.insert(TabId(2));
    let layout = layout(&state, &tabs(), Some(TabId(2)), spec(0));

    assert_eq!(layout.rows.len(), 2);
    let SidebarRow::Workspace(second) = layout.rows[1] else {
        panic!("expected workspace");
    };
    assert!(!second.expanded);
    assert_eq!(
        hit_test(&layout, second.toggle.x + 2.0, second.toggle.y + 2.0),
        Some(SidebarHit::ToggleWorkspace(TabId(2)))
    );
}

#[test]
fn collapse_reveal_and_mode_buttons_are_clickable() {
    let tabs = tabs();
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
fn top_offset_keeps_the_sidebar_below_window_chrome() {
    let state = Sidebar::default();
    let hidden_state = Sidebar {
        visible: false,
        ..Sidebar::default()
    };
    let spec = LayoutSpec {
        top_offset: 28.0,
        ..spec(0)
    };

    let visible = layout(&state, &tabs(), Some(TabId(1)), spec);
    assert_eq!(visible.rect.y, 28.0);
    assert_eq!(visible.header.y, 28.0);
    assert_eq!(visible.list.y, 64.0);
    assert!(visible.collapse_button.y >= visible.header.y);
    assert_eq!(
        hit_test(
            &visible,
            visible.collapse_button.x + 2.0,
            visible.collapse_button.y + 2.0
        ),
        Some(SidebarHit::ToggleSidebar)
    );

    let hidden = layout(&hidden_state, &tabs(), Some(TabId(1)), spec);
    assert_eq!(hidden.rect.y, 28.0);
    assert_eq!(hidden.reveal_button.y, 28.0);
    assert_eq!(
        hit_test(&hidden, hidden.rect.x + 2.0, hidden.rect.y + 2.0),
        Some(SidebarHit::ToggleSidebar)
    );
}

#[test]
fn profile_menu_builds_shell_rows() {
    let state = Sidebar {
        profile_menu: true,
        ..Sidebar::default()
    };
    let layout = layout(&state, &[], None, profile_spec(3));

    assert!(layout.profile_menu);
    assert_eq!(layout.profile_items.len(), 3);
    assert!(layout.rows.is_empty());

    let row = layout.profile_items[1];
    assert_eq!(
        hit_test(&layout, row.rect.x + 4.0, row.rect.y + 4.0),
        Some(SidebarHit::Profile(1))
    );
    assert_eq!(
        hit_test(
            &layout,
            layout.profile_button.x + 2.0,
            layout.profile_button.y + 2.0
        ),
        Some(SidebarHit::ToggleProfiles)
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
    assert!(layout.rows.is_empty());
    assert!(layout.path_rect.height > 0.0);
    assert_eq!(layout.profile_button, Rect::default());

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
    assert_eq!(
        hit_test(
            &layout,
            layout.back_button.x + 2.0,
            layout.back_button.y + 2.0
        ),
        Some(SidebarHit::ExplorerUp)
    );
}
