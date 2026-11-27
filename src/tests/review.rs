use super::*;

#[test]
fn parses_a_unified_diff() {
    let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index 123..456 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@ fn main() {
 fn main() {
-    old();
+    new();
+    extra();
 }
";
    let detail = parse_diff(
        diff,
        ReviewSummary {
            title: "test".into(),
            subtitle: String::new(),
            kind: ReviewKind::Working,
        },
    );

    assert_eq!(detail.additions, 2);
    assert_eq!(detail.deletions, 1);
    assert_eq!(detail.files.len(), 1);
    assert_eq!(detail.files[0].path, "src/lib.rs");
    assert_eq!(detail.files[0].status, FileStatus::Modified);
    assert!(
        detail.files[0]
            .lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Hunk)
    );

    let add = detail.files[0]
        .lines
        .iter()
        .find(|line| line.kind == DiffLineKind::Add)
        .unwrap();
    assert_eq!(add.new, Some(2));

    let remove = detail.files[0]
        .lines
        .iter()
        .find(|line| line.kind == DiffLineKind::Remove)
        .unwrap();
    assert_eq!(remove.old, Some(2));
}

#[test]
fn rename_revert_paths_include_both_names() {
    let file = FileDiff {
        path: "new.rs".to_owned(),
        old_path: Some("old.rs".to_owned()),
        status: FileStatus::Renamed,
        additions: 0,
        deletions: 0,
        binary: false,
        untracked: false,
        lines: Vec::new(),
    };
    assert_eq!(file.revert_paths(), vec!["new.rs", "old.rs"]);
}

#[test]
fn marks_untracked_files_in_working_diffs() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("f-terminal-review-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]).unwrap();
    git(&dir, &["config", "user.name", "Test"]).unwrap();
    git(&dir, &["config", "user.email", "test@example.com"]).unwrap();
    std::fs::write(dir.join("tracked.txt"), "base\n").unwrap();
    git(&dir, &["add", "."]).unwrap();
    git(&dir, &["commit", "-qm", "base"]).unwrap();
    std::fs::write(dir.join("untracked.txt"), "new\n").unwrap();

    let summary = ReviewSummary {
        title: "Local changes".to_owned(),
        subtitle: String::new(),
        kind: ReviewKind::Working,
    };
    let detail = load_detail(&dir, &summary, &Default::default()).unwrap();
    let untracked = detail
        .files
        .iter()
        .find(|file| file.path == "untracked.txt")
        .expect("untracked file in diff");
    assert!(untracked.untracked);
    assert_eq!(untracked.status, FileStatus::Added);
    assert!(
        detail
            .files
            .iter()
            .filter(|file| file.path != "untracked.txt")
            .all(|file| !file.untracked)
    );

    let _ = std::fs::remove_dir_all(dir);
}
