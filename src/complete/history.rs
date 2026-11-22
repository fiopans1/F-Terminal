use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::PathBuf;

const MAX_ENTRIES: usize = 5_000;
const MAX_BYTES: usize = 512 * 1024;
const MAX_FILE_BYTES: u64 = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Zsh,
    Bash,
    Fish,
    PowerShell,
}

/// Compact shell history: all commands live in one arena and entries are
/// byte ranges into it.
#[derive(Debug, Default)]
pub struct History {
    arena: String,
    ranges: Vec<Range<u32>>,
    bytes: usize,
}

impl History {
    pub fn load_standard() -> Self {
        let mut commands = Vec::new();
        for (path, format) in history_files() {
            if let Some(text) = read_tail(&path, MAX_FILE_BYTES) {
                commands.extend(parse_history(&text, format));
            }
        }
        Self::from_commands(commands)
    }

    fn from_commands(commands: Vec<String>) -> Self {
        let mut seen = HashSet::new();
        let mut newest = Vec::new();
        let mut bytes = 0usize;

        for command in commands.into_iter().rev() {
            if !is_usable(&command) || !seen.insert(command.clone()) {
                continue;
            }
            let size = command.len();
            if bytes + size > MAX_BYTES && !newest.is_empty() {
                break;
            }
            bytes += size;
            newest.push(command);
            if newest.len() >= MAX_ENTRIES {
                break;
            }
        }

        let mut history = Self::default();
        for command in newest {
            history.push_back(&command);
        }
        history
    }

    pub fn add(&mut self, command: &str) {
        let command = command.trim();
        if !is_usable(command) {
            return;
        }

        if let Some(index) = self.index_of(command) {
            let removed = self.ranges.remove(index);
            self.bytes = self.bytes.saturating_sub(removed.len());
        }

        self.push_front(command);
        self.trim();

        if self.arena.len() > self.bytes * 2 && self.arena.len() > 64 * 1024 {
            self.compact();
        }
    }

    pub fn suggest(&self, prefix: &str) -> Option<&str> {
        if prefix.is_empty() {
            return None;
        }
        self.ranges.iter().find_map(|range| {
            let candidate = self.get(range);
            (candidate.len() > prefix.len() && candidate.starts_with(prefix)).then_some(candidate)
        })
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.ranges.iter().map(|range| self.get(range))
    }

    fn get(&self, range: &Range<u32>) -> &str {
        self.arena
            .get(range.start as usize..range.end as usize)
            .unwrap_or_default()
    }

    fn index_of(&self, command: &str) -> Option<usize> {
        self.ranges
            .iter()
            .position(|range| self.get(range) == command)
    }

    fn push_front(&mut self, command: &str) {
        let start = self.arena.len() as u32;
        self.arena.push_str(command);
        let end = self.arena.len() as u32;
        self.ranges.insert(0, start..end);
        self.bytes += command.len();
    }

    fn push_back(&mut self, command: &str) {
        let start = self.arena.len() as u32;
        self.arena.push_str(command);
        let end = self.arena.len() as u32;
        self.ranges.push(start..end);
        self.bytes += command.len();
    }

    fn trim(&mut self) {
        while self.ranges.len() > MAX_ENTRIES {
            if let Some(range) = self.ranges.pop() {
                self.bytes = self.bytes.saturating_sub(range.len());
            }
        }
        while self.bytes > MAX_BYTES && self.ranges.len() > 1 {
            if let Some(range) = self.ranges.pop() {
                self.bytes = self.bytes.saturating_sub(range.len());
            }
        }
        if self.arena.len() > self.bytes * 2 && self.arena.len() > 64 * 1024 {
            self.compact();
        }
    }

    fn compact(&mut self) {
        let commands: Vec<String> = self
            .ranges
            .iter()
            .map(|range| self.get(range).to_owned())
            .collect();
        self.arena.clear();
        self.ranges.clear();
        self.bytes = 0;
        for command in commands {
            self.push_back(&command);
        }
    }
}

fn is_usable(command: &str) -> bool {
    let command = command.trim();
    !command.is_empty()
        && !command.starts_with(' ')
        && !command.contains('\n')
        && !command.contains('\t')
        && command.len() <= 4096
}

fn history_files() -> Vec<(PathBuf, Format)> {
    let mut files = Vec::new();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));

    if let Some(home) = &home {
        let home = PathBuf::from(home);
        files.push((home.join(".zsh_history"), Format::Zsh));
        files.push((home.join(".bash_history"), Format::Bash));

        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));
        files.push((data.join("fish").join("fish_history"), Format::Fish));
    }

    if cfg!(windows)
        && let Some(appdata) = std::env::var_os("APPDATA")
    {
        files.push((
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("PowerShell")
                .join("PSReadLine")
                .join("ConsoleHost_history.txt"),
            Format::PowerShell,
        ));
    }

    files
        .into_iter()
        .filter(|(path, _)| path.is_file())
        .collect()
}

fn read_tail(path: &PathBuf, max_bytes: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let skip = length.saturating_sub(max_bytes);
    if skip > 0 {
        file.seek(SeekFrom::Start(skip)).ok()?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    if skip > 0
        && let Some(newline) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=newline);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_history(text: &str, format: Format) -> Vec<String> {
    match format {
        Format::Zsh => parse_zsh(text),
        Format::Bash | Format::PowerShell => parse_lines(text),
        Format::Fish => parse_fish(text),
    }
}

fn parse_zsh(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut continuing = false;
    for line in text.lines() {
        if continuing {
            continuing = line.ends_with('\\');
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let command = if line.starts_with(": ") {
            line.find(';')
                .map(|index| &line[index + 1..])
                .unwrap_or(line)
        } else {
            line
        };
        if command.ends_with('\\') {
            continuing = true;
        } else if !command.is_empty() {
            commands.push(command.to_owned());
        }
    }
    commands
}

fn parse_lines(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut continuing = false;
    for line in text.lines() {
        if continuing {
            continuing = line.ends_with('\\');
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if line.ends_with('\\') {
            continuing = true;
            continue;
        }
        let is_timestamp = line
            .strip_prefix('#')
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
        if !is_timestamp {
            commands.push(line.to_owned());
        }
    }
    commands
}

fn parse_fish(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    for line in text.lines() {
        let Some(command) = line.strip_prefix("- cmd: ") else {
            continue;
        };
        let command = unescape_fish(command);
        if !command.contains('\n') && !command.is_empty() {
            commands.push(command);
        }
    }
    commands
}

fn unescape_fish(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zsh_extended_history() {
        let text = ": 1700000000:0;git status\n: 1700000001:0;cargo build --release\n";
        assert_eq!(parse_zsh(text), vec!["git status", "cargo build --release"]);
    }

    #[test]
    fn skips_multiline_entries_instead_of_storing_fragments() {
        let text = ": 1:0;for i in 1 2; do\\\n  echo $i\\\ndone\n: 2:0;ls\n";
        assert_eq!(parse_zsh(text), vec!["ls"]);
    }

    #[test]
    fn parses_fish_escapes() {
        let text = "- cmd: echo hello\\nworld\n- cmd: git status\n";
        assert_eq!(parse_fish(text), vec!["git status"]);
    }

    #[test]
    fn ignores_bash_timestamps() {
        let text = "#1700000000\ngit status\n#1700000001\nls\n";
        assert_eq!(parse_lines(text), vec!["git status", "ls"]);
    }

    #[test]
    fn suggestions_are_newest_first() {
        let history = History::from_commands(vec![
            "git status".into(),
            "git stash".into(),
            "git status".into(),
        ]);
        assert_eq!(history.len(), 2);
        assert_eq!(history.suggest("git st"), Some("git status"));
        assert_eq!(history.suggest("git status"), None);
        assert_eq!(history.suggest("nope"), None);
    }

    #[test]
    fn adding_moves_duplicates_to_the_front() {
        let mut history = History::from_commands(vec!["one".into(), "two".into()]);
        history.add("one");
        assert_eq!(history.iter().collect::<Vec<_>>(), vec!["one", "two"]);
        assert_eq!(history.bytes(), 6);
    }
}
