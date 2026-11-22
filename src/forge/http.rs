use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Authentication header used for a forge API.
pub enum Auth {
    Bearer(String),
    PrivateToken(String),
}

impl Auth {
    fn header(&self) -> String {
        match self {
            Self::Bearer(token) => format!("Authorization: Bearer {token}"),
            Self::PrivateToken(token) => format!("PRIVATE-TOKEN: {token}"),
        }
    }
}

/// Perform an authenticated GET request with curl.
///
/// The token is written to curl's stdin through `-H @-` so it never appears in
/// the process argument list.
pub fn get(
    url: &str,
    accept: &str,
    auth: Option<Auth>,
    service: &str,
    hint: &str,
) -> Result<String> {
    let mut command = Command::new("curl");
    command.args([
        "-sS",
        "-L",
        "--max-time",
        "30",
        "--connect-timeout",
        "10",
        "-H",
        "User-Agent: f-terminal",
        "-H",
        &format!("Accept: {accept}"),
        "-w",
        "\n%{http_code}",
    ]);

    if auth.is_some() {
        command.args(["-H", "@-"]).stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .arg(url)
        .spawn()
        .context("failed to run curl; install curl to use remote reviews")?;

    if let Some(auth) = auth
        && let Some(mut stdin) = child.stdin.take()
    {
        let _ = writeln!(stdin, "{}", auth.header());
    }

    let output = child
        .wait_with_output()
        .context("failed to wait for the forge request")?;

    if !output.status.success() {
        bail!(
            "{service} request failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let response = String::from_utf8_lossy(&output.stdout);
    let Some((body, status)) = response.rsplit_once('\n') else {
        bail!("unexpected response from the {service} API");
    };
    let status = status.trim();

    if status != "200" {
        bail!(
            "{service} API returned HTTP {status}{hint}: {}",
            body.trim()
        );
    }

    Ok(body.to_owned())
}
