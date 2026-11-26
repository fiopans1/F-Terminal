# Agent Mode

> [Documentation](README.md) · [Español](../es/agent-mode.md)

Opened with `Cmd+Shift+A` / `Ctrl+Shift+A` or from the macOS menu. The window
and menu use the name **Agentes**; Agent Mode is the feature name. It is a
**separate window** that can stay open while you use the terminal, Code Review
or the Git manager.

## Agent center

The window works as a supervision dashboard:

- **Header**: active agents, pending attention, **+ Launch**, **Seen**,
  **Refresh** and close.
- **KPIs**: active/blocked, pending attention, aggregate CPU and RAM, and
  accumulated Git diff per repository.
- **Search**: filters by name, status, message, title, tab, branch, command,
  path or PID. Filter counters respect the search.
- **Filters**: All, Attention, Active and Finished; origin filter
  (internal/external) and sort by priority, CPU, RAM, time or name.
- **List**: each row shows status, message or last output, repository, branch
  with `+N/-N`, subagents/child processes, uptime, pane and telemetry.
- **Inspector**: **Summary**, **Console** and **Subagents** tabs. The console
  keeps up to 60 lines of the visible screen and scrolls with the mouse wheel.
- **Actions**: focus the pane, open Git or Code Review in its directory, copy
  path/PID, mark as seen and kill the process with confirmation.
- **Quick reply**: `I` opens the message field; `Enter` sends it to the pane.
  `Y` approves if the agent is blocked. There are also buttons to approve, send
  `Enter`, send `Ctrl+C` and paste from the clipboard.
- **Launcher**: `N` opens the profile bar; `1` Claude, `2` Codex, `3` OpenCode
  and `4` Junie. It launches the CLI in a new tab using the selected agent's
  directory (or the focused cwd).

It shows every detected agent, both those running inside F-Terminal and those
running in other terminals or IDEs (the latter in read-only mode). The list is
sorted by priority:

1. Pending permissions or questions.
2. Attention notices.
3. Errors.
4. Finished work not yet read.
5. Working.
6. Idle.
7. External processes.

Statuses:

- **Working**: the agent is generating or running tools.
- **Asks permission / Asks for input / Asks for login**: the agent is blocked.
- **Needs attention**: a notification or bell without a structured status.
- **Finished / Error**: the turn ended; the notice disappears when the pane is
  focused.
- **Idle**: the agent is still alive, waiting for the next prompt.
- **Running** (external): process detected, without real-time status.

## Detection

The main source is **OSC 7501** (Program Status Protocol). F-Terminal responds
to the support query and reads the reports agents write:

| Agent | Accurate status |
|---|---|
| Claude Code | `OSC 7501` since 2.1.295 |
| Codex CLI | `OSC 7501` since 0.163; `OSC 9` for completions |
| OpenCode | OpenCode 2.x; earlier versions use the fallback |
| Junie | Optional hooks (see below); otherwise process + activity + bell |
| Others | Any agent implementing `OSC 7501` |

There is also a background process watcher that walks each pane's process tree
and recognizes `claude`, `codex`, `opencode` and `junie`. That watcher is used
to know whether the agent is still alive, for agents without a protocol, and to
list external processes. The scan runs every 2 seconds by default, on a
separate thread, and does not block the UI.

## Controls

| Action | Key |
|---|---|
| Move selection | `↑` / `↓`, `J` / `K`, `PgUp` / `PgDn`, `Home` / `End` |
| Focus the agent's pane | `Enter` or double-click |
| Open quick reply | `I` |
| Approve y/n prompt | `Y` or the **Approve** button |
| Send `Enter` / `Ctrl+C` | quick-row buttons |
| Paste into search or message | `Ctrl/Cmd+V`, `Shift+Insert` |
| Search | `/` or `Cmd/Ctrl+F`; `↑` / `↓` move the selection; `Esc` clears |
| Change status filter | `Tab` / `Shift+Tab`, `F`, `1`–`4` |
| Change origin | `E` |
| Change sort order | `S` |
| Inspector tabs | `[` / `]` |
| Open launcher | `N`; `1`–`4` choose a profile |
| Refresh processes | `R` or `F5`, or **Refresh** |
| Open Git / Code Review | `G` / `V` |
| Copy path or PID | `C` |
| Mark as seen / all seen | `M` / `A` |
| Kill process | `X`, `Del` or `Backspace`; double press or **Kill PID** |
| Close window | `Esc`, `Cmd/Ctrl+W` or `×` |

The first press of `X`/`Del` arms the kill and the second confirms. Killing an
agent first sends `SIGTERM`; if it is still alive after two seconds, `SIGKILL`
is forced (on Windows, `taskkill /F`). The result notice appears in the window
footer.

External agents appear as `external` and cannot be focused or replied to
because their PTY belongs to another application. They can be inspected and
terminated from Agent Mode if they have a known PID.

## Junie with hooks

Junie does not implement `OSC 7501` yet, but its hooks can provide accurate
states. F-Terminal reads a small log at `~/.f-terminal/junie-events.log`; the
pane is identified by the `F_TERMINAL_PANE` variable that F-Terminal injects
into every shell.

On macOS/Linux, add this to `~/.junie/config.json` (only the `hooks` part):

```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command",
        "command": "echo \"$F_TERMINAL_PANE|working|\" >> \"$HOME/.f-terminal/junie-events.log\"",
        "async": true }] }
    ],
    "Stop": [
      { "hooks": [{ "type": "command",
        "command": "echo \"$F_TERMINAL_PANE|done|\" >> \"$HOME/.f-terminal/junie-events.log\"",
        "async": true }] }
    ],
    "StopFailure": [
      { "hooks": [{ "type": "command",
        "command": "echo \"$F_TERMINAL_PANE|error|\" >> \"$HOME/.f-terminal/junie-events.log\"",
        "async": true }] }
    ],
    "SessionEnd": [
      { "hooks": [{ "type": "command",
        "command": "echo \"$F_TERMINAL_PANE|end|\" >> \"$HOME/.f-terminal/junie-events.log\"",
        "async": true }] }
    ]
  }
}
```

On Windows, the equivalent command uses PowerShell, because `cmd` makes
escaping vertical bars complicated:

```text
powershell -NoProfile -Command "Add-Content -Path \"$env:USERPROFILE\\.f-terminal\\junie-events.log\" -Value \"$env:F_TERMINAL_PANE|working|\""
```

Change `working` to `done`, `error` or `end` in each hook.

Do not configure a `PermissionRequest` hook just to record status: in Junie, a
permission hook that succeeds without a decision **auto-approves** the action.
Pending permissions are shown as **Needs attention** through the bell, title
and terminal activity.

## Configuration

```toml
[agents]
# Discovers agents and responds to OSC 7501.
enabled = true
# Includes agents from other terminals/IDEs (read-only).
show_external = true
# Process scan interval, in milliseconds.
poll_interval_ms = 2000
```

## Limitations

- Accurate status depends on the agent implementing `OSC 7501` or on Junie
  hooks.
- `tmux` and `screen` may block `OSC 7501`; the process watcher keeps working,
  but status may be approximate.
- Over SSH the remote process is not visible, although `OSC` reports may reach
  the PTY.
- Under WSL, Linux processes are not visible from Windows `sysinfo`; `OSC`
  reports can still cross the PTY.
- External agents have no real-time status; they can be inspected (telemetry,
  command, PID) and terminated, but not controlled because their PTY belongs to
  another application.
- There is no structured approval: `Y`, `I` and the buttons send keystrokes to
  the PTY; the final decision is made by the agent CLI.

---

[← Back to the index](README.md)
