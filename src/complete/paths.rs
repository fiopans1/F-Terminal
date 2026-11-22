use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(2);
const MAX_DIR_ENTRIES: usize = 2_000;
const MAX_CANDIDATES: usize = 100;

#[derive(Clone, Debug)]
struct DirEntry {
    name: String,
    is_dir: bool,
}

#[derive(Debug)]
struct CachedDir {
    path: PathBuf,
    loaded: Instant,
    entries: Vec<DirEntry>,
}

/// Small, single-directory cache. Completion never indexes the filesystem;
/// it only lists the directory containing the token being completed.
#[derive(Debug, Default)]
pub struct PathCache {
    cached: Option<CachedDir>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathCandidate {
    pub append: String,
    pub label: String,
    pub is_dir: bool,
}

impl PathCache {
    pub fn complete(
        &mut self,
        cwd: &Path,
        token: &str,
        quoted: bool,
        dirs_only: bool,
    ) -> Vec<PathCandidate> {
        let Some((directory, display_dir, name_prefix)) = resolve_token(cwd, token) else {
            return Vec::new();
        };
        let entries = self.entries(&directory);
        let mut candidates = Vec::new();

        for entry in entries {
            if dirs_only && !entry.is_dir {
                continue;
            }
            if entry.name.starts_with('.') && !name_prefix.starts_with('.') {
                continue;
            }
            if !entry.name.starts_with(name_prefix) {
                continue;
            }

            let suffix = &entry.name[name_prefix.len()..];
            let mut append = if quoted {
                suffix.to_owned()
            } else {
                match escape(suffix) {
                    Some(escaped) => escaped,
                    None => continue,
                }
            };
            let separator = if display_dir.contains('\\') {
                '\\'
            } else {
                '/'
            };
            if entry.is_dir {
                append.push(separator);
            }
            if append.is_empty() && !entry.is_dir {
                continue;
            }

            let mut label = format!("{display_dir}{}", entry.name);
            if entry.is_dir {
                label.push(separator);
            }
            candidates.push(PathCandidate {
                append,
                label,
                is_dir: entry.is_dir,
            });

            if candidates.len() >= MAX_CANDIDATES {
                break;
            }
        }

        candidates
    }

    fn entries(&mut self, directory: &Path) -> &[DirEntry] {
        let fresh = self
            .cached
            .as_ref()
            .is_some_and(|cached| cached.path == directory && cached.loaded.elapsed() < CACHE_TTL);
        if !fresh {
            self.cached = Some(CachedDir {
                path: directory.to_path_buf(),
                loaded: Instant::now(),
                entries: read_directory(directory),
            });
        }
        self.cached
            .as_ref()
            .map(|cached| cached.entries.as_slice())
            .unwrap_or_default()
    }
}

fn resolve_token<'a>(cwd: &Path, token: &'a str) -> Option<(PathBuf, &'a str, &'a str)> {
    let (display_dir, name_prefix) = match token.rfind(['/', '\\']) {
        Some(index) => (&token[..=index], &token[index + 1..]),
        None => ("", token),
    };

    if token.starts_with("~/") || token.starts_with("~\\") {
        let home = home_dir()?;
        let rest = &display_dir[2..];
        return Some((home.join(rest), display_dir, name_prefix));
    }

    if display_dir.is_empty() {
        return Some((cwd.to_path_buf(), display_dir, name_prefix));
    }

    if Path::new(display_dir).is_absolute() {
        return Some((PathBuf::from(display_dir), display_dir, name_prefix));
    }

    if let Some(rest) = display_dir.strip_prefix('$') {
        let (variable, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let value = std::env::var_os(variable)?;
        let mut path = PathBuf::from(value);
        if !tail.is_empty() {
            path.push(tail);
        }
        return Some((path, display_dir, name_prefix));
    }

    Some((cwd.join(display_dir), display_dir, name_prefix))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn read_directory(directory: &Path) -> Vec<DirEntry> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut result: Vec<DirEntry> = entries
        .flatten()
        .take(MAX_DIR_ENTRIES)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            if name.is_empty() || name.chars().any(char::is_control) {
                return None;
            }
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
                || (entry.file_type().is_ok_and(|kind| kind.is_symlink())
                    && entry
                        .path()
                        .metadata()
                        .is_ok_and(|metadata| metadata.is_dir()));
            Some(DirEntry { name, is_dir })
        })
        .collect();

    result.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    result
}

fn escape(value: &str) -> Option<String> {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            return None;
        }
        if " \t\\'\"$`&;|<>()*?[]#!".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    Some(escaped)
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
}
