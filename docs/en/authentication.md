# Authentication

> [Documentation](README.md) · [Español](../es/autenticacion.md)

## Local and remote Git

The Git manager and footer use the system `git` binary. For network operations
it is assumed that the user already has one of these configured:

- An SSH agent and keys for `git@github.com`.
- Or a credential helper for HTTPS.

Commands run without `sh -c`, with `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat` and
`LC_ALL=C`. If Git needs an interactive password, the operation fails and the
error is shown in the Git window footer.

## GitHub and GitLab for Code Review

The remote changes tab uses the detected provider's REST API. Public
repositories do not need a token. For private repositories, configure a token:

**GitHub**, in order:

1. `[forge.github] token` in `config.toml`.
2. `GITHUB_TOKEN` or `GH_TOKEN` in the environment.
3. `~/.config/f-terminal/github-token`
   (`%APPDATA%\f-terminal\github-token` on Windows), with the token on the
   first line.
4. `gh auth token`, if `gh` is installed and authenticated.

Recommended token:

- Fine-grained: read access to **Pull requests** and **Contents**.
- Classic: `repo` scope.

**GitLab**, in order:

1. `[forge.gitlab] token` in `config.toml`.
2. `GITLAB_TOKEN` or `GL_TOKEN` in the environment.
3. `~/.config/f-terminal/gitlab-token`
   (`%APPDATA%\f-terminal\gitlab-token` on Windows).
4. `glab auth token`, if `glab` is installed and authenticated.

Recommended token: **`read_api`** scope.

Tokens are sent to `curl` through stdin so they are not exposed in the process
list.

### Examples

```bash
# GitHub
export GITHUB_TOKEN=ghp_...

# GitLab
export GITLAB_TOKEN=glpat-...
```

Or create the corresponding file:

```bash
mkdir -p ~/.config/f-terminal
printf 'YOUR_TOKEN\n' > ~/.config/f-terminal/github-token
printf 'YOUR_TOKEN\n' > ~/.config/f-terminal/gitlab-token
chmod 600 ~/.config/f-terminal/github-token ~/.config/f-terminal/gitlab-token
```

On Windows:

```
%APPDATA%\f-terminal\github-token
%APPDATA%\f-terminal\gitlab-token
```

You can also use `gh auth token` and `glab auth token` if those CLIs are
installed and authenticated.

General configuration, including tokens, is described in
[Configuration](configuration.md).

---

[← Back to the index](README.md)
