use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch_file(contents: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "f-terminal-editor-{}-{nanos}.txt",
        std::process::id()
    ));
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn edits_and_saves_text() {
    let path = scratch_file("hello\nworld\n");
    let mut editor = Editor::open(&path).unwrap();

    editor.insert_str("brave ");
    editor.insert_newline();
    editor.move_down();
    editor.move_end();
    editor.backspace();
    editor.insert_char('!');
    assert!(editor.dirty);

    editor.save().unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert_eq!(saved, "brave \nhello\nworl!\n");

    fs::remove_file(path).unwrap();
}

#[test]
fn backspace_joins_lines() {
    let path = scratch_file("one\ntwo");
    let mut editor = Editor::open(&path).unwrap();
    editor.move_down();
    editor.backspace();
    assert_eq!(editor.lines, vec!["onetwo"]);
    fs::remove_file(path).unwrap();
}

#[test]
fn scrolling_follows_the_cursor() {
    let path = scratch_file("0123456789");
    let mut editor = Editor::open(&path).unwrap();
    editor.move_end();
    editor.ensure_visible(1, 4);
    assert!(editor.scroll_col > 0);
    fs::remove_file(path).unwrap();
}
