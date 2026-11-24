pub mod commands;
pub mod git;
pub mod history;
pub mod line;
pub mod paths;

use std::collections::HashSet;
use std::path::Path;

use crate::complete::git::RefsCache;
use crate::complete::history::History;
use crate::complete::line::{ParsedCommand, Token};

const MAX_CANDIDATES: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    History,
    Command,
    Builtin,
    Subcommand,
    Branch,
    RemoteBranch,
    Tag,
    Path,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::History => "historial",
            Self::Command => "comando",
            Self::Builtin => "builtin",
            Self::Subcommand => "subcomando",
            Self::Branch => "rama",
            Self::RemoteBranch => "rama remota",
            Self::Tag => "tag",
            Self::Path => "ruta",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Text to send to the PTY at the cursor.
    pub append: String,
    /// Text shown in the popup.
    pub label: String,
    pub kind: Kind,
    pub detail: String,
    pub score: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub history: bool,
    pub commands: bool,
    pub paths: bool,
    pub git: bool,
    pub is_fish: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            history: true,
            commands: true,
            paths: true,
            git: true,
            is_fish: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Completion {
    pub candidates: Vec<Candidate>,
    /// Characters in the whole command line.
    pub typed_line: usize,
    /// Characters in the token being completed.
    pub typed_token: usize,
}

#[derive(Default)]
pub struct Engine {
    history: History,
    commands: Option<Vec<String>>,
    paths: paths::PathCache,
    git: RefsCache,
}

impl Engine {
    pub fn new(load_history: bool) -> Self {
        let history = if load_history {
            let history = History::load_standard();
            log::info!(
                "completion history: {} commands, {} KB",
                history.len(),
                history.bytes() / 1024
            );
            history
        } else {
            History::default()
        };
        Self {
            history,
            commands: None,
            paths: paths::PathCache::default(),
            git: RefsCache::default(),
        }
    }

    pub fn set_commands(&mut self, commands: Vec<String>) {
        let bytes: usize = commands.iter().map(String::len).sum();
        log::info!(
            "completion command index: {} commands, {} KB",
            commands.len(),
            bytes / 1024
        );
        self.commands = Some(commands);
    }

    /// Install history loaded by a background worker.
    pub fn set_history(&mut self, history: History) {
        log::info!(
            "completion history: {} commands, {} KB",
            history.len(),
            history.bytes() / 1024
        );
        self.history = history;
    }

    pub fn add_history(&mut self, command: &str) {
        self.history.add(command);
    }

    pub fn completion(&mut self, command: &str, cwd: &Path, options: &Options) -> Completion {
        let parsed = line::parse_command(command);
        let context = Context::analyze(&parsed);
        let mut candidates = Vec::new();

        if options.history
            && !options.is_fish
            && !command.trim().is_empty()
            && let Some(entry) = self.history.suggest(command)
        {
            candidates.push(Candidate {
                append: entry[command.len()..].to_owned(),
                label: entry.to_owned(),
                kind: Kind::History,
                detail: Kind::History.label().to_owned(),
                score: 80,
            });
        }

        if options.commands && context.current_is_command {
            let mut seen = HashSet::new();
            for builtin in commands::BUILTINS {
                push_command(
                    &mut candidates,
                    &mut seen,
                    builtin,
                    &parsed.current_text,
                    Kind::Builtin,
                );
            }
            if let Some(index) = &self.commands {
                let prefix = parsed.current_text.as_str();
                let start = index.partition_point(|name| name.as_str() < prefix);
                for name in &index[start..] {
                    if !name.starts_with(prefix) {
                        break;
                    }
                    push_command(&mut candidates, &mut seen, name, prefix, Kind::Command);
                }
            }
        }

        if options.git {
            match context.git {
                GitContext::Subcommand => {
                    let prefix = parsed.current_text.as_str();
                    for subcommand in git::SUBCOMMANDS {
                        if !subcommand.starts_with(prefix) || *subcommand == prefix {
                            continue;
                        }
                        candidates.push(Candidate {
                            append: subcommand[prefix.len()..].to_owned(),
                            label: (*subcommand).to_owned(),
                            kind: Kind::Subcommand,
                            detail: Kind::Subcommand.label().to_owned(),
                            score: 95,
                        });
                    }
                }
                GitContext::Branches => {
                    if let Some(refs) = self.git.get(cwd) {
                        let prefix = parsed.current_text.as_str();
                        for branch in &refs.branches {
                            if !branch.starts_with(prefix) || branch == prefix {
                                continue;
                            }
                            candidates.push(Candidate {
                                append: branch[prefix.len()..].to_owned(),
                                label: branch.clone(),
                                kind: Kind::Branch,
                                detail: if refs.current.as_deref() == Some(branch.as_str()) {
                                    "rama actual".to_owned()
                                } else {
                                    Kind::Branch.label().to_owned()
                                },
                                score: if refs.current.as_deref() == Some(branch.as_str()) {
                                    96
                                } else {
                                    100
                                },
                            });
                        }
                        for branch in &refs.remotes {
                            if !branch.starts_with(prefix) || branch == prefix {
                                continue;
                            }
                            candidates.push(Candidate {
                                append: branch[prefix.len()..].to_owned(),
                                label: branch.clone(),
                                kind: Kind::RemoteBranch,
                                detail: Kind::RemoteBranch.label().to_owned(),
                                score: 90,
                            });
                        }
                        for tag in &refs.tags {
                            if !tag.starts_with(prefix) || tag == prefix {
                                continue;
                            }
                            candidates.push(Candidate {
                                append: tag[prefix.len()..].to_owned(),
                                label: tag.clone(),
                                kind: Kind::Tag,
                                detail: Kind::Tag.label().to_owned(),
                                score: 88,
                            });
                        }
                    }
                }
                GitContext::None | GitContext::NewBranch => {}
            }
        }

        if options.paths && context.path_allowed {
            for path in self.paths.complete(
                cwd,
                &parsed.current_text,
                parsed.current_quoted,
                context.dirs_only,
            ) {
                candidates.push(Candidate {
                    append: path.append,
                    label: path.label,
                    kind: Kind::Path,
                    detail: if path.is_dir {
                        "directorio".to_owned()
                    } else {
                        "archivo".to_owned()
                    },
                    score: if path.is_dir { 93 } else { 92 },
                });
            }
        }

        candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.label.cmp(&b.label)));
        let mut seen = HashSet::new();
        candidates.retain(|candidate| seen.insert(candidate.append.clone()));
        candidates.truncate(MAX_CANDIDATES);

        Completion {
            candidates,
            typed_line: command.chars().count(),
            typed_token: parsed.current_text.chars().count(),
        }
    }
}

fn push_command(
    candidates: &mut Vec<Candidate>,
    seen: &mut HashSet<String>,
    name: &str,
    prefix: &str,
    kind: Kind,
) {
    if !name.starts_with(prefix) || name == prefix || !seen.insert(name.to_owned()) {
        return;
    }
    let base: u32 = match kind {
        Kind::Builtin => 88,
        _ => 90,
    };
    let score = base.saturating_sub(name.chars().count().min(24) as u32);
    candidates.push(Candidate {
        append: name[prefix.len()..].to_owned(),
        label: name.to_owned(),
        kind,
        detail: kind.label().to_owned(),
        score,
    });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum GitContext {
    #[default]
    None,
    Subcommand,
    Branches,
    NewBranch,
}

struct Context {
    current_is_command: bool,
    git: GitContext,
    path_allowed: bool,
    dirs_only: bool,
}

impl Context {
    fn analyze(parsed: &ParsedCommand) -> Self {
        let command_index = command_index(&parsed.tokens);
        let current_is_command = parsed.current <= command_index;
        let command_name = parsed
            .tokens
            .get(command_index)
            .map(|token| basename(&token.text))
            .unwrap_or_default();
        let previous = parsed
            .current
            .checked_sub(1)
            .and_then(|index| parsed.tokens.get(index))
            .map(|token| token.text.as_str())
            .unwrap_or_default();

        let mut git = GitContext::None;
        if command_name == "git" && !current_is_command {
            let subcommand_index = (command_index + 1..parsed.tokens.len())
                .find(|index| !parsed.tokens[*index].text.starts_with('-'));
            match subcommand_index {
                Some(index) if parsed.current <= index => {
                    git = GitContext::Subcommand;
                }
                Some(index) => {
                    let subcommand = parsed.tokens[index].text.as_str();
                    if git::BRANCH_SUBCOMMANDS.contains(&subcommand) {
                        git = if git::is_new_branch_flag(previous) {
                            GitContext::NewBranch
                        } else {
                            GitContext::Branches
                        };
                    }
                }
                None => {
                    git = GitContext::Subcommand;
                }
            }
        }

        let looks_like_path = parsed.current_text.contains('/')
            || parsed.current_text.starts_with('.')
            || parsed.current_text.starts_with('~')
            || parsed.current_text.starts_with('$');
        let value_option = matches!(
            previous,
            "-m" | "--message"
                | "-b"
                | "--branch"
                | "-c"
                | "--config"
                | "-C"
                | "-e"
                | "--edit"
                | "-t"
                | "--tag"
                | "-u"
                | "--user"
                | "-o"
                | "--output"
                | "--author"
                | "--email"
                | "--remote"
        );

        let path_allowed = if current_is_command {
            looks_like_path
        } else if git == GitContext::Subcommand || git == GitContext::NewBranch {
            false
        } else {
            !parsed.current_text.starts_with('-') && !value_option
        };

        Self {
            current_is_command,
            git,
            path_allowed,
            dirs_only: matches!(command_name.as_str(), "cd" | "pushd"),
        }
    }
}

fn command_index(tokens: &[Token]) -> usize {
    let mut index = 0;
    loop {
        let Some(token) = tokens.get(index) else {
            return index;
        };
        if is_assignment(&token.text) {
            index += 1;
            continue;
        }
        if is_wrapper(&token.text) {
            index += 1;
            while tokens
                .get(index)
                .is_some_and(|token| token.text.starts_with('-'))
            {
                index += 1;
            }
            continue;
        }
        return index;
    }
}

fn is_assignment(token: &str) -> bool {
    let Some((name, _value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
}

fn is_wrapper(token: &str) -> bool {
    matches!(
        token,
        "sudo" | "doas" | "env" | "command" | "builtin" | "exec" | "time" | "nohup" | "xargs"
    )
}

fn basename(command: &str) -> String {
    command.rsplit('/').next().unwrap_or(command).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn context(command: &str) -> Context {
        Context::analyze(&line::parse_command(command))
    }

    fn scratch_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "f-terminal-complete-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(path.join(".git").join("refs").join("heads")).unwrap();
        fs::write(
            path.join(".git").join("refs").join("heads").join("main"),
            "abc\n",
        )
        .unwrap();
        fs::create_dir_all(path.join(".git").join("refs").join("heads").join("feature")).unwrap();
        fs::write(
            path.join(".git")
                .join("refs")
                .join("heads")
                .join("feature")
                .join("login"),
            "def\n",
        )
        .unwrap();
        fs::write(path.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(path.join("main.rs"), "").unwrap();
        fs::write(path.join("lib.rs"), "").unwrap();
        path
    }

    #[test]
    fn short_command_names_win_over_old_history() {
        let mut engine = Engine::default();
        engine.set_commands(vec![
            "ecdh_curve25519".to_owned(),
            "echo".to_owned(),
            "env".to_owned(),
        ]);
        engine.add_history("ecdh_curve25519 --old-command");

        let completion = engine.completion("ec", &std::env::temp_dir(), &Options::default());
        assert_eq!(completion.candidates[0].label, "echo");
        assert_eq!(completion.candidates[0].append, "ho");
    }

    #[test]
    fn detects_git_subcommand_and_branch_positions() {
        assert_eq!(context("git ch").git, GitContext::Subcommand);
        assert_eq!(context("git ").git, GitContext::Subcommand);
        assert_eq!(context("git checkout ").git, GitContext::Branches);
        assert_eq!(context("git checkout fe").git, GitContext::Branches);
        assert_eq!(context("git checkout -b new").git, GitContext::NewBranch);
        assert_eq!(context("git commit ").git, GitContext::None);
    }

    #[test]
    fn recognizes_command_position_after_wrappers_and_assignments() {
        assert!(context("car").current_is_command);
        assert!(context("FOO=bar git").current_is_command);
        assert!(context("sudo git").current_is_command);
        assert!(!context("git checkout").current_is_command);
    }

    #[test]
    fn suppresses_paths_for_options_and_messages() {
        assert!(!context("git commit -m fix").path_allowed);
        assert!(!context("cargo build --release").path_allowed);
        assert!(context("cat src/").path_allowed);
        assert!(context("cd ").path_allowed);
        assert!(context("cd ").dirs_only);
    }

    #[test]
    fn completes_history_and_git_subcommands() {
        let mut engine = Engine::default();
        engine.add_history("git checkout main");
        let completion = engine.completion("git ch", &std::env::temp_dir(), &Options::default());
        assert_eq!(completion.candidates[0].kind, Kind::Subcommand);
        assert_eq!(completion.candidates[0].append, "eckout");
        assert!(
            completion
                .candidates
                .iter()
                .any(|candidate| candidate.kind == Kind::History
                    && candidate.append == "eckout main")
        );
    }

    #[test]
    fn completes_paths_and_respects_directory_only_commands() {
        let root = scratch_dir();
        let mut engine = Engine::default();
        let completion = engine.completion("cd s", &root, &Options::default());
        assert_eq!(completion.candidates[0].kind, Kind::Path);
        assert_eq!(completion.candidates[0].append, "rc/");

        let completion = engine.completion("cat ma", &root, &Options::default());
        assert!(
            completion
                .candidates
                .iter()
                .any(|candidate| candidate.append == "in.rs")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completes_git_branches_from_refs() {
        let root = scratch_dir();
        let mut engine = Engine::default();
        let completion = engine.completion("git checkout fe", &root, &Options::default());
        assert_eq!(completion.candidates[0].kind, Kind::Branch);
        assert_eq!(completion.candidates[0].append, "ature/login");
        assert_eq!(completion.candidates[0].detail, "rama");

        let completion = engine.completion("git checkout -b ne", &root, &Options::default());
        assert!(completion.candidates.iter().all(|candidate| {
            candidate.kind != Kind::Branch && candidate.kind != Kind::RemoteBranch
        }));
        fs::remove_dir_all(root).unwrap();
    }
}
