use super::*;

#[test]
fn resolves_theme_aliases() {
    assert_eq!(
        Theme::from_name("foundry").map(|theme| theme.background),
        Some(Theme::foundry().background)
    );
    assert_eq!(
        Theme::from_name("tokyo_night").map(|theme| theme.background),
        Some(Theme::tokyo_night().background)
    );
    assert_eq!(
        Theme::from_name("Catppuccin Mocha").map(|theme| theme.background),
        Some(Theme::catppuccin_mocha().background)
    );
    assert!(Theme::from_name("dracula").is_some());
    assert!(Theme::from_name("nord").is_some());
    assert!(Theme::from_name("gruvbox").is_some());
    assert!(Theme::from_name("one-dark").is_some());
    assert!(Theme::from_name("solarized").is_some());
    assert!(Theme::from_name("github").is_some());
    assert!(Theme::from_name("kanagawa").is_some());
    assert!(Theme::from_name("does-not-exist").is_none());
}

#[test]
fn themes_have_distinct_backgrounds_and_complete_ansi_palettes() {
    let themes = [
        Theme::foundry(),
        Theme::tokyo_night(),
        Theme::catppuccin_mocha(),
        Theme::dracula(),
        Theme::nord(),
        Theme::gruvbox_dark(),
        Theme::one_dark(),
        Theme::solarized_dark(),
        Theme::github_dark(),
        Theme::kanagawa(),
    ];
    for (index, theme) in themes.iter().enumerate() {
        assert_eq!(theme.ansi.len(), 16);
        for other in &themes[index + 1..] {
            assert_ne!(theme.background, other.background);
        }
    }
}
