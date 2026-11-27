use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "f-terminal-explorer-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn lists_directories_first_and_expands_them() {
    let root = scratch_dir();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("README.md"), "hello").unwrap();
    fs::write(root.join("src").join("main.rs"), "fn main() {}").unwrap();

    let mut explorer = FileExplorer::new(root.clone());
    assert_eq!(explorer.rows.len(), 2);
    assert!(explorer.rows[0].is_dir);
    assert_eq!(explorer.rows[0].name, "src");
    assert_eq!(explorer.rows[1].name, "README.md");

    explorer.toggle(&root.join("src"));
    assert_eq!(explorer.rows.len(), 3);
    assert_eq!(explorer.rows[1].name, "main.rs");
    assert_eq!(explorer.rows[1].depth, 1);

    explorer.toggle(&root.join("src"));
    assert_eq!(explorer.rows.len(), 2);

    fs::remove_dir_all(root).unwrap();
}
