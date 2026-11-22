use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::forge::http::{self, Auth};
use crate::forge::{ForgeKind, RemoteChange, RemoteInfo, token_from_env, token_from_file};

const HINT: &str = " (private repository or missing token? set GITLAB_TOKEN, GL_TOKEN or ~/.config/f-terminal/gitlab-token)";

pub fn list_open(remote: &RemoteInfo) -> Result<Vec<RemoteChange>> {
    let project = encode_project(&remote.path);
    let mut changes = Vec::new();

    for page in 1..=10 {
        let url = format!(
            "{}/projects/{project}/merge_requests?state=opened&per_page=100&page={page}&order_by=updated_at&sort=desc",
            remote.api_base
        );
        let body = http::get(
            &url,
            "application/json",
            token(remote).map(gitlab_auth),
            "GitLab",
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

pub fn diff(remote: &RemoteInfo, id: u64) -> Result<String> {
    let project = encode_project(&remote.path);
    let url = format!(
        "{}/projects/{project}/merge_requests/{id}/changes",
        remote.api_base
    );
    let body = http::get(
        &url,
        "application/json",
        token(remote).map(gitlab_auth),
        "GitLab",
        HINT,
    )?;
    build_diff(&body)
}

pub fn parse_list(body: &str) -> Result<Vec<RemoteChange>> {
    let merge_requests: Vec<GitLabMr> =
        serde_json::from_str(body).context("failed to parse the GitLab merge request list")?;
    Ok(merge_requests.into_iter().map(RemoteChange::from).collect())
}

pub fn build_diff(body: &str) -> Result<String> {
    let response: ChangesResponse =
        serde_json::from_str(body).context("failed to parse the GitLab merge request changes")?;

    let mut diff = String::new();
    for change in &response.changes {
        diff.push_str(&format!(
            "diff --git a/{} b/{}\n",
            change.old_path, change.new_path
        ));

        if change.new_file {
            diff.push_str("new file mode 100644\n");
        }
        if change.deleted_file {
            diff.push_str("deleted file mode 100644\n");
        }
        if change.renamed_file {
            diff.push_str(&format!(
                "rename from {}\nrename to {}\n",
                change.old_path, change.new_path
            ));
        }

        let old = if change.new_file {
            "/dev/null".to_owned()
        } else {
            format!("a/{}", change.old_path)
        };
        let new = if change.deleted_file {
            "/dev/null".to_owned()
        } else {
            format!("b/{}", change.new_path)
        };
        diff.push_str(&format!("--- {old}\n+++ {new}\n"));
        diff.push_str(&change.diff);
        if !change.diff.is_empty() && !change.diff.ends_with('\n') {
            diff.push('\n');
        }
    }

    if response.overflow {
        diff.push_str("\\ ... GitLab truncated this diff ...\n");
    }

    Ok(diff)
}

pub fn encode_project(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len() + 8);
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn gitlab_auth(token: String) -> Auth {
    if token.starts_with("glpat-") || token.starts_with("gldt-") {
        Auth::PrivateToken(token)
    } else {
        Auth::Bearer(token)
    }
}

fn token(remote: &RemoteInfo) -> Option<String> {
    token_from_env(&["GITLAB_TOKEN", "GL_TOKEN"])
        .or_else(|| token_from_file("gitlab-token"))
        .or_else(|| {
            static GLAB_TOKEN: OnceLock<Option<String>> = OnceLock::new();
            GLAB_TOKEN
                .get_or_init(|| {
                    let host_output = std::process::Command::new("glab")
                        .args(["auth", "token", "--hostname", &remote.host])
                        .output()
                        .ok();
                    if let Some(output) = host_output
                        && output.status.success()
                    {
                        let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                        if !token.is_empty() {
                            return Some(token);
                        }
                    }

                    let output = std::process::Command::new("glab")
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

#[derive(Deserialize)]
struct GitLabMr {
    iid: u64,
    title: String,
    author: Option<GitLabUser>,
    source_branch: String,
    target_branch: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    work_in_progress: bool,
    #[serde(default)]
    web_url: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
struct GitLabUser {
    username: String,
}

impl From<GitLabMr> for RemoteChange {
    fn from(mr: GitLabMr) -> Self {
        Self {
            id: mr.iid,
            title: mr.title,
            author: mr
                .author
                .map(|author| author.username)
                .unwrap_or_else(|| "ghost".to_owned()),
            source_branch: mr.source_branch,
            target_branch: mr.target_branch,
            draft: mr.draft || mr.work_in_progress,
            web_url: mr.web_url,
            updated_at: mr.updated_at,
            forge: ForgeKind::GitLab,
        }
    }
}

#[derive(Deserialize)]
struct ChangesResponse {
    #[serde(default)]
    changes: Vec<GitLabChange>,
    #[serde(default)]
    overflow: bool,
}

#[derive(Deserialize)]
struct GitLabChange {
    #[serde(default)]
    old_path: String,
    #[serde(default)]
    new_path: String,
    #[serde(default)]
    new_file: bool,
    #[serde(default)]
    deleted_file: bool,
    #[serde(default)]
    renamed_file: bool,
    #[serde(default)]
    diff: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_nested_project_paths() {
        assert_eq!(
            encode_project("grupo/subgrupo/repo"),
            "grupo%2Fsubgrupo%2Frepo"
        );
    }

    #[test]
    fn parses_gitlab_merge_requests() {
        let body = r#"[
            {
                "iid": 12,
                "title": "Add search",
                "author": { "username": "octocat" },
                "source_branch": "feat/search",
                "target_branch": "main",
                "draft": false,
                "web_url": "https://gitlab.com/g/r/-/merge_requests/12",
                "updated_at": "2026-11-22T10:00:00Z"
            },
            {
                "iid": 8,
                "title": "Draft work",
                "author": null,
                "source_branch": "wip",
                "target_branch": "main",
                "work_in_progress": true
            }
        ]"#;

        let changes = parse_list(body).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].id, 12);
        assert_eq!(changes[0].id_label(), "!12");
        assert_eq!(changes[0].author, "octocat");
        assert_eq!(changes[0].source_branch, "feat/search");
        assert_eq!(changes[1].author, "ghost");
        assert!(changes[1].draft);
    }

    #[test]
    fn builds_a_unified_diff_from_gitlab_changes() {
        let body = r#"{
            "overflow": false,
            "changes": [
                {
                    "old_path": "src/a.rs",
                    "new_path": "src/a.rs",
                    "diff": "@@ -1 +1 @@\n-old\n+new\n"
                },
                {
                    "old_path": "src/new.rs",
                    "new_path": "src/new.rs",
                    "new_file": true,
                    "diff": "@@ -0,0 +1 @@\n+hello\n"
                }
            ]
        }"#;

        let diff = build_diff(body).unwrap();
        assert!(diff.contains("diff --git a/src/a.rs b/src/a.rs"));
        assert!(diff.contains("new file mode 100644"));
        assert!(diff.contains("--- /dev/null"));
        assert!(diff.contains("@@ -1 +1 @@"));
    }
}
