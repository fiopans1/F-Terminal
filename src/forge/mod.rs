pub mod github;
pub mod gitlab;
pub mod http;

use crate::process::git_command;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Optional API tokens loaded from the unified config file.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ForgeTokens {
    pub github: Option<String>,
    pub gitlab: Option<String>,
}

impl std::fmt::Debug for ForgeTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForgeTokens")
            .field("github", &self.github.as_ref().map(|_| "[redacted]"))
            .field("gitlab", &self.gitlab.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

/// Forge supported by the Code Review window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgeKind {
    GitHub,
    GitLab,
}

impl ForgeKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
        }
    }

    pub fn tab_label(self) -> &'static str {
        match self {
            Self::GitHub => "PULL REQUESTS",
            Self::GitLab => "MERGE REQUESTS",
        }
    }

    pub fn search_placeholder(self) -> &'static str {
        match self {
            Self::GitHub => "Buscar PRs…",
            Self::GitLab => "Buscar MRs…",
        }
    }

    pub fn empty_message(self) -> &'static str {
        match self {
            Self::GitHub => "No open pull requests.",
            Self::GitLab => "No open merge requests.",
        }
    }

    pub fn changes_name(self) -> &'static str {
        match self {
            Self::GitHub => "pull requests",
            Self::GitLab => "merge requests",
        }
    }
}

/// Remote repository information used to build forge API URLs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteInfo {
    pub kind: ForgeKind,
    pub host: String,
    pub path: String,
    pub api_base: String,
}

/// Normalized pull request / merge request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteChange {
    pub id: u64,
    pub title: String,
    pub author: String,
    pub source_branch: String,
    pub target_branch: String,
    pub draft: bool,
    pub web_url: String,
    pub updated_at: String,
    pub forge: ForgeKind,
}

impl RemoteChange {
    pub fn id_label(&self) -> String {
        match self.forge {
            ForgeKind::GitHub => format!("#{}", self.id),
            ForgeKind::GitLab => format!("!{}", self.id),
        }
    }

    pub fn list_title(&self) -> String {
        format!("{} {}", self.id_label(), self.title)
    }
}

/// Detect the forge from the repository's configured remote.
pub fn detect(repo: &Path) -> Result<RemoteInfo> {
    let url = remote_url(repo)?;
    detect_url(&url)
}

/// Load all open pull requests or merge requests from the detected forge.
pub fn load_open_changes(repo: &Path, tokens: &ForgeTokens) -> Result<Vec<RemoteChange>> {
    let remote = detect(repo)?;
    match remote.kind {
        ForgeKind::GitHub => github::list_open(&remote, tokens),
        ForgeKind::GitLab => gitlab::list_open(&remote, tokens),
    }
}

/// Load the unified diff for one remote change.
pub fn load_change_diff(
    repo: &Path,
    kind: ForgeKind,
    id: u64,
    tokens: &ForgeTokens,
) -> Result<String> {
    let remote = detect(repo)?;
    if remote.kind != kind {
        bail!(
            "the repository remote changed from {} to {} while the review was open",
            kind.name(),
            remote.kind.name()
        );
    }
    match kind {
        ForgeKind::GitHub => github::diff(&remote, id, tokens),
        ForgeKind::GitLab => gitlab::diff(&remote, id, tokens),
    }
}

/// Path of a token file inside the app config directory.
pub fn token_file(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("f-terminal").join(name))
    }

    #[cfg(not(windows))]
    {
        if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(base).join("f-terminal").join(name));
        }
        if let Some(base) = std::env::var_os("HOME") {
            return Some(
                PathBuf::from(base)
                    .join(".config")
                    .join("f-terminal")
                    .join(name),
            );
        }
        None
    }
}

pub fn token_from_env(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

pub fn token_from_file(name: &str) -> Option<String> {
    let path = token_file(name)?;
    let value = std::fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub fn detect_url(url: &str) -> Result<RemoteInfo> {
    let (host, path) = parse_remote_url(url)
        .with_context(|| format!("unsupported Git remote URL: {}", url.trim()))?;
    let path = path.trim_matches('/').trim_end_matches(".git").to_owned();
    if path.is_empty() {
        bail!("remote has no repository path: {}", url.trim());
    }

    let host_without_port = host.split(':').next().unwrap_or(&host).to_ascii_lowercase();
    let (kind, api_base) = if host_without_port == "github.com"
        || host_without_port == "www.github.com"
    {
        (ForgeKind::GitHub, "https://api.github.com".to_owned())
    } else if host_without_port == "gitlab.com" || host_without_port == "www.gitlab.com" {
        (ForgeKind::GitLab, "https://gitlab.com/api/v4".to_owned())
    } else if host_without_port.starts_with("github.") || host_without_port.contains(".github.") {
        (ForgeKind::GitHub, format!("https://{host}/api/v3"))
    } else if host_without_port.starts_with("gitlab.") || host_without_port.contains(".gitlab.") {
        (ForgeKind::GitLab, format!("https://{host}/api/v4"))
    } else {
        bail!(
            "remote host is not GitHub or GitLab: {host}. \
             Self-hosted instances need a host starting with github. or gitlab."
        );
    };

    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    match kind {
        ForgeKind::GitHub if segments.len() != 2 => {
            bail!("GitHub remote path must be owner/repository: {path}")
        }
        ForgeKind::GitLab if segments.len() < 2 => {
            bail!("GitLab remote path must be group/repository: {path}")
        }
        _ => {}
    }

    Ok(RemoteInfo {
        kind,
        host,
        path,
        api_base,
    })
}

fn parse_remote_url(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches('/');

    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        return Some((host.to_owned(), path.to_owned()));
    }

    for scheme in ["ssh://", "https://", "http://", "git://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            let (authority, path) = rest.split_once('/')?;
            let host = authority.rsplit('@').next().unwrap_or(authority);
            return Some((host.to_owned(), path.to_owned()));
        }
    }

    None
}

fn remote_url(repo: &Path) -> Result<String> {
    git(repo, &["remote", "get-url", "origin"])
        .ok()
        .or_else(|| {
            let remotes = git(repo, &["remote"]).ok()?;
            let name = remotes.lines().next()?;
            git(repo, &["remote", "get-url", name]).ok()
        })
        .map(|url| url.trim().to_owned())
        .context("this repository has no Git remote")
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = git_command()
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
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
}
