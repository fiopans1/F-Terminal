use super::*;

#[test]
fn menu_ids_map_to_actions() {
    assert_eq!(
        Action::from_menu_id("open-explorer"),
        Some(Action::Explorer)
    );
    assert_eq!(Action::from_menu_id("open-code"), Some(Action::OpenCode));
    assert_eq!(
        Action::from_menu_id("code-review"),
        Some(Action::CodeReview)
    );
    assert_eq!(
        Action::from_menu_id("git-manager"),
        Some(Action::GitManager)
    );
    assert_eq!(Action::from_menu_id("agent-mode"), Some(Action::AgentMode));
    assert_eq!(
        Action::from_menu_id("open-settings"),
        Some(Action::Settings)
    );
    assert_eq!(Action::from_menu_id("about"), Some(Action::About));
    assert_eq!(Action::from_menu_id("quit"), Some(Action::Quit));
    assert_eq!(Action::from_menu_id("unknown"), None);
}
