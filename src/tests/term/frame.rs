use super::*;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::Config;

use crate::term::GridSize;

fn frame_for(bytes: &[u8]) -> Frame {
    let size = GridSize { cols: 20, lines: 4 };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut term, bytes);
    build(&term, &Theme::tokyo_night())
}

#[test]
fn plain_text_is_captured() {
    let frame = frame_for(b"hello");

    assert_eq!(frame.rows[0].text.trim_end(), "hello");
    assert_eq!(frame.rows[0].runs.len(), 1);
    assert_eq!(frame.rows[0].runs[0].color, Theme::tokyo_night().foreground);
}

#[test]
fn ansi_colors_are_resolved() {
    let frame = frame_for(b"\x1b[31mred\x1b[0m plain");
    let theme = Theme::tokyo_night();

    assert_eq!(frame.rows[0].text.trim_end(), "red plain");
    assert_eq!(frame.rows[0].runs[0].color, theme.ansi[1]);
    assert_eq!(frame.rows[0].runs[1].color, theme.foreground);
}

#[test]
fn backgrounds_are_merged() {
    let frame = frame_for(b"\x1b[44mAB\x1b[0mC");

    let backgrounds = &frame.rows[0].backgrounds;
    assert_eq!(backgrounds.len(), 2);
    assert_eq!(backgrounds[0].start, 0);
    assert_eq!(backgrounds[0].end, 2);
    assert_eq!(backgrounds[0].color, Theme::tokyo_night().ansi[4]);
    assert_eq!(backgrounds[1].start, 2);
    assert_eq!(backgrounds[1].end, 20);
    assert_eq!(backgrounds[1].color, Theme::tokyo_night().background);
}

#[test]
fn cursor_follows_written_text() {
    let frame = frame_for(b"hi");

    let cursor = frame.cursor.expect("cursor should be visible");
    assert_eq!(cursor.row, 0);
    assert_eq!(cursor.col, 2);
}

#[test]
fn partial_row_update_leaves_other_rows_untouched() {
    let size = GridSize { cols: 20, lines: 4 };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut term, b"hello\nworld");
    let theme = Theme::tokyo_night();
    let mut frame = build(&term, &theme);

    parser.advance(&mut term, b"\rX");
    update_rows(&term, &theme, &mut frame, &[1]);

    assert_eq!(frame.rows[0].text.trim_end(), "hello");
    assert!(frame.rows[1].text.starts_with('X'));
    assert!(frame.rows[1].text.contains("world"));
}

#[test]
fn partial_row_update_matches_a_full_rebuild() {
    let size = GridSize { cols: 20, lines: 4 };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut term, b"hello\nworld");
    let theme = Theme::tokyo_night();
    let mut frame = build(&term, &theme);
    term.reset_damage();

    parser.advance(&mut term, b"\rX\nnext");
    let damaged: Vec<usize> = match term.damage() {
        alacritty_terminal::term::TermDamage::Partial(lines) => {
            lines.map(|line| line.line).collect()
        }
        alacritty_terminal::term::TermDamage::Full => (0..size.lines).collect(),
    };
    term.reset_damage();
    update_rows(&term, &theme, &mut frame, &damaged);

    let rebuilt = build(&term, &theme);
    assert_eq!(frame.rows, rebuilt.rows);
    assert_eq!(frame.cursor, rebuilt.cursor);
    assert_eq!(frame.background, rebuilt.background);
}
