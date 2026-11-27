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
        "const view = <div className=\"app\">{items.map((item) => item.name)}</div>;".to_owned(),
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
