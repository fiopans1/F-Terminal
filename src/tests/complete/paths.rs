use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("f-terminal-paths-{}-{nanos}", std::process::id()));
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(path.join("src/main.rs"), "").unwrap();
    fs::write(path.join("notes.txt"), "").unwrap();
    fs::write(path.join("My File.txt"), "").unwrap();
    fs::write(path.join(".hidden"), "").unwrap();
    path
}

#[test]
fn completes_prefixes_and_directories_first() {
    let root = scratch_dir();
    let mut cache = PathCache::default();
    let candidates = cache.complete(&root, "s", false, false);
    assert_eq!(candidates[0].append, "rc/");
    assert!(candidates[0].is_dir);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn escapes_spaces_only_when_unquoted() {
    let root = scratch_dir();
    let mut cache = PathCache::default();
    let unquoted = cache.complete(&root, "My", false, false);
    assert_eq!(unquoted[0].append, "\\ File.txt");

    cache = PathCache::default();
    let quoted = cache.complete(&root, "My", true, false);
    assert_eq!(quoted[0].append, " File.txt");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hides_dotfiles_until_requested() {
    let root = scratch_dir();
    let mut cache = PathCache::default();
    assert!(
        cache
            .complete(&root, "", false, false)
            .iter()
            .all(|c| !c.label.contains(".hidden"))
    );
    assert!(
        cache
            .complete(&root, ".", false, false)
            .iter()
            .any(|c| c.label.contains(".hidden"))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn directory_only_mode() {
    let root = scratch_dir();
    let mut cache = PathCache::default();
    let candidates = cache.complete(&root, "", false, true);
    assert!(candidates.iter().all(|candidate| candidate.is_dir));
    assert!(candidates.iter().any(|candidate| candidate.label == "src/"));
    fs::remove_dir_all(root).unwrap();
}
