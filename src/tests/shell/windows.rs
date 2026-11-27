use super::*;

#[test]
fn wsl_env_translates_the_ignored_mise_config_path() {
    let env = wsl_env();
    let value = env
        .iter()
        .find(|(key, _)| key == "WSLENV")
        .map(|(_, value)| value.as_str())
        .unwrap();
    assert!(value.contains("MISE_IGNORED_CONFIG_PATHS/p"));
}
