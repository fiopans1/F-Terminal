# Code Review

> [Documentation](README.md) · [Español](../es/code-review.md)

Opened with `Cmd+Shift+R` / `Ctrl+Shift+R` or from the macOS menu. It is a
**separate native window**: the main terminal remains usable.

## Local tab

- **Local changes**: working tree changes, including untracked files.
- **Branch against main/master**: diff of the current branch against the base.
- **Last 15 commits**.
- `↑↓` to move, `Enter` or click to open, `R` to refresh.

## Pull requests / Merge requests tab

- Detects the provider from the `origin` remote:
  - **GitHub**: the tab is called **Pull requests** and uses `#123`.
  - **GitLab**: the tab is called **Merge requests** and uses `!123`.
- Lists the remote's open changes.
- Paginated loading of up to 1,000 PRs/MRs.
- Search by number, title, author or branch; multiple space-separated terms are
  supported.
- `/` or `Cmd+F`/`Ctrl+F` focus the search box.
- `↑↓` selects, `Enter` loads the diff, `R` reloads the list.
- `Esc` leaves the search box without closing the window.

Remote support:

- `github.com` and GitHub Enterprise on `github.*` hosts.
- `gitlab.com`, nested groups (`group/subgroup/repo`) and self-hosted GitLab on
  `gitlab.*` hosts.
- Hosts with other names will need explicit configuration in a future
  iteration.

## Diff and review

- Three columns: reviews, file selector and diff.
- Per-file status: `M/A/D/R/B`.
- `+additions −deletions` per file.
- `Space` or `M` marks a file as reviewed.
- In **Local changes**, the **Revert** button (or `Del`/`Backspace`) reverts
  the selected file; the first click/press asks for confirmation and `Esc`
  cancels. Untracked files are deleted.
- Marks live only in memory for the session.
- `Tab` switches panels; `Esc` or `×` closes.
- The diff loads in the background and is discarded if it arrives late
  (`window_id` + `request_id`).

## Authentication

Public repositories work without a token, within each API's request limit. For
private repositories, see
[Authentication](authentication.md#github-and-gitlab-for-code-review).

---

[← Back to the index](README.md)
