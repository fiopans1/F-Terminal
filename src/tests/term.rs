use super::*;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::Config;

fn line_for(bytes: &[u8]) -> Option<InputLine> {
    let size = GridSize { cols: 40, lines: 4 };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut term, bytes);
    input_line_from(&term, size)
}

#[test]
fn reads_the_cursor_row_and_prompt() {
    let line = line_for(b"user@host:~$ git ch").unwrap();
    assert_eq!(line.row, 0);
    assert_eq!(line.col, 19);
    assert_eq!(&line.text[..19], "user@host:~$ git ch");
    assert_eq!(line.right_limit, 40);
}

#[test]
fn reports_a_right_prompt_limit() {
    let line = line_for(b"user@host:~$ git ch   12:30\x1b[1;20H").unwrap();
    assert_eq!(line.col, 19);
    assert_eq!(line.right_limit, 22);
}

#[test]
fn ignores_alternate_screen() {
    assert!(line_for(b"\x1b[?1049huser@host:~$ git").is_none());
}

#[test]
fn reads_tail_lines_trimmed() {
    let size = GridSize { cols: 40, lines: 6 };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut term, b"line 1\r\nline 2  \r\nline 3\r\nline 4");
    assert_eq!(
        tail_lines_from(&term, 2),
        vec!["line 3".to_owned(), "line 4".to_owned()]
    );
    assert_eq!(tail_lines_from(&term, 0), Vec::<String>::new());
}

fn search_count(term: &Term<VoidListener>, query: &str) -> usize {
    let mut regex = RegexSearch::new(&search_pattern(query)).unwrap();
    let start = Point::new(term.topmost_line(), Column(0));
    let end = Point::new(term.bottommost_line(), term.last_column());
    RegexIter::new(start, end, Direction::Right, term, &mut regex).count()
}

#[test]
fn search_uses_smart_case() {
    let size = GridSize { cols: 20, lines: 4 };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut term, b"Hello\r\nhello\r\nHELLO");

    assert_eq!(search_count(&term, "hello"), 3);
    assert_eq!(search_count(&term, "Hello"), 1);
}

#[test]
fn search_treats_the_query_as_a_literal() {
    assert_eq!(regex_literal("a.b[c]"), r"a\.b\[c\]");
    assert_eq!(search_pattern("a.b"), r"(?i)a\.b");
}

#[test]
fn detects_plain_links_and_strips_trailing_punctuation() {
    let size = GridSize { cols: 40, lines: 3 };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(&mut term, b"see https://example.com/docs).");

    let point = Point::new(Line(0), Column(8));
    let (uri, range) = link_at_in_term(&term, point).unwrap();
    assert_eq!(uri, "https://example.com/docs");
    assert!(range.contains(&point));
    assert_eq!(link_at_in_term(&term, Point::new(Line(0), Column(0))), None);
}

#[test]
fn detects_osc8_hyperlinks_across_their_cells() {
    let size = GridSize { cols: 20, lines: 2 };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
    parser.advance(
        &mut term,
        b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\",
    );

    let (uri, range) = link_at_in_term(&term, Point::new(Line(0), Column(1))).unwrap();
    assert_eq!(uri, "https://example.com");
    assert_eq!(*range.start(), Point::new(Line(0), Column(0)));
    assert_eq!(*range.end(), Point::new(Line(0), Column(3)));
}

#[test]
fn link_normalization_strips_punctuation_and_rejects_unsafe_schemes() {
    assert_eq!(
        normalize_link("https://example.com).").as_deref(),
        Some("https://example.com")
    );
    assert_eq!(
        normalize_link("www.example.com").as_deref(),
        Some("https://www.example.com")
    );
    assert_eq!(
        normalize_link("https://en.wikipedia.org/wiki/Foo_(bar)").as_deref(),
        Some("https://en.wikipedia.org/wiki/Foo_(bar)")
    );
    assert_eq!(normalize_link("file:///etc/passwd"), None);
    assert_eq!(
        normalize_link("mailto:dev@example.com").as_deref(),
        Some("mailto:dev@example.com")
    );
}
