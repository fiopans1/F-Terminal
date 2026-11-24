use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::process::git_command;

/// Captured output of a Git command.
#[derive(Clone, Debug)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

/// Run Git without a shell, disabling interactive terminal prompts.
pub fn run(repo: &Path, args: &[&str]) -> Result<Output> {
    run_impl(repo, args, None)
}

/// Run Git and feed `input` through stdin (used for commit messages).
pub fn run_with_stdin(repo: &Path, args: &[&str], input: &str) -> Result<Output> {
    run_impl(repo, args, Some(input))
}

/// Convenience wrapper for dynamically built argument lists.
pub fn run_args(repo: &Path, args: &[String]) -> Result<Output> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run(repo, &args)
}

fn run_impl(repo: &Path, args: &[&str], input: Option<&str>) -> Result<Output> {
    let mut command = git_command();
    command
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if input.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if let Some(input) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        stdin
            .write_all(input.as_bytes())
            .context("failed to write to git stdin")?;
    }

    let output = child.wait_with_output().context("failed to wait for git")?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if !output.status.success() {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        bail!("git {} failed: {detail}", args.join(" "));
    }

    Ok(Output { stdout, stderr })
}
