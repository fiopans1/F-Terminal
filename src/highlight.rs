use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Instant;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme as SyntaxTheme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use winit::event_loop::EventLoopProxy;

use crate::event::UserEvent;
use crate::term::color::Rgb;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSpan {
    pub range: Range<usize>,
    pub color: Rgb,
}

pub struct HighlightRequest {
    pub generation: u64,
    pub revision: u64,
    pub path: PathBuf,
    pub lines: Vec<String>,
}

/// Spawn the syntax highlighting worker. Keeping syntect off the UI thread is
/// what makes typing in large files stay smooth.
pub fn spawn_worker(proxy: EventLoopProxy<UserEvent>) -> Option<Sender<HighlightRequest>> {
    let (sender, receiver) = mpsc::channel::<HighlightRequest>();

    thread::Builder::new()
        .name("syntax-highlighter".into())
        .spawn(move || {
            let mut highlighter: Option<Highlighter> = None;

            while let Ok(mut request) = receiver.recv() {
                // While the UI was busy, several edits may have queued jobs.
                // Only the newest content matters.
                while let Ok(newer) = receiver.try_recv() {
                    request = newer;
                }

                let highlighter = highlighter.get_or_insert_with(Highlighter::new);
                let started = Instant::now();
                let spans = highlighter.highlight(&request.path, &request.lines);
                log::debug!(
                    "highlighted {} lines in {:?}",
                    request.lines.len(),
                    started.elapsed()
                );

                if proxy
                    .send_event(UserEvent::Highlight {
                        generation: request.generation,
                        revision: request.revision,
                        spans,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .ok()
        .map(|_| sender)
}

/// TextMate grammars via syntect, the same family of grammars VS Code uses.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme: SyntaxTheme,
}

impl Highlighter {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = [
            "base16-ocean.dark",
            "Solarized (dark)",
            "base16-eighties.dark",
        ]
        .iter()
        .find_map(|name| theme_set.themes.get(*name))
        .cloned()
        .or_else(|| theme_set.themes.values().next().cloned())
        .expect("syntect ships default themes");

        Self { syntax_set, theme }
    }

    /// Pick a grammar. JSX is highlighted with the JavaScript grammar, and
    /// TypeScript falls back to JavaScript because syntect's default set does
    /// not ship TS/TSX grammars.
    fn syntax_for(&self, path: &Path) -> &SyntaxReference {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase());
        let extension = extension.as_deref().map(|extension| match extension {
            "jsx" | "mjs" | "cjs" | "ts" | "tsx" => "js",
            "jsonc" | "json5" => "json",
            other => other,
        });

        extension
            .and_then(|extension| self.syntax_set.find_syntax_by_extension(extension))
            .or_else(|| self.syntax_set.find_syntax_for_file(path).ok().flatten())
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
    }

    /// Return colored byte ranges per line. Lines must not include newlines.
    pub fn highlight(&self, path: &Path, lines: &[String]) -> Vec<Vec<StyledSpan>> {
        let syntax = self.syntax_for(path);

        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut result = Vec::with_capacity(lines.len());

        for line in lines {
            let Ok(ranges) = highlighter.highlight_line(line, &self.syntax_set) else {
                result.push(Vec::new());
                continue;
            };

            let mut spans = Vec::with_capacity(ranges.len());
            let mut offset = 0;
            for (style, text) in ranges {
                let start = offset;
                offset += text.len();
                let color = style.foreground;
                spans.push(StyledSpan {
                    range: start..offset,
                    color: Rgb {
                        r: color.r,
                        g: color.g,
                        b: color.b,
                    },
                });
            }
            result.push(spans);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust_code() {
        let highlighter = Highlighter::new();
        let lines = vec!["fn main() {".to_owned(), "    let x = 1;".to_owned()];
        let spans = highlighter.highlight(Path::new("sample.rs"), &lines);

        assert_eq!(spans.len(), 2);
        assert!(!spans[0].is_empty());
        assert!(
            spans[0]
                .iter()
                .any(|span| span.color != Rgb { r: 0, g: 0, b: 0 })
        );
    }

    #[test]
    fn highlights_jsx_with_the_javascript_grammar() {
        let highlighter = Highlighter::new();
        let lines = vec![
            "const view = <div className=\"app\">{items.map((item) => item.name)}</div>;"
                .to_owned(),
        ];
        let javascript = highlighter.highlight(Path::new("sample.js"), &lines);

        for extension in ["jsx", "mjs", "cjs", "ts", "tsx"] {
            let path = PathBuf::from(format!("sample.{extension}"));
            assert_eq!(
                highlighter.highlight(&path, &lines),
                javascript,
                "{extension} should use the JavaScript grammar"
            );
        }
    }

    #[test]
    fn highlights_json_variants_as_json() {
        let highlighter = Highlighter::new();
        let lines = vec![r#"{"name": "f-terminal", "version": 1}"#.to_owned()];
        let json = highlighter.highlight(Path::new("sample.json"), &lines);

        for extension in ["jsonc", "json5"] {
            let path = PathBuf::from(format!("sample.{extension}"));
            assert_eq!(highlighter.highlight(&path, &lines), json);
        }
    }
}
