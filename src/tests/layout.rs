use super::*;
use crate::workspace::LayoutNode;

#[test]
fn splits_right_with_a_gap() {
    let tree = LayoutNode::Split(Box::new(crate::workspace::SplitNode {
        id: 1,
        direction: SplitDirection::Right,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf(PaneId(1))),
        second: Box::new(LayoutNode::Leaf(PaneId(2))),
    }));

    let layout = compute_layout(&tree, Rect::new(0.0, 0.0, 100.0, 50.0), 2.0, 4.0, 0.0, 0.0);

    assert_eq!(layout.panes.len(), 2);
    assert_eq!(layout.dividers.len(), 1);
    assert_eq!(layout.panes[0].id, PaneId(1));
    assert_eq!(layout.panes[1].id, PaneId(2));
    assert!((layout.panes[0].outer.width - 49.0).abs() < 0.01);
    assert!((layout.panes[1].outer.x - 51.0).abs() < 0.01);
    assert_eq!(layout.dividers[0].line.width, 2.0);
    assert_eq!(layout.panes[0].content.width, 41.0);
    assert_eq!(layout.panes[0].footer.height, 0.0);
}

#[test]
fn reserves_space_for_the_pane_footer() {
    let tree = LayoutNode::Leaf(PaneId(1));
    let layout = compute_layout(&tree, Rect::new(0.0, 0.0, 100.0, 80.0), 2.0, 4.0, 0.0, 12.0);

    let pane = layout.panes[0];
    assert_eq!(pane.content.height, 60.0);
    assert_eq!(pane.footer.y, 64.0);
    assert_eq!(pane.footer.height, 12.0);
}

#[test]
fn insets_only_the_left_side_of_the_grid() {
    let tree = LayoutNode::Leaf(PaneId(1));
    let layout = compute_layout(
        &tree,
        Rect::new(0.0, 0.0, 100.0, 80.0),
        2.0,
        0.0,
        10.0,
        12.0,
    );

    let pane = layout.panes[0];
    assert_eq!(pane.content.x, 10.0);
    assert_eq!(pane.content.width, 90.0);
    assert_eq!(pane.footer.x, 10.0);
    assert_eq!(pane.footer.width, 90.0);
    assert_eq!(pane.content.height, 68.0);
}

#[test]
fn splits_down_with_a_gap() {
    let tree = LayoutNode::Split(Box::new(crate::workspace::SplitNode {
        id: 1,
        direction: SplitDirection::Down,
        ratio: 0.25,
        first: Box::new(LayoutNode::Leaf(PaneId(1))),
        second: Box::new(LayoutNode::Leaf(PaneId(2))),
    }));

    let layout = compute_layout(&tree, Rect::new(0.0, 0.0, 80.0, 100.0), 2.0, 0.0, 0.0, 0.0);

    assert_eq!(layout.panes[0].outer.height, 24.5);
    assert_eq!(layout.panes[1].outer.y, 26.5);
    assert_eq!(layout.dividers[0].line.height, 2.0);
}

#[test]
fn splits_left_and_up_use_the_first_child_as_the_leading_pane() {
    for direction in [SplitDirection::Left, SplitDirection::Up] {
        let horizontal = matches!(direction, SplitDirection::Left);
        let tree = LayoutNode::Split(Box::new(crate::workspace::SplitNode {
            id: 1,
            direction,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf(PaneId(2))),
            second: Box::new(LayoutNode::Leaf(PaneId(1))),
        }));

        let layout = compute_layout(&tree, Rect::new(0.0, 0.0, 80.0, 100.0), 2.0, 0.0, 0.0, 0.0);

        assert_eq!(layout.panes[0].id, PaneId(2));
        assert_eq!(layout.panes[1].id, PaneId(1));
        if horizontal {
            assert_eq!(layout.panes[0].outer.x, 0.0);
            assert_eq!(layout.panes[1].outer.x, 41.0);
            assert_eq!(layout.dividers[0].line.width, 2.0);
        } else {
            assert_eq!(layout.panes[0].outer.y, 0.0);
            assert_eq!(layout.panes[1].outer.y, 51.0);
            assert_eq!(layout.dividers[0].line.height, 2.0);
        }
    }
}
