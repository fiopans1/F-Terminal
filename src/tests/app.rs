use super::*;
use crate::review::{DiffLineKind, FileStatus};

#[test]
fn title_bar_controls_fit_without_overlaps() {
    let height = title_bar_height();
    let layout = title_bar_layout(200.0, 900.0, height, 1.0);
    let middle = height / 2.0;

    assert!(layout.drag.width >= 0.0);
    assert!(layout.close.right() <= 1100.0);
    assert!(layout.about.right() <= 1100.0);
    assert!(layout.title.right() <= layout.about.x);
    assert_eq!(
        layout.button_at(layout.about.x + 2.0, middle),
        Some(TitleButton::About)
    );
    if cfg!(target_os = "macos") {
        assert_eq!(layout.close, Rect::default());
        assert_eq!(layout.minimize, Rect::default());
        assert_eq!(layout.maximize, Rect::default());
        assert!(layout.icon.x >= 200.0 + MACOS_TRAFFIC_LIGHTS_WIDTH);
        assert!(layout.drag.x >= layout.icon.x);
        assert!(layout.is_drag_area(layout.title.x + 2.0, middle));
    } else {
        assert!(layout.drag.width > 0.0);
        assert!(layout.minimize.right() <= layout.maximize.x);
        assert!(layout.maximize.right() <= layout.close.x);
        assert!(layout.about.right() <= layout.minimize.x);
        assert_eq!(
            layout.button_at(layout.close.x + 2.0, middle),
            Some(TitleButton::Close)
        );
        assert!(layout.is_drag_area(layout.title.x + 2.0, middle));
    }
}

#[test]
fn the_window_icon_decodes() {
    assert!(window_icon().is_some());
}

fn candidate(label: &str) -> complete::Candidate {
    complete::Candidate {
        append: label.to_owned(),
        label: label.to_owned(),
        kind: complete::Kind::Path,
        detail: "ruta".into(),
        score: 1,
    }
}

#[test]
fn restores_tabs_panes_and_session_keys() {
    let shells = ShellCatalog {
        profiles: vec![TerminalProfile {
            id: "test".into(),
            name: "Test".into(),
            short: "T".into(),
            program: PathBuf::from("sh"),
            args: Vec::new(),
            env: Vec::new(),
            cwd_mode: crate::shell::CwdMode::Native,
        }],
        default_id: "test".into(),
    };
    let cwd = std::env::temp_dir().to_string_lossy().into_owned();
    let session = SessionState {
        version: session::SESSION_VERSION,
        window: None,
        active_tab: 1,
        tabs: vec![
            SavedTab {
                title: Some("first".into()),
                custom_title: None,
                pinned: true,
                focused: 1,
                layout: SavedLayout::Split {
                    direction: SavedDirection::Right,
                    ratio: 0.5,
                    first: Box::new(SavedLayout::Leaf { index: 0 }),
                    second: Box::new(SavedLayout::Leaf { index: 1 }),
                },
                panes: vec![
                    SavedPane {
                        key: 1,
                        profile_id: "test".into(),
                        cwd: Some(cwd.clone()),
                        grid: None,
                        initial_command: None,
                    },
                    SavedPane {
                        key: 2,
                        profile_id: "missing".into(),
                        cwd: Some(cwd.clone()),
                        grid: None,
                        initial_command: None,
                    },
                ],
            },
            SavedTab {
                title: Some("second".into()),
                custom_title: Some("pinned-name".into()),
                pinned: false,
                focused: 0,
                layout: SavedLayout::Leaf { index: 0 },
                panes: vec![SavedPane {
                    key: 3,
                    profile_id: "test".into(),
                    cwd: Some(cwd),
                    grid: None,
                    initial_command: None,
                }],
            },
        ],
        collapsed_tabs: vec![0],
    };

    let mut workspace = Workspace::new();
    let restored = restore_workspace(
        &mut workspace,
        &shells,
        Some(&session),
        Path::new("."),
        false,
    );

    assert_eq!(workspace.tabs.len(), 2);
    assert_eq!(workspace.active_index(), 1);
    assert_eq!(restored.next_key, 4);
    assert_eq!(restored.pending.len(), 3);
    let mut keys: Vec<u64> = restored
        .pending
        .values()
        .map(|spawn| spawn.session_key)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec![1, 2, 3]);
    assert_eq!(
        restored
            .pending
            .values()
            .find(|spawn| spawn.session_key == 2)
            .map(|spawn| spawn.profile_id.as_str()),
        Some("test")
    );
    assert!(restored.collapsed.contains(&workspace.tabs[0].id));
    assert_eq!(workspace.tabs[0].leaves().len(), 2);
    assert!(workspace.tabs[0].pinned);
    assert_eq!(
        workspace.tabs[1].custom_title.as_deref(),
        Some("pinned-name")
    );
    assert_eq!(workspace.tabs[1].title, "pinned-name");
}

#[test]
fn path_titles_fall_back_to_the_default_label() {
    for title in [
        r"C:\Users\me",
        "c:/Users/me/project",
        r"\\server\share",
        r"\Windows\System32",
        "/home/me/project",
        "~/projects",
        "~",
    ] {
        assert_eq!(clean_pty_title(title, true), None, "{title}");
    }

    assert_eq!(
        clean_pty_title("New session", true).as_deref(),
        Some("New session")
    );
    assert_eq!(
        clean_pty_title("  vim README.md  ", true).as_deref(),
        Some("vim README.md")
    );
    assert_eq!(
        clean_pty_title("C: drive", true).as_deref(),
        Some("C: drive")
    );
    assert_eq!(clean_pty_title("", true), None);
}

#[test]
fn path_titles_are_dropped_after_hiding_the_user_prefix() {
    assert_eq!(clean_pty_title("me@host: /home/me/project", false), None);
    assert_eq!(
        clean_pty_title("me@host: opencode", false).as_deref(),
        Some("opencode")
    );
    assert_eq!(clean_pty_title("me@host /home/me", false), None);
}

#[test]
fn popup_keeps_the_selection_visible_and_inside_the_pane() {
    let candidates: Vec<_> = (0..20)
        .map(|index| candidate(&format!("item-{index}")))
        .collect();
    let content = Rect::new(10.0, 20.0, 500.0, 300.0);
    let layout = popup_layout(
        PopupGeometry {
            content,
            row: 0,
            col: 2,
            selected: 10,
            metrics: CellMetrics {
                width: 8.0,
                height: 18.0,
            },
            ui_font_size: 14.0,
            scale: 1.0,
        },
        &candidates,
    );
    assert_eq!(layout.rows, 8);
    assert_eq!(layout.first, 3);
    assert!(layout.rect.x >= content.x);
    assert!(layout.rect.right() <= content.right());
    assert!(layout.rect.y >= content.y);
    assert!(layout.rect.bottom() <= content.bottom());
}

#[test]
fn popup_moves_above_when_there_is_no_room_below() {
    let candidates = vec![candidate("one"), candidate("two")];
    let content = Rect::new(0.0, 0.0, 400.0, 100.0);
    let layout = popup_layout(
        PopupGeometry {
            content,
            row: 4,
            col: 0,
            selected: 0,
            metrics: CellMetrics {
                width: 8.0,
                height: 18.0,
            },
            ui_font_size: 14.0,
            scale: 1.0,
        },
        &candidates,
    );
    assert!(layout.rect.bottom() <= content.bottom());
    assert!(layout.rect.y >= content.y);
}

#[test]
fn dominant_background_uses_the_most_common_color() {
    let theme = Theme::tokyo_night();
    let black = Rgb { r: 0, g: 0, b: 0 };
    let row = || Row {
        backgrounds: vec![crate::term::frame::Background {
            start: 0,
            end: 2,
            color: black,
        }],
        ..Row::default()
    };
    let frame = Frame {
        rows: vec![row(), row()],
        cols: 2,
        cursor: None,
        background: theme.background,
    };
    assert_eq!(dominant_background(&frame), black);

    let plain = Frame {
        rows: vec![Row::default()],
        cols: 2,
        cursor: None,
        background: theme.background,
    };
    assert_eq!(dominant_background(&plain), theme.background);
}

#[test]
fn branch_menu_opens_above_the_footer_and_scrolls_to_the_selection() {
    let branches: Vec<BranchInfo> = (0..12)
        .map(|index| BranchInfo {
            name: format!("branch-{index}"),
            full: format!("refs/heads/branch-{index}"),
            hash: "abc123".into(),
            upstream: None,
            current: index == 0,
        })
        .collect();
    let mut menu = BranchMenu {
        pane: PaneId(1),
        cwd: PathBuf::new(),
        branches,
        selected: 0,
        loading: false,
        error: None,
        busy: None,
    };
    let anchor = Rect::new(100.0, 700.0, 80.0, 36.0);
    let outer = Rect::new(0.0, 0.0, 800.0, 800.0);
    let layout = branch_menu_layout(&menu, anchor, outer, 2.0, 25.0);

    assert_eq!(layout.visible_rows, 10);
    assert!(layout.rect.bottom() <= anchor.y);
    assert!(layout.rect.x >= outer.x && layout.rect.right() <= outer.right());
    assert_eq!(layout.offset, 0);

    menu.selected = 11;
    let layout = branch_menu_layout(&menu, anchor, outer, 2.0, 25.0);
    assert_eq!(layout.offset, 2);
}

#[test]
fn branch_menu_renders_branches_and_messages() {
    let branches: Vec<BranchInfo> = (0..3)
        .map(|index| BranchInfo {
            name: format!("branch-{index}"),
            full: format!("refs/heads/branch-{index}"),
            hash: "abc123".into(),
            upstream: None,
            current: index == 0,
        })
        .collect();
    let mut menu = BranchMenu {
        pane: PaneId(1),
        cwd: PathBuf::new(),
        branches,
        selected: 1,
        loading: false,
        error: Some("checkout failed".into()),
        busy: None,
    };
    let theme = Theme::tokyo_night();
    let anchor = Rect::new(100.0, 700.0, 80.0, 36.0);
    let outer = Rect::new(0.0, 0.0, 800.0, 800.0);
    let mut quads = Vec::new();
    let mut labels = Vec::new();

    build_branch_menu(
        &menu,
        anchor,
        outer,
        (150.0, 600.0),
        BranchMenuStyle {
            scale: 2.0,
            ui_font_size: 25.0,
            theme: &theme,
        },
        &mut quads,
        &mut labels,
    );

    assert!(!quads.is_empty());
    assert!(labels.iter().any(|label| label.text == "Branches"));
    assert!(labels.iter().any(|label| label.text == "branch-0"));
    assert!(labels.iter().any(|label| label.text == "checkout failed"));

    menu.error = None;
    menu.busy = Some("Switching to branch-1…".into());
    labels.clear();
    build_branch_menu(
        &menu,
        anchor,
        outer,
        (150.0, 600.0),
        BranchMenuStyle {
            scale: 2.0,
            ui_font_size: 25.0,
            theme: &theme,
        },
        &mut quads,
        &mut labels,
    );
    assert!(
        labels
            .iter()
            .any(|label| label.text == "Switching to branch-1…")
    );
}

#[test]
fn diff_rows_mark_the_boundary_between_files() {
    let file = |path: &str| crate::review::FileDiff {
        path: path.to_owned(),
        old_path: None,
        status: FileStatus::Modified,
        additions: 0,
        deletions: 0,
        binary: false,
        untracked: false,
        lines: Vec::new(),
    };
    let detail = ReviewDetail {
        summary: ReviewSummary {
            title: "Local changes".to_owned(),
            subtitle: String::new(),
            kind: ReviewKind::Working,
        },
        files: vec![file("a.rs"), file("b.rs")],
        additions: 0,
        deletions: 0,
    };

    let rows = build_diff_rows(&detail);
    assert_eq!(rows[0].kind, DiffRowKind::File);
    assert_eq!(rows[0].file, Some(0));
    assert_eq!(rows[1].kind, DiffRowKind::Separator);
    assert_eq!(rows[2].kind, DiffRowKind::File);
    assert_eq!(rows[2].file, Some(1));
    assert_eq!(rows[3].kind, DiffRowKind::Separator);
}

#[test]
fn diff_rows_format_files_and_line_numbers() {
    let detail = ReviewDetail {
        summary: ReviewSummary {
            title: "Local changes".to_owned(),
            subtitle: String::new(),
            kind: ReviewKind::Working,
        },
        files: vec![crate::review::FileDiff {
            path: "src/lib.rs".to_owned(),
            old_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            binary: false,
            untracked: false,
            lines: vec![
                crate::review::DiffLine {
                    kind: DiffLineKind::Context,
                    text: "fn main() {".to_owned(),
                    old: Some(1),
                    new: Some(1),
                },
                crate::review::DiffLine {
                    kind: DiffLineKind::Add,
                    text: "    let x = 1;".to_owned(),
                    old: None,
                    new: Some(2),
                },
                crate::review::DiffLine {
                    kind: DiffLineKind::Remove,
                    text: "    let y = 2;".to_owned(),
                    old: Some(2),
                    new: None,
                },
            ],
        }],
        additions: 1,
        deletions: 1,
    };

    let rows = build_diff_rows(&detail);
    assert_eq!(rows[0].kind, DiffRowKind::File);
    assert!(rows[0].text.contains("src/lib.rs"));
    assert!(rows[0].text.contains("+1 −1"));
    assert!(rows[0].text.contains("revert"));
    assert_eq!(rows[1].kind, DiffRowKind::Context);
    assert!(rows[1].text.contains("fn main()"));
    assert_eq!(rows[2].kind, DiffRowKind::Add);
    assert!(rows[2].text.contains("+ "));
    assert_eq!(rows[3].kind, DiffRowKind::Remove);
    assert!(rows[3].text.contains("- "));
    assert_eq!(rows[4].kind, DiffRowKind::Separator);

    let diff = DiffView {
        pane: PaneId(1),
        cwd: PathBuf::new(),
        repo: Some(PathBuf::new()),
        generation: 1,
        loading: false,
        error: None,
        detail: Some(detail),
        rows,
        max_cols: 80,
        scroll_row: 0,
        scroll_col: 0,
        pending_revert: None,
        reverting: false,
        status: None,
    };
    let theme = Theme::tokyo_night();
    let metrics = CellMetrics {
        width: 8.0,
        height: 16.0,
    };
    let mut quads = Vec::new();
    let frame = build_diff_frame(
        &diff,
        Rect::new(0.0, 0.0, 400.0, 160.0),
        metrics,
        &theme,
        &mut quads,
    );
    assert_eq!(frame.rows.len(), 10);
    assert!(!quads.is_empty());
}

#[test]
fn collects_and_trims_every_visible_prompt_row() {
    let frame = Frame {
        rows: vec![
            Row {
                text: "user@host ~/repo % ls".into(),
                ..Row::default()
            },
            Row {
                text: "README.md".into(),
                ..Row::default()
            },
            Row {
                text: "user@host ~/repo % ".into(),
                ..Row::default()
            },
        ],
        cols: 30,
        cursor: None,
        background: Theme::github_dark().background,
    };
    let trims = collect_prompt_trims(&frame, true);
    assert_eq!(trims.len(), 2);
    assert_eq!(trims[0].row, 0);
    assert_eq!(trims[1].row, 2);
    assert!(collect_prompt_trims(&frame, false).is_empty());
}

#[test]
fn hides_a_wrapped_prompt_in_a_narrow_pane() {
    let cols = 20;
    let prompt = "fiopans1@MacBook-Pro-de-Diego-3 ~/repo % ";
    let mut rows = Vec::new();
    for chunk in prompt.chars().collect::<Vec<_>>().chunks(cols) {
        let mut text: String = chunk.iter().collect();
        while text.chars().count() < cols {
            text.push(' ');
        }
        rows.push(Row {
            text,
            ..Row::default()
        });
    }
    let mut frame = Frame {
        rows,
        cols,
        cursor: None,
        background: Theme::tokyo_night().background,
    };

    let trims = collect_prompt_trims(&frame, true);
    assert_eq!(trims.len(), 2);
    assert_eq!((trims[0].row, trims[0].columns), (0, 20));
    assert_eq!((trims[1].row, trims[1].columns), (1, 12));

    for trim in &trims {
        trim_prompt_row(&mut frame, trim.row, trim.columns);
    }
    assert_eq!(frame.rows[0].text, " ".repeat(cols));
    assert!(frame.rows[1].text.starts_with("~/repo %"));
}

#[test]
fn trimming_prompt_user_keeps_the_rest_of_the_line() {
    let text = "user@host dir % ls";
    let width = text.chars().count();
    let mut frame = Frame {
        rows: vec![Row {
            text: text.into(),
            runs: vec![Run {
                range: 0..text.len(),
                color: Theme::tokyo_night().foreground,
            }],
            backgrounds: vec![Background {
                start: 0,
                end: width,
                color: Theme::tokyo_night().sidebar_active,
            }],
        }],
        cols: width,
        cursor: Some(Cursor {
            row: 0,
            col: width - 1,
            shape: CursorShapeKind::Beam,
        }),
        background: Theme::tokyo_night().background,
    };

    let shift = complete::line::user_host_prefix_len(text).unwrap();
    trim_prompt_row(&mut frame, 0, shift);

    assert_eq!(frame.rows[0].text.trim_end(), "dir % ls");
    assert_eq!(frame.cursor.unwrap().col, width - 1 - shift);
    assert_eq!(frame.rows[0].backgrounds[0].start, 0);
    assert_eq!(frame.rows[0].backgrounds[0].end, width - shift);
    assert_eq!(frame.rows[0].runs[0].range, 0..text.len() - shift);
}

#[test]
fn blanking_hides_cells_without_touching_the_runs() {
    let mut frame = Frame {
        rows: vec![Row {
            text: "hello world".into(),
            runs: vec![Run {
                range: 0..11,
                color: Theme::tokyo_night().foreground,
            }],
            backgrounds: Vec::new(),
        }],
        cols: 11,
        cursor: None,
        background: Theme::tokyo_night().background,
    };
    blank_region(&mut frame, 0..1, 2..5, &Theme::tokyo_night());
    assert_eq!(frame.rows[0].text, "he    world");
    assert_eq!(frame.rows[0].runs[0].range, 0..11);
}

#[test]
fn blanking_clears_wide_rows_instead_of_breaking_runs() {
    let mut frame = Frame {
        rows: vec![Row {
            text: "héllo".into(),
            runs: vec![Run {
                range: 0..6,
                color: Theme::tokyo_night().foreground,
            }],
            backgrounds: Vec::new(),
        }],
        cols: 5,
        cursor: None,
        background: Theme::tokyo_night().background,
    };
    blank_region(&mut frame, 0..1, 1..2, &Theme::tokyo_night());
    assert_eq!(frame.rows[0].text, "     ");
    assert_eq!(frame.rows[0].runs[0].range, 0..5);
}

fn pane(id: u64, cols: usize, active: bool, focused: bool) -> ScrollbackPane {
    ScrollbackPane {
        id: PaneId(id),
        cols,
        active,
        focused,
    }
}

#[test]
fn active_and_hidden_sessions_get_their_own_limits() {
    let config = Config::default();
    let panes = [
        pane(1, 100, true, true),
        pane(2, 100, true, false),
        pane(3, 100, false, false),
    ];

    let limits = plan_scrollback_limits(&panes, &config);
    assert_eq!(limits[&PaneId(1)], 6_000);
    assert_eq!(limits[&PaneId(2)], 6_000);
    assert_eq!(limits[&PaneId(3)], 3_000);
}

#[test]
fn budget_trims_hidden_panes_before_visible_and_focused() {
    let config = Config {
        scrollback_budget_mb: 24,
        ..Config::default()
    };
    let panes = [
        pane(1, 100, false, false),
        pane(2, 100, true, false),
        pane(3, 100, true, true),
    ];

    let limits = plan_scrollback_limits(&panes, &config);
    assert_eq!(
        limits[&PaneId(1)],
        1_000,
        "hidden pane should be trimmed first"
    );
    assert_eq!(
        limits[&PaneId(2)],
        1_000,
        "visible pane should be trimmed next"
    );
    assert!(
        limits[&PaneId(3)] > 1_000,
        "focused pane should retain the most history"
    );

    let total = panes.iter().fold(0usize, |total, pane| {
        total.saturating_add(estimate_scrollback_bytes(pane.cols, limits[&pane.id]))
    });
    assert!(total <= 24 * 1024 * 1024);
}

#[test]
fn budget_never_trims_below_the_minimum() {
    let config = Config {
        scrollback_budget_mb: 1,
        ..Config::default()
    };
    let panes = [pane(1, 100, true, true)];

    let limits = plan_scrollback_limits(&panes, &config);
    assert_eq!(limits[&PaneId(1)], 1_000);
}

#[test]
fn disabled_budget_leaves_limits_alone() {
    let config = Config::default();
    let panes = [pane(1, 100, false, false)];
    let limits = plan_scrollback_limits(&panes, &config);
    assert_eq!(limits[&PaneId(1)], 3_000);
}

#[test]
fn gui_launches_start_in_the_home_directory() {
    let directory = choose_launch_directory(
        false,
        Some(PathBuf::from("/")),
        Some(PathBuf::from("/home/user")),
    );
    assert_eq!(directory, PathBuf::from("/home/user"));
}

#[test]
fn terminal_launches_preserve_the_current_directory() {
    let directory = choose_launch_directory(
        true,
        Some(PathBuf::from("/work/project")),
        Some(PathBuf::from("/home/user")),
    );
    assert_eq!(directory, PathBuf::from("/work/project"));
}

#[test]
fn editor_click_maps_to_scrolled_cells() {
    let metrics = CellMetrics {
        width: 8.0,
        height: 16.0,
    };
    let text = Rect::new(10.0, 20.0, 80.0, 64.0);
    assert_eq!(
        editor_cursor_for_point(text, metrics, 0, 0, PhysicalPosition::new(18.0, 36.0)),
        Some((1, 1))
    );
    assert_eq!(
        editor_cursor_for_point(text, metrics, 5, 2, PhysicalPosition::new(18.0, 36.0)),
        Some((6, 3))
    );
    assert_eq!(
        editor_cursor_for_point(text, metrics, 0, 0, PhysicalPosition::new(5.0, 10.0)),
        None
    );
}
