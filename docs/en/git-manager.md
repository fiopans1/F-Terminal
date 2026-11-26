# Git manager

> [Documentation](README.md) · [Español](../es/gestor-git.md)

Opened with `Cmd+Shift+M` / `Ctrl+Shift+M` or from the macOS menu.

It is a **separate native window**, distinct from Code Review. It can be open
at the same time as the terminal and Code Review. It uses the Git repository at
the focused pane's cwd.

## Interface

```
┌─────────────────────────────────────────────────────────────────────┐
│ GIT  repo · main ↑2 ↓1        Fetch  Pull  Push  Refresh            │
├──────────────┬──────────────────────────────────┬───────────────────┤
│ CHANGES      │ HISTORY                          │ DETAIL            │
│ Staged (3)   │ ● abc1234  Fix footer...  main   │ author, date      │
│ Changes (5)  │ ● def5678  Add PR search         │ message           │
│              │ ● 9012abc  ...                   │ files             │
│ REFS         │                                  │ diff              │
│  branches    │                                  │                   │
│  remotes     │                                  │                   │
│  tags        │                                  │                   │
│  stashes     │                                  │                   │
├──────────────┴──────────────────────────────────┴───────────────────┤
│ Commit message...                             Stash Amend Commit     │
└─────────────────────────────────────────────────────────────────────┘
```

## Working tree

- Separate list of **staged** and **unstaged** files.
- Conflict marker `U` for unresolved files.
- Click `+` to stage, `−` to unstage.
- `Enter`/`Space` on a row also stages/unstages.
- `Del`/`Backspace` discards changes with confirmation.
- **Stage all** / **Unstage all** buttons.
- If there are conflicts, **Abort merge** appears (`git merge --abort`).

## Commits

- Single-line message field in the bottom bar.
- **Commit** only with staged files and a non-empty message.
- **Amend** enables `git commit --amend`.
- The message is passed to Git through stdin (`git commit -F -`), without a
  shell.
- Commits run in the background and show the result in the footer.

## Branches, remotes and stash

In the **REFS** tab:

- `↑↓` moves the selection.
- `Enter` checks out a local or remote branch.
- A remote branch without a local one creates a tracking branch.
- `N` opens the new branch dialog.
- `M` merges the selected branch.
- `D` deletes a local branch with confirmation.
- For stashes, `Pop` applies and `Drop` deletes with confirmation.
- `R` refreshes.

## Remote

- **Fetch**: `git fetch --all --prune`.
- **Pull**: `git pull --ff-only` to avoid unexpected merges.
- **Push**: `git push`; if there is no upstream,
  `git push -u origin <branch>`.
- Network commands run in the background and do not block the UI.

## History and detail

- History of up to 300 commits with the working tree as the first row.
- Selection with `↑↓`, `PgUp/PgDn`, `Home/End`.
- Commit detail: hash, subject, author, date and refs.
- File list with status and `+/-`.
- Per-file diff with line numbers and colors.
- `↑↓` changes file; `PgUp/PgDn` scrolls the diff; `←→` scrolls horizontally.

## Refresh

- Working tree every 2 seconds.
- History, refs and detail after each operation or with `R`.
- All mutating operations are serialized: while one runs, the rest are
  disabled.

## Git manager limitations

- No rebase, cherry-pick, revert, tag management or force push.
- Deleting a branch uses `-d`; if it is not merged, Git returns an error.
- Pull is fast-forward only.
- Conflicts are shown and can be aborted, but there is no built-in resolution
  editor.
- History is limited to 300 commits.
- There is no interactive authentication: Git must have an SSH agent or
  credential helper configured. It runs with `GIT_TERMINAL_PROMPT=0`.
- No force push or remote management.

---

[← Back to the index](README.md)
