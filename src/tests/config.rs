use super::*;

#[test]
fn empty_config_uses_defaults() {
    let config = Config::from_toml("").unwrap();
    assert_eq!(config.padding, 0.0);
    assert_eq!(config.padding_left, 8.0);
    assert!(config.show_user_in_prompt);
    assert_eq!(config.scrollback_lines, 6_000);
    assert_eq!(config.hidden_scrollback_lines, 3_000);
    assert_eq!(config.scrollback_budget_mb, 0);
    assert_eq!(config.render_mode, RenderMode::Shared);
    assert!(config.completions);
    assert_eq!(config.completion_min_chars, 2);
    assert_eq!(config.theme.background, Theme::foundry().background);
    assert!(config.agents.enabled);
    assert!(config.agents.show_external);
    assert_eq!(config.agents.poll_interval_ms, 2_000);
    assert!(config.restore_session);
    assert!(config.restore_scrollback);
}

#[test]
fn parses_scrollback_render_shell_and_tokens() {
    let text = r#"
[appearance]
font_size = 18.0
sidebar_width = 250.0
option_as_meta = true
show_user_in_prompt = false
theme = "dracula"
padding_left = 12.0

[terminal]
scrollback_lines = 10000
hidden_scrollback_lines = 10000
scrollback_budget_mb = 128
completions = false
completion_min_chars = 5

[render]
mode = "isolated"

[session]
restore = false
restore_scrollback = false

[agents]
enabled = false
show_external = false
poll_interval_ms = 100

[shell]
default = "bash"

[forge.github]
token = "ghp_test"

[forge.gitlab]
token = "glpat_test"
"#;
    let config = Config::from_toml(text).unwrap();
    assert_eq!(config.font_size, 18.0);
    assert_eq!(config.sidebar_width, 250.0);
    assert!(config.option_as_meta);
    assert!(!config.show_user_in_prompt);
    assert_eq!(config.theme.background, Theme::dracula().background);
    assert_eq!(config.padding_left, 12.0);
    assert_eq!(config.scrollback_lines, 10_000);
    assert_eq!(config.hidden_scrollback_lines, 10_000);
    assert_eq!(config.scrollback_budget_mb, 128);
    assert!(!config.completions);
    assert_eq!(config.completion_min_chars, 5);
    assert_eq!(config.render_mode, RenderMode::Isolated);
    assert!(!config.restore_session);
    assert!(!config.restore_scrollback);
    assert!(!config.agents.enabled);
    assert!(!config.agents.show_external);
    assert_eq!(config.agents.poll_interval_ms, 500);
    assert_eq!(config.shell.as_deref(), Some("bash"));
    assert_eq!(config.forge_tokens.github.as_deref(), Some("ghp_test"));
    assert_eq!(config.forge_tokens.gitlab.as_deref(), Some("glpat_test"));
}

#[test]
fn clamps_and_repairs_invalid_values() {
    let text = r#"
[appearance]
font_size = 1.0

[terminal]
scrollback_lines = 100
hidden_scrollback_lines = 90000
scrollback_budget_mb = 9999

[render]
mode = "something-else"
"#;
    let config = Config::from_toml(text).unwrap();
    assert_eq!(config.font_size, 6.0);
    assert_eq!(config.scrollback_lines, 500);
    assert_eq!(config.hidden_scrollback_lines, 500);
    assert_eq!(config.scrollback_budget_mb, 4096);
    assert_eq!(config.render_mode, RenderMode::Isolated);
}

#[test]
fn hidden_scrollback_never_drops_below_the_floor() {
    let text = r#"
[terminal]
scrollback_lines = 5000
hidden_scrollback_lines = 0
"#;
    let config = Config::from_toml(text).unwrap();
    assert_eq!(config.scrollback_lines, 5_000);
    assert_eq!(config.hidden_scrollback_lines, 100);
}

#[test]
fn default_template_parses_to_the_defaults() {
    let config = Config::from_toml(&default_template()).unwrap();
    let defaults = Config::default();
    assert_eq!(config.font_size, defaults.font_size);
    assert_eq!(config.line_height, defaults.line_height);
    assert_eq!(config.padding, defaults.padding);
    assert_eq!(config.padding_left, defaults.padding_left);
    assert_eq!(config.show_user_in_prompt, defaults.show_user_in_prompt);
    assert_eq!(config.theme.background, defaults.theme.background);
    assert_eq!(config.sidebar_width, defaults.sidebar_width);
    assert_eq!(config.ui_font_size, defaults.ui_font_size);
    assert_eq!(config.option_as_meta, defaults.option_as_meta);
    assert_eq!(config.scrollback_lines, defaults.scrollback_lines);
    assert_eq!(
        config.hidden_scrollback_lines,
        defaults.hidden_scrollback_lines
    );
    assert_eq!(config.scrollback_budget_mb, defaults.scrollback_budget_mb);
    assert_eq!(config.completions, defaults.completions);
    assert_eq!(config.completion_min_chars, defaults.completion_min_chars);
    assert_eq!(config.render_mode, defaults.render_mode);
    assert_eq!(config.agents, defaults.agents);
    assert!(config.font_family.is_none());
    assert!(config.shell.is_none());
    assert!(config.forge_tokens.github.is_none());
    assert!(config.forge_tokens.gitlab.is_none());
}

#[test]
fn updates_font_size_in_place() {
    let text =
        "[appearance]\nfont_size = 14.0\nui_font_size = 12.5\n\n[terminal]\nfont_size = 99.0\n";
    let updated = update_font_size(text, 16.0);
    assert!(updated.contains("[appearance]\nfont_size = 16.0\nui_font_size = 12.5"));
    assert!(updated.contains("[terminal]\nfont_size = 99.0"));
}

#[test]
fn preserves_windows_line_endings_when_updating_font_size() {
    let updated = update_font_size("[appearance]\r\nfont_size = 14.0\r\n", 16.0);
    assert!(updated.contains("\r\nfont_size = 16.0\r\n"));
}

#[test]
fn inserts_font_size_when_missing() {
    let updated = update_font_size("[appearance]\nui_font_size = 12.5\n", 18.0);
    assert!(updated.contains("[appearance]\nui_font_size = 12.5\nfont_size = 18.0"));
    let updated = update_font_size("[terminal]\nscrollback_lines = 100\n", 18.0);
    assert!(updated.ends_with("[appearance]\nfont_size = 18.0\n"));
}

#[test]
fn writes_a_private_template_once() {
    let dir = std::env::temp_dir().join(format!("f-terminal-config-test-{}", std::process::id()));
    let path = dir.join("config.toml");
    let _ = fs::remove_dir_all(&dir);

    write_default_config(&path).expect("config template should be created");
    let text = fs::read_to_string(&path).expect("config template should be readable");
    assert!(text.contains("[terminal]"));
    assert!(Config::from_toml(&text).is_ok());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config file should be private");
    }

    // The template is only created once; an existing file is preserved.
    assert!(write_default_config(&path).is_err());
    let _ = fs::remove_dir_all(&dir);
}
