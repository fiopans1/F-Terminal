use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use winit::event_loop::EventLoopProxy;

use crate::event::UserEvent;
use crate::git_status::GitStatus;
use crate::pty::Session;
use crate::shell::TerminalProfile;
use crate::term::{EventProxy, GridSize, Terminal};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    /// Side by side (left/right).
    Right,
    /// Stacked (top/bottom).
    Down,
}

#[derive(Clone, Debug)]
pub struct SplitNode {
    pub id: u64,
    pub direction: SplitDirection,
    pub ratio: f32,
    pub first: Box<LayoutNode>,
    pub second: Box<LayoutNode>,
}

#[derive(Clone, Debug)]
pub enum LayoutNode {
    Leaf(PaneId),
    Split(Box<SplitNode>),
}

/// Runtime-independent layout used to rebuild a saved tab. Leaves are indices
/// into the pane list instead of `PaneId`s, which only live for one process.
#[derive(Clone, Debug)]
pub enum LayoutSpec {
    Leaf(usize),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<LayoutSpec>,
        second: Box<LayoutSpec>,
    },
}

fn build_layout(spec: &LayoutSpec, panes: &[PaneId], next_id: &mut u64) -> Option<LayoutNode> {
    match spec {
        LayoutSpec::Leaf(index) => panes.get(*index).copied().map(LayoutNode::Leaf),
        LayoutSpec::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let first = build_layout(first, panes, next_id)?;
            let second = build_layout(second, panes, next_id)?;
            let id = *next_id;
            *next_id += 1;
            Some(LayoutNode::Split(Box::new(SplitNode {
                id,
                direction: *direction,
                ratio: ratio.clamp(0.1, 0.9),
                first: Box::new(first),
                second: Box::new(second),
            })))
        }
    }
}

impl LayoutNode {
    #[cfg(test)]
    pub fn leaf_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Split(split) => split.first.leaf_count() + split.second.leaf_count(),
        }
    }

    pub fn leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Self::Leaf(id) => out.push(*id),
            Self::Split(split) => {
                split.first.leaves(out);
                split.second.leaves(out);
            }
        }
    }

    pub fn first_leaf(&self) -> PaneId {
        match self {
            Self::Leaf(id) => *id,
            Self::Split(split) => split.first.first_leaf(),
        }
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        match self {
            Self::Leaf(id) => *id == pane,
            Self::Split(split) => split.first.contains(pane) || split.second.contains(pane),
        }
    }

    /// Replace `target` with a split containing `target` and `new_pane`.
    pub fn split(
        &mut self,
        target: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
        split_id: u64,
    ) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                let old = Self::Leaf(target);
                *self = Self::Split(Box::new(SplitNode {
                    id: split_id,
                    direction,
                    ratio: 0.5,
                    first: Box::new(old),
                    second: Box::new(Self::Leaf(new_pane)),
                }));
                true
            }
            Self::Leaf(_) => false,
            Self::Split(split) => {
                split.first.split(target, new_pane, direction, split_id)
                    || split.second.split(target, new_pane, direction, split_id)
            }
        }
    }

    pub fn set_ratio(&mut self, split_id: u64, ratio: f32) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split(split) => {
                if split.id == split_id {
                    split.ratio = ratio.clamp(0.1, 0.9);
                    true
                } else {
                    split.first.set_ratio(split_id, ratio)
                        || split.second.set_ratio(split_id, ratio)
                }
            }
        }
    }

    /// Remove a leaf, collapsing its parent split.
    ///
    /// Returns the remaining tree (if any) and whether the target was found.
    pub fn remove(self, target: PaneId) -> (Option<Self>, bool) {
        match self {
            Self::Leaf(id) if id == target => (None, true),
            Self::Leaf(id) => (Some(Self::Leaf(id)), false),
            Self::Split(split) => {
                let SplitNode {
                    id,
                    direction,
                    ratio,
                    first,
                    second,
                } = *split;

                let (first, first_removed) = first.remove(target);
                let (second, second_removed) = second.remove(target);

                match (first, second) {
                    (Some(first), Some(second)) => (
                        Some(Self::Split(Box::new(SplitNode {
                            id,
                            direction,
                            ratio,
                            first: Box::new(first),
                            second: Box::new(second),
                        }))),
                        first_removed || second_removed,
                    ),
                    (Some(remaining), None) => (Some(remaining), true),
                    (None, Some(remaining)) => (Some(remaining), true),
                    (None, None) => (None, true),
                }
            }
        }
    }
}

/// Session fields needed when a pane is spawned after a restore.
#[derive(Clone, Copy)]
pub struct PaneRestore<'a> {
    pub session_key: u64,
    pub scrollback: Option<&'a [u8]>,
}

/// One PTY-backed terminal inside a tab.
pub struct Pane {
    pub id: PaneId,
    /// Stable key used by the session snapshot and its scrollback file.
    pub session_key: u64,
    pub terminal: Terminal,
    pub session: Session,
    pub grid: GridSize,
    pub title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub git_status: Option<GitStatus>,
    pub profile_id: String,
    /// Monotonic counter used to avoid re-exporting unchanged grids.
    pub content_revision: u64,
    /// Completion is only considered after the user has edited the line.
    pub suggestion_armed: bool,
    /// Last time the PTY produced output, used by the agent fallback state.
    pub last_output: Instant,
    /// Revision of the `OSC 7501` records already consumed by the app.
    pub program_status_revision: u64,
    /// The shell reports its directory through `OSC 7`/`OSC 9;9`, which is
    /// more reliable than the process cwd on Windows PowerShell.
    pub cwd_from_shell: bool,
}

impl Pane {
    pub fn new(
        id: PaneId,
        grid: GridSize,
        proxy: EventLoopProxy<UserEvent>,
        profile: &TerminalProfile,
        cwd: PathBuf,
        restore: PaneRestore<'_>,
        scrollback_lines: usize,
    ) -> Result<Self> {
        let mut terminal =
            Terminal::new(grid, EventProxy::new(id, proxy.clone()), scrollback_lines);
        let mut restored = false;
        if let Some(bytes) = restore.scrollback {
            terminal.restore_scrollback(bytes);
            restored = true;
        }
        let session = Session::spawn(id, grid.lines, grid.cols, proxy, profile, &cwd)
            .with_context(|| format!("failed to spawn shell for pane {}", id.0))?;

        Ok(Self {
            id,
            session_key: restore.session_key,
            terminal,
            session,
            grid,
            title: None,
            cwd: Some(cwd),
            git_status: None,
            profile_id: profile.id.clone(),
            content_revision: u64::from(restored),
            suggestion_armed: false,
            last_output: Instant::now(),
            program_status_revision: 0,
            cwd_from_shell: false,
        })
    }

    pub fn resize(&mut self, grid: GridSize) -> bool {
        if grid == self.grid {
            return false;
        }
        self.terminal.resize(grid);
        self.session.resize(grid.lines, grid.cols);
        self.grid = grid;
        self.content_revision = self.content_revision.wrapping_add(1);
        true
    }

    pub fn apply_scrollback_limit(&mut self, lines: usize) {
        self.terminal.set_scrollback_limit(lines);
    }
}

/// A sidebar item: a tab that can contain one or more split panes.
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub default_title: String,
    pub layout: LayoutNode,
    pub focused: PaneId,
    pub panes: HashMap<PaneId, Pane>,
}

impl Tab {
    pub fn leaves(&self) -> Vec<PaneId> {
        let mut leaves = Vec::new();
        self.layout.leaves(&mut leaves);
        leaves
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseOutcome {
    PaneClosed,
    TabClosed,
    WorkspaceEmpty,
    NotFound,
}

pub struct Workspace {
    pub tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Add a new sidebar terminal with a single pane. The pane's PTY is
    /// created later by the layout pass, once its size is known.
    pub fn create_terminal(&mut self) -> (TabId, PaneId) {
        let tab_id = TabId(self.alloc_id());
        let pane_id = PaneId(self.alloc_id());

        self.tabs.push(Tab {
            id: tab_id,
            title: "Nueva sesión".to_owned(),
            default_title: "Nueva sesión".to_owned(),
            layout: LayoutNode::Leaf(pane_id),
            focused: pane_id,
            panes: HashMap::new(),
        });
        self.active = self.tabs.len() - 1;

        (tab_id, pane_id)
    }

    pub fn insert_pane(&mut self, pane: Pane) -> bool {
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.layout.contains(pane.id))
        {
            let focused = tab.focused == pane.id;
            let title = pane.title.clone();
            tab.panes.insert(pane.id, pane);
            if focused {
                tab.title = title
                    .filter(|title| !title.is_empty())
                    .unwrap_or_else(|| tab.default_title.clone());
            }
            true
        } else {
            false
        }
    }

    /// Rebuild a tab from a saved layout. Panes are created later by the
    /// layout pass, so `panes` only needs to be filled in order of the leaf
    /// indices in `layout`.
    pub fn restore_terminal(
        &mut self,
        layout: &LayoutSpec,
        pane_count: usize,
        focused: usize,
        title: Option<String>,
    ) -> Option<(TabId, Vec<PaneId>)> {
        if pane_count == 0 || focused >= pane_count {
            return None;
        }

        let panes: Vec<PaneId> = (0..pane_count).map(|_| PaneId(self.alloc_id())).collect();
        let mut next_split_id = self.next_id;
        let layout = build_layout(layout, &panes, &mut next_split_id)?;
        self.next_id = next_split_id;

        let mut leaves = Vec::new();
        layout.leaves(&mut leaves);
        if leaves.len() != pane_count {
            return None;
        }
        let unique: HashSet<PaneId> = leaves.iter().copied().collect();
        if unique.len() != pane_count {
            return None;
        }

        let tab_id = TabId(self.alloc_id());
        let title = title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Nueva sesión".to_owned());
        self.tabs.push(Tab {
            id: tab_id,
            title: title.clone(),
            default_title: title,
            layout,
            focused: panes[focused],
            panes: HashMap::new(),
        });
        self.active = self.tabs.len() - 1;

        Some((tab_id, panes))
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Move a tab to a new position, keeping the currently active tab active.
    pub fn move_tab(&mut self, id: TabId, target: usize) -> bool {
        let Some(from) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if self.tabs.len() < 2 {
            return false;
        }

        // `target` is an insertion index in the current list. Removing the tab
        // shifts everything after it one slot to the left.
        let mut target = target.min(self.tabs.len());
        if from < target {
            target -= 1;
        }
        if from == target {
            return false;
        }

        let active = self.tabs.get(self.active).map(|tab| tab.id);
        let tab = self.tabs.remove(from);
        self.tabs.insert(target.min(self.tabs.len()), tab);

        if let Some(active) = active {
            self.active = self
                .tabs
                .iter()
                .position(|tab| tab.id == active)
                .unwrap_or(0);
        }
        true
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.tabs.iter().find_map(|tab| tab.panes.get(&id))
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.tabs.iter_mut().find_map(|tab| tab.panes.get_mut(&id))
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    pub fn active_tab_id(&self) -> Option<TabId> {
        self.tabs.get(self.active).map(|tab| tab.id)
    }

    pub fn focused_pane(&self) -> Option<PaneId> {
        self.active_tab().map(|tab| tab.focused)
    }

    pub fn is_pane_visible(&self, id: PaneId) -> bool {
        self.active_tab().is_some_and(|tab| tab.layout.contains(id))
    }

    pub fn select_tab(&mut self, id: TabId) -> bool {
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
            self.active = index;
            true
        } else {
            false
        }
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    pub fn next_terminal(&mut self, delta: i32) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        let len = self.tabs.len() as i32;
        self.active = (self.active as i32 + delta).rem_euclid(len) as usize;
        true
    }

    /// Focus a pane, switching to its tab if necessary.
    pub fn focus_pane(&mut self, id: PaneId) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.layout.contains(id)) else {
            return false;
        };

        self.active = index;
        let tab = &mut self.tabs[index];
        if !tab.layout.contains(id) {
            return false;
        }
        tab.focused = id;
        Self::refresh_title(tab);
        true
    }

    pub fn split_focused(&mut self, direction: SplitDirection) -> Option<PaneId> {
        let index = self.active;
        let target = self.tabs.get(index)?.focused;
        let new_pane = PaneId(self.alloc_id());
        let split_id = self.alloc_id();

        let tab = &mut self.tabs[index];
        if !tab.layout.split(target, new_pane, direction, split_id) {
            return None;
        }
        tab.focused = new_pane;
        Some(new_pane)
    }

    pub fn set_split_ratio(&mut self, split_id: u64, ratio: f32) -> bool {
        self.active_tab_mut()
            .is_some_and(|tab| tab.layout.set_ratio(split_id, ratio))
    }

    pub fn cycle_pane(&mut self, forward: bool) -> bool {
        let Some(tab) = self.active_tab_mut() else {
            return false;
        };
        let leaves = tab.leaves();
        if leaves.len() < 2 {
            return false;
        }
        let current = leaves.iter().position(|id| *id == tab.focused).unwrap_or(0);
        let next = if forward {
            (current + 1) % leaves.len()
        } else {
            (current + leaves.len() - 1) % leaves.len()
        };
        tab.focused = leaves[next];
        Self::refresh_title(tab);
        true
    }

    pub fn set_pane_title(&mut self, id: PaneId, title: Option<String>) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.layout.contains(id)) else {
            return false;
        };

        if let Some(pane) = tab.panes.get_mut(&id) {
            pane.title = title;
        }

        if tab.focused == id {
            Self::refresh_title(tab);
        }
        true
    }

    fn refresh_title(tab: &mut Tab) {
        tab.title = tab
            .panes
            .get(&tab.focused)
            .and_then(|pane| pane.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| tab.default_title.clone());
    }

    pub fn close_pane(&mut self, id: PaneId) -> CloseOutcome {
        let Some(index) = self.tabs.iter().position(|tab| tab.layout.contains(id)) else {
            return CloseOutcome::NotFound;
        };

        let old = std::mem::replace(
            &mut self.tabs[index].layout,
            LayoutNode::Leaf(PaneId(u64::MAX)),
        );
        let (new_layout, removed) = old.remove(id);
        if !removed {
            return CloseOutcome::NotFound;
        }

        self.tabs[index].panes.remove(&id);

        match new_layout {
            Some(layout) => {
                let tab = &mut self.tabs[index];
                tab.layout = layout;
                if tab.focused == id {
                    tab.focused = tab.layout.first_leaf();
                }
                Self::refresh_title(tab);
                CloseOutcome::PaneClosed
            }
            None => {
                self.tabs.remove(index);
                if self.tabs.is_empty() {
                    self.active = 0;
                    CloseOutcome::WorkspaceEmpty
                } else {
                    self.fix_active_after_removal(index);
                    CloseOutcome::TabClosed
                }
            }
        }
    }

    pub fn close_tab(&mut self, id: TabId) -> CloseOutcome {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return CloseOutcome::NotFound;
        };

        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active = 0;
            return CloseOutcome::WorkspaceEmpty;
        }

        self.fix_active_after_removal(index);
        CloseOutcome::TabClosed
    }

    fn fix_active_after_removal(&mut self, removed_index: usize) {
        if removed_index < self.active {
            self.active -= 1;
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
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
            .restore_terminal(&layout, 3, 1, Some("saved".to_owned()))
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
        assert!(workspace.restore_terminal(&layout, 2, 0, None).is_none());
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
}
