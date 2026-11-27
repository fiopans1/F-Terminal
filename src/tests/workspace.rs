use super::*;

fn tree_two_panes() -> LayoutNode {
    LayoutNode::Split(Box::new(SplitNode {
        id: 10,
        direction: SplitDirection::Right,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf(PaneId(1))),
        second: Box::new(LayoutNode::Leaf(PaneId(2))),
    }))
}

#[test]
fn restores_a_saved_split_layout() {
    let mut workspace = Workspace::new();
    let layout = LayoutSpec::Split {
        direction: SplitDirection::Right,
        ratio: 0.4,
        first: Box::new(LayoutSpec::Leaf(0)),
        second: Box::new(LayoutSpec::Split {
            direction: SplitDirection::Down,
            ratio: 0.6,
            first: Box::new(LayoutSpec::Leaf(1)),
            second: Box::new(LayoutSpec::Leaf(2)),
        }),
    };

    let (tab_id, panes) = workspace
        .restore_terminal(&layout, 3, 1, Some("saved".to_owned()), false, None)
        .unwrap();
    assert_eq!(workspace.tabs.len(), 1);
    assert_eq!(workspace.active_tab_id(), Some(tab_id));

    let tab = workspace.active_tab().unwrap();
    assert_eq!(tab.title, "saved");
    assert_eq!(tab.focused, panes[1]);
    assert_eq!(tab.leaves().len(), 3);
    assert!(tab.leaves().contains(&panes[2]));
}

#[test]
fn rejects_a_layout_with_repeated_leaves() {
    let mut workspace = Workspace::new();
    let layout = LayoutSpec::Split {
        direction: SplitDirection::Right,
        ratio: 0.5,
        first: Box::new(LayoutSpec::Leaf(0)),
        second: Box::new(LayoutSpec::Leaf(0)),
    };
    assert!(
        workspace
            .restore_terminal(&layout, 2, 0, None, false, None)
            .is_none()
    );
}

#[test]
fn reordering_tabs_keeps_the_active_tab() {
    let mut workspace = Workspace::new();
    let (first, _) = workspace.create_terminal();
    let (second, _) = workspace.create_terminal();
    let (third, _) = workspace.create_terminal();
    workspace.select_tab(second);
    assert_eq!(workspace.active_tab_id(), Some(second));

    assert!(workspace.move_tab(first, 3));
    let order: Vec<TabId> = workspace.tabs.iter().map(|tab| tab.id).collect();
    assert_eq!(order, vec![second, third, first]);
    assert_eq!(workspace.active_tab_id(), Some(second));

    assert!(workspace.move_tab(first, 0));
    let order: Vec<TabId> = workspace.tabs.iter().map(|tab| tab.id).collect();
    assert_eq!(order, vec![first, second, third]);
    assert_eq!(workspace.active_tab_id(), Some(second));

    assert!(!workspace.move_tab(second, 1));
}

#[test]
fn splitting_a_leaf_replaces_it() {
    let mut tree = LayoutNode::Leaf(PaneId(1));
    assert!(tree.split(PaneId(1), PaneId(2), SplitDirection::Down, 7));
    assert_eq!(tree.leaf_count(), 2);
    assert!(tree.contains(PaneId(1)));
    assert!(tree.contains(PaneId(2)));
}

#[test]
fn splitting_left_or_up_puts_the_new_pane_first() {
    for direction in [SplitDirection::Left, SplitDirection::Up] {
        let mut tree = LayoutNode::Leaf(PaneId(1));
        assert!(tree.split(PaneId(1), PaneId(2), direction, 7));
        match tree {
            LayoutNode::Split(split) => {
                assert_eq!(split.direction, direction);
                assert!(matches!(&*split.first, LayoutNode::Leaf(PaneId(2))));
                assert!(matches!(&*split.second, LayoutNode::Leaf(PaneId(1))));
            }
            _ => panic!("expected a split"),
        }
    }
}

#[test]
fn removing_a_pane_collapses_the_split() {
    let (remaining, removed) = tree_two_panes().remove(PaneId(2));
    assert!(removed);
    match remaining {
        Some(LayoutNode::Leaf(id)) => assert_eq!(id, PaneId(1)),
        _ => panic!("split should collapse to the surviving leaf"),
    }
}

#[test]
fn ratio_updates_by_split_id() {
    let mut tree = tree_two_panes();
    assert!(tree.set_ratio(10, 0.75));
    match tree {
        LayoutNode::Split(split) => assert_eq!(split.ratio, 0.75),
        _ => panic!("expected a split"),
    }
}

#[test]
fn tab_lifecycle_tracks_the_active_tab() {
    let mut workspace = Workspace::new();

    let (first_tab, first_pane) = workspace.create_terminal();
    let (second_tab, _second_pane) = workspace.create_terminal();
    assert_eq!(workspace.active_tab_id(), Some(second_tab));

    assert!(workspace.select_tab(first_tab));
    let split = workspace
        .split_focused(SplitDirection::Right)
        .expect("split should be created");
    assert_eq!(workspace.active_tab().unwrap().layout.leaf_count(), 2);
    assert_eq!(workspace.focused_pane(), Some(split));

    assert_eq!(workspace.close_pane(split), CloseOutcome::PaneClosed);
    assert_eq!(workspace.active_tab().unwrap().focused, first_pane);

    assert_eq!(workspace.close_pane(first_pane), CloseOutcome::TabClosed);
    assert_eq!(workspace.active_tab_id(), Some(second_tab));

    assert_eq!(
        workspace.close_tab(second_tab),
        CloseOutcome::WorkspaceEmpty
    );
    assert!(workspace.tabs.is_empty());
}

#[test]
fn closing_a_tab_closes_all_of_its_panes() {
    let mut workspace = Workspace::new();
    let (tab, _) = workspace.create_terminal();
    workspace
        .split_focused(SplitDirection::Right)
        .expect("first split");
    workspace
        .split_focused(SplitDirection::Down)
        .expect("second split");
    assert_eq!(workspace.active_tab().unwrap().layout.leaf_count(), 3);

    assert_eq!(workspace.close_tab(tab), CloseOutcome::WorkspaceEmpty);
    assert!(workspace.tabs.is_empty());
}

#[test]
fn closing_a_background_tab_keeps_the_active_one() {
    let mut workspace = Workspace::new();
    let (first, _) = workspace.create_terminal();
    let (second, _) = workspace.create_terminal();
    let (third, _) = workspace.create_terminal();
    assert_eq!(workspace.active_tab_id(), Some(third));

    assert!(workspace.select_tab(second));
    assert_eq!(workspace.close_tab(first), CloseOutcome::TabClosed);
    assert_eq!(workspace.active_tab_id(), Some(second));

    assert_eq!(workspace.close_tab(second), CloseOutcome::TabClosed);
    assert_eq!(workspace.active_tab_id(), Some(third));
}

#[test]
fn pinning_orders_tabs_and_keeps_the_active_one() {
    let mut workspace = Workspace::new();
    let (first, _) = workspace.create_terminal();
    let (second, _) = workspace.create_terminal();
    let (third, _) = workspace.create_terminal();
    workspace.select_tab(second);

    assert!(workspace.set_pinned(third, true));
    let order: Vec<TabId> = workspace.tabs.iter().map(|tab| tab.id).collect();
    assert_eq!(order, vec![third, first, second]);
    assert_eq!(workspace.active_tab_id(), Some(second));

    assert!(workspace.set_pinned(first, true));
    let order: Vec<TabId> = workspace.tabs.iter().map(|tab| tab.id).collect();
    assert_eq!(order, vec![third, first, second]);

    assert!(workspace.set_pinned(third, false));
    let order: Vec<TabId> = workspace.tabs.iter().map(|tab| tab.id).collect();
    assert_eq!(order, vec![first, third, second]);
}

#[test]
fn drag_targets_cannot_cross_the_pinned_boundary() {
    let mut workspace = Workspace::new();
    let (first, _) = workspace.create_terminal();
    let (second, _) = workspace.create_terminal();
    let (third, _) = workspace.create_terminal();
    workspace.set_pinned(first, true);
    workspace.set_pinned(second, true);

    assert_eq!(workspace.clamp_tab_target(third, 0), 2);
    assert_eq!(workspace.clamp_tab_target(first, 3), 2);
    assert!(!workspace.move_tab(third, 0));
    let order: Vec<TabId> = workspace.tabs.iter().map(|tab| tab.id).collect();
    assert_eq!(order, vec![first, second, third]);
}

#[test]
fn custom_titles_override_the_automatic_title() {
    let mut workspace = Workspace::new();
    let (tab, _) = workspace.create_terminal();

    assert!(workspace.set_tab_custom_title(tab, Some("  deploy  ".into())));
    assert_eq!(workspace.active_tab().unwrap().title, "deploy");
    assert_eq!(
        workspace.active_tab().unwrap().custom_title.as_deref(),
        Some("deploy")
    );

    assert!(workspace.set_tab_custom_title(tab, Some("   ".into())));
    assert_eq!(workspace.active_tab().unwrap().title, "New session");
    assert_eq!(workspace.active_tab().unwrap().custom_title, None);
}
