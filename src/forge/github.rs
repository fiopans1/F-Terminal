use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::forge::http::{self, Auth};
use crate::forge::{
    ForgeKind, ForgeTokens, RemoteChange, RemoteInfo, token_from_env, token_from_file,
};

const HINT: &str = " (private repository or missing token? set [forge.github] token, GITHUB_TOKEN, GH_TOKEN or ~/.config/f-terminal/github-token)";

pub fn list_open(remote: &RemoteInfo, tokens: &ForgeTokens) -> Result<Vec<RemoteChange>> {
    let mut changes = Vec::new();

    for page in 1..=10 {
        let url = format!(
            "{}/repos/{}/pulls?state=open&per_page=100&page={page}&sort=updated&direction=desc",
            remote.api_base, remote.path
        );
        let body = http::get(
            &url,
            "application/vnd.github+json",
            token(tokens).map(Auth::Bearer),
            "GitHub",
            HINT,
        )?;
        let mut page_changes = parse_list(&body)?;
        let count = page_changes.len();
        changes.append(&mut page_changes);
        if count < 100 {
            break;
        }
    }

    Ok(changes)
}

pub fn diff(remote: &RemoteInfo, id: u64, tokens: &ForgeTokens) -> Result<String> {
    let url = format!("{}/repos/{}/pulls/{id}", remote.api_base, remote.path);
    http::get(
        &url,
        "application/vnd.github.v3.diff",
        token(tokens).map(Auth::Bearer),
        "GitHub",
        HINT,
    )
}

pub fn parse_list(body: &str) -> Result<Vec<RemoteChange>> {
    let pulls: Vec<GithubPull> = serde_json::from_str(body)
        .context("no se pudo interpretar la lista de pull requests de GitHub")?;
    Ok(pulls.into_iter().map(RemoteChange::from).collect())
}

#[derive(Deserialize)]
struct GithubPull {
    number: u64,
    title: String,
    user: Option<GithubUser>,
    head: GithubRef,
    base: GithubRef,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    web_url: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
struct GithubUser {
    login: String,
}

#[derive(Deserialize)]
struct GithubRef {
    #[serde(rename = "ref")]
    name: String,
}

impl From<GithubPull> for RemoteChange {
    fn from(pull: GithubPull) -> Self {
        Self {
            id: pull.number,
            title: pull.title,
            author: pull
                .user
                .map(|user| user.login)
                .unwrap_or_else(|| "ghost".to_owned()),
            source_branch: pull.head.name,
            target_branch: pull.base.name,
            draft: pull.draft,
            web_url: pull.web_url,
            updated_at: pull.updated_at,
            forge: ForgeKind::GitHub,
        }
    }
}

fn token(tokens: &ForgeTokens) -> Option<String> {
    tokens
        .github
        .clone()
        .or_else(|| token_from_env(&["GITHUB_TOKEN", "GH_TOKEN"]))
        .or_else(|| token_from_file("github-token"))
        .or_else(|| {
            static GH_TOKEN: OnceLock<Option<String>> = OnceLock::new();
            GH_TOKEN
                .get_or_init(|| {
                    let output = crate::process::command("gh")
                        .args(["auth", "token"])
                        .output()
                        .ok()?;
                    if output.status.success() {
                        let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                        if !token.is_empty() {
                            return Some(token);
                        }
                    }
                    None
                })
                .clone()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_pull_requests() {
        let body = r#"[
            {
                "number": 42,
                "title": "Add search",
                "user": { "login": "octocat" },
                "head": { "ref": "feat/search" },
                "base": { "ref": "main" },
                "draft": false,
                "web_url": "https://github.com/o/r/pull/42",
                "updated_at": "2026-11-22T10:00:00Z"
            },
            {
                "number": 7,
                "title": "Draft work",
                "user": null,
                "head": { "ref": "wip" },
                "base": { "ref": "main" },
                "draft": true
            }
        ]"#;

        let pulls = parse_list(body).unwrap();
        assert_eq!(pulls.len(), 2);
        assert_eq!(pulls[0].id, 42);
        assert_eq!(pulls[0].id_label(), "#42");
        assert_eq!(pulls[0].author, "octocat");
        assert_eq!(pulls[0].source_branch, "feat/search");
        assert_eq!(pulls[0].web_url, "https://github.com/o/r/pull/42");
        assert_eq!(pulls[1].author, "ghost");
        assert!(pulls[1].draft);
        assert_eq!(pulls[0].list_title(), "#42 Add search");
    }
}
