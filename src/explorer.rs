use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_ROWS: usize = 800;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug)]
pub struct FileRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

/// Directory tree shown in the sidebar. It follows the focused terminal's
/// working directory and lets the user expand folders and open files.
pub struct FileExplorer {
    pub root: PathBuf,
    expanded: HashSet<PathBuf>,
    pub rows: Vec<FileRow>,
    pub error: Option<String>,
}

impl FileExplorer {
    pub fn new(root: PathBuf) -> Self {
        let mut explorer = Self {
            root,
            expanded: HashSet::new(),
            rows: Vec::new(),
            error: None,
        };
        explorer.rebuild();
        explorer
    }

    pub fn set_root(&mut self, root: PathBuf) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.expanded.clear();
        self.rebuild();
    }

    pub fn rebuild(&mut self) {
        self.rows.clear();
        self.error = None;

        if !self.root.is_dir() {
            self.error = Some(format!("not a directory: {}", self.root.display()));
            return;
        }

        let mut rows = Vec::new();
        walk_directory(&self.root, 0, &self.expanded, &mut rows);
        self.rows = rows;
    }

    pub fn toggle(&mut self, path: &Path) {
        if self.expanded.contains(path) {
            self.expanded.remove(path);
        } else {
            self.expanded.insert(path.to_path_buf());
        }
        self.rebuild();
    }

    pub fn display_root(&self) -> String {
        let root = self.root.display().to_string();
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home = PathBuf::from(home);
            if let Ok(stripped) = self.root.strip_prefix(&home) {
                return format!("~/{}", stripped.display());
            }
        }
        root
    }
}

fn walk_directory(
    directory: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    rows: &mut Vec<FileRow>,
) {
    if depth > MAX_DEPTH || rows.len() >= MAX_ROWS {
        return;
    }

    let Ok(read_dir) = fs::read_dir(directory) else {
        return;
    };

    let mut entries: Vec<_> = read_dir.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| {
        let is_dir = entry.path().is_dir();
        (!is_dir, entry.file_name().to_string_lossy().to_lowercase())
    });

    for entry in entries {
        if rows.len() >= MAX_ROWS {
            return;
        }

        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = path.is_dir();
        let is_expanded = is_dir && expanded.contains(&path);

        rows.push(FileRow {
            path: path.clone(),
            name,
            depth,
            is_dir,
            expanded: is_expanded,
        });

        if is_expanded {
            walk_directory(&path, depth + 1, expanded, rows);
        }
    }
}

#[cfg(test)]
mod tests {
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
}
