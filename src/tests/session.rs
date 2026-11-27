use super::*;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::{Config as TermConfig, Term};

fn grid_for(text: &str) -> (Grid<Cell>, GridSize) {
    let size = GridSize { cols: 40, lines: 4 };
    let mut term = Term::new(TermConfig::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut term, text.as_bytes());
    (term.grid().clone(), size)
}

#[test]
fn writes_session_metadata_and_scrollback() {
    let dir = std::env::temp_dir().join(format!("f-terminal-session-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (grid, size) = grid_for("hello\r\n");
    let state = SessionState {
        version: SESSION_VERSION,
        window: None,
        active_tab: 0,
        tabs: vec![SavedTab {
            title: Some("saved".into()),
            custom_title: None,
            pinned: false,
            focused: 0,
            layout: SavedLayout::Leaf { index: 0 },
            panes: vec![SavedPane {
                key: 7,
                profile_id: "windows:pwsh".into(),
                cwd: Some(r"C:\github\api".into()),
                grid: Some(SavedGrid {
                    cols: size.cols,
                    lines: size.lines,
                }),
                initial_command: Some("opencode".to_owned()),
            }],
        }],
        collapsed_tabs: Vec::new(),
    };
    let job = SaveJob {
        state: state.clone(),
        keys: vec![7],
        scrollbacks: vec![ScrollbackSource {
            key: 7,
            grid,
            size,
            theme: Theme::foundry(),
        }],
    };

    write_job(&dir, job).unwrap();
    let text = fs::read_to_string(dir.join("session.json")).unwrap();
    let parsed: SessionState = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed, state);

    let scrollback = fs::read(dir.join("session-scrollback/0000000000000007.ansi")).unwrap();
    assert!(String::from_utf8_lossy(&scrollback).contains("hello"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn serializes_and_deserializes_a_split_layout() {
    let state = SessionState {
        version: SESSION_VERSION,
        window: Some(SavedWindow {
            width: 1200.0,
            height: 800.0,
            x: Some(20),
            y: Some(30),
            maximized: true,
        }),
        active_tab: 1,
        tabs: vec![SavedTab {
            title: Some("api".into()),
            custom_title: Some("api".into()),
            pinned: true,
            focused: 1,
            layout: SavedLayout::Split {
                direction: SavedDirection::Right,
                ratio: 0.4,
                first: Box::new(SavedLayout::Leaf { index: 0 }),
                second: Box::new(SavedLayout::Leaf { index: 1 }),
            },
            panes: vec![
                SavedPane {
                    key: 1,
                    profile_id: "windows:pwsh".into(),
                    cwd: Some(r"C:\github\api".into()),
                    grid: Some(SavedGrid {
                        cols: 120,
                        lines: 40,
                    }),
                    initial_command: None,
                },
                SavedPane {
                    key: 2,
                    profile_id: "windows:wsl:Ubuntu".into(),
                    cwd: Some(r"\\wsl$\Ubuntu\home\me".into()),
                    grid: None,
                    initial_command: None,
                },
            ],
        }],
        collapsed_tabs: vec![0],
    };

    let json = serde_json::to_string(&state).unwrap();
    let parsed: SessionState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, state);
}

#[test]
fn current_scrollback_files_are_returned_without_a_copy() {
    let dir =
        std::env::temp_dir().join(format!("f-terminal-read-scrollback-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("history.ansi");
    let mut bytes = String::from(SCROLLBACK_MAGIC);
    bytes.push_str("hello\r\n");
    fs::write(&path, &bytes).unwrap();

    let read = read_scrollback(&path).expect("current scrollback");
    assert_eq!(read, bytes.as_bytes());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn saved_grids_reject_impossible_sizes() {
    assert!(
        SavedGrid {
            cols: 80,
            lines: 24
        }
        .is_valid()
    );
    assert!(!SavedGrid { cols: 0, lines: 24 }.is_valid());
    assert!(!SavedGrid { cols: 80, lines: 0 }.is_valid());
    assert!(
        !SavedGrid {
            cols: GridSize::MAX_COLS + 1,
            lines: 24
        }
        .is_valid()
    );
    assert!(
        !SavedGrid {
            cols: 80,
            lines: GridSize::MAX_LINES + 1
        }
        .is_valid()
    );
}

#[test]
fn saved_panes_without_a_grid_still_load() {
    let json = r#"{
            "version": 1,
            "window": null,
            "active_tab": 0,
            "tabs": [{
                "title": null,
                "custom_title": null,
                "pinned": false,
                "focused": 0,
                "layout": {"kind":"leaf","index":0},
                "panes": [{"key":1,"profile_id":"unix:/bin/sh","cwd":"/tmp"}]
            }],
            "collapsed_tabs": []
        }"#;
    let state: SessionState = serde_json::from_str(json).unwrap();
    assert_eq!(state.tabs[0].panes[0].grid, None);
}

#[test]
fn scrollback_replays_text_and_colors() {
    let (grid, size) = grid_for("hello \x1b[31mred\x1b[0m\r\n");
    let ansi = ansi_from_grid(&grid, size, Theme::foundry()).unwrap();
    assert!(ansi.contains("hello"));
    assert!(ansi.contains("red"));
    assert!(ansi.contains("38;2;"));
    assert!(ansi.ends_with("\r\n"));
}

#[test]
fn scrollback_keeps_text_attributes() {
    let (grid, size) = grid_for("\x1b[1;3;4mstyled\x1b[0m\r\n");
    let ansi = ansi_from_grid(&grid, size, Theme::foundry()).unwrap();
    assert!(ansi.contains("\x1b[1m"));
    assert!(ansi.contains("\x1b[3m"));
    assert!(ansi.contains("\x1b[4m"));
}

#[test]
fn trailing_blank_cells_are_trimmed() {
    let (grid, size) = grid_for("ok\r\n");
    let ansi = ansi_from_grid(&grid, size, Theme::foundry()).unwrap();
    assert!(ansi.contains("ok"));
    assert!(!ansi.contains("ok                          "));
}

#[test]
fn scrollback_omits_the_cursor_row_and_the_viewport_tail() {
    let (grid, size) = grid_for("first\r\nsecond\r\n");
    let ansi = ansi_from_grid(&grid, size, Theme::foundry()).unwrap();
    let history = ansi.strip_prefix(SCROLLBACK_MAGIC).unwrap();

    assert!(history.contains("first"));
    assert!(history.contains("second"));
    assert_eq!(history.matches("\r\n").count(), 2);
    assert!(!history.contains("\r\n\r\n"));
}

#[test]
fn a_fresh_prompt_is_not_saved_as_history() {
    let (grid, size) = grid_for("prompt$ ");
    let ansi = ansi_from_grid(&grid, size, Theme::foundry()).unwrap();
    assert_eq!(ansi, SCROLLBACK_MAGIC);
    assert!(decode_scrollback(ansi.as_bytes()).is_none());
}

#[test]
fn legacy_scrollback_drops_padding_and_the_live_prompt() {
    let legacy = b"\x1b[0m\r\n\x1b[0mold output\r\n\x1b[0m$ prompt\r\n\x1b[0m\r\n\x1b[0m\r\n";
    assert_eq!(sanitize_legacy_scrollback(legacy), b"\x1b[0mold output\r\n");
}

#[test]
fn decode_keeps_current_scrollback_intact() {
    let mut bytes = String::from(SCROLLBACK_MAGIC);
    bytes.push_str("\x1b[0mhistory\r\n");
    assert_eq!(
        decode_scrollback(bytes.as_bytes()),
        Some(b"\x1b[0mhistory\r\n".to_vec())
    );
}

#[test]
fn restored_history_keeps_the_new_prompt_after_the_last_line() {
    let size = GridSize {
        cols: 40,
        lines: 10,
    };
    let mut source = Term::new(TermConfig::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut source, b"first\r\nsecond\r\n");
    let ansi = ansi_from_grid(source.grid(), size, Theme::foundry()).unwrap();
    let history = decode_scrollback(ansi.as_bytes()).unwrap();

    let mut restored = Term::new(TermConfig::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut restored, &history);
    parser.advance(&mut restored, b"prompt$ ");

    let row_text = |line: i32| {
        let row = &restored.grid()[Line(line)];
        row.into_iter().map(|cell| cell.c).collect::<String>()
    };
    assert_eq!(restored.grid().cursor.point.line.0, 2);
    assert!(row_text(0).starts_with("first"));
    assert!(row_text(1).starts_with("second"));
    assert!(row_text(2).starts_with("prompt$"));
}
