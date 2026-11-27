use super::*;

#[test]
fn detects_github_and_gitlab_remotes() {
    for url in [
        "git@github.com:fiopans1/F-Terminal.git",
        "ssh://git@github.com/fiopans1/F-Terminal.git",
        "https://github.com/fiopans1/F-Terminal.git",
        "https://www.github.com/fiopans1/F-Terminal/",
    ] {
        let remote = detect_url(url).unwrap();
        assert_eq!(remote.kind, ForgeKind::GitHub, "{url}");
        assert_eq!(remote.path, "fiopans1/F-Terminal");
        assert_eq!(remote.api_base, "https://api.github.com");
    }

    for url in [
        "git@gitlab.com:fiopans1/F-Terminal.git",
        "https://gitlab.com/fiopans1/F-Terminal",
        "git@gitlab.com:grupo/subgrupo/repo.git",
    ] {
        let remote = detect_url(url).unwrap();
        assert_eq!(remote.kind, ForgeKind::GitLab, "{url}");
        assert_eq!(remote.api_base, "https://gitlab.com/api/v4");
    }

    let nested = detect_url("git@gitlab.com:grupo/subgrupo/repo.git").unwrap();
    assert_eq!(nested.path, "grupo/subgrupo/repo");
}

#[test]
fn detects_self_hosted_named_instances() {
    let github = detect_url("https://github.empresa.com/equipo/repo.git").unwrap();
    assert_eq!(github.kind, ForgeKind::GitHub);
    assert_eq!(github.api_base, "https://github.empresa.com/api/v3");

    let gitlab = detect_url("ssh://git@gitlab.empresa.com/equipo/sub/repo.git").unwrap();
    assert_eq!(gitlab.kind, ForgeKind::GitLab);
    assert_eq!(gitlab.api_base, "https://gitlab.empresa.com/api/v4");
    assert_eq!(gitlab.path, "equipo/sub/repo");
}

#[test]
fn rejects_unknown_hosts() {
    assert!(detect_url("https://example.com/owner/repo.git").is_err());
    assert!(detect_url("https://github.com/owner").is_err());
}

#[test]
fn debug_output_redacts_tokens() {
    let tokens = ForgeTokens {
        github: Some("ghp_secret".into()),
        gitlab: Some("glpat_secret".into()),
    };
    let debug = format!("{tokens:?}");
    assert!(!debug.contains("ghp_secret"));
    assert!(!debug.contains("glpat_secret"));
    assert!(debug.contains("[redacted]"));
}
