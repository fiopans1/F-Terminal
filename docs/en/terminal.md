# Terminal

> [Documentation](README.md) · [Español](../es/terminal.md)

Each pane has:

- Its own PTY and child process.
- Its own `alacritty_terminal` `Term` and renderable `Frame`.
- Independent scrollback.
- Its cwd, tracked with `sysinfo` every 750 ms.
- Its selection state, cursor and VT modes.
- A **Starting \<profile\>…** notice until the shell produces its first output.

VT support includes:

- 16/256-color ANSI and truecolor.
- Inverse, hidden, bold, underline and other attributes.
- Alternate screen and scrollback.
- DA/DSR responses and color queries.
- Window title via OSC.
- Focus, IME commit and bracketed paste.
- SGR mouse and wheel.
- Real PTY resizing (`TIOCSWINSZ`/ConPTY).

The renderer does not know about `alacritty_terminal`: it consumes a `Frame`
already resolved into text, colors, backgrounds and cursor. This keeps the VT
engine isolated from the graphics layer.

## Scrollback search and links

`Cmd+Shift+F` on macOS and `Ctrl+Shift+F` on Linux/Windows opens the search box
for the focused pane. The search walks the scrollback and the visible screen,
treats the query as literal text and uses *smart case*: lowercase ignores case;
if you type any uppercase character, it becomes case-sensitive. Results are
highlighted and capped at 2000 matches to keep the interface responsive.

| Action | Key |
|---|---|
| Open/close the search box | `Cmd+Shift+F` / `Ctrl+Shift+F` |
| Search as you type | any text |
| Next / previous | `Enter` / `Shift+Enter` or `↓` / `↑` |
| Delete a character | `Backspace` |
| Close | `Esc` |

The box counter shows `position/total`. Opening another pane, switching
sessions or closing the searched pane closes the search box.

`OSC 8` links and plain URLs (`http://`, `https://`, `www.`, `mailto:`) are
detected on hover and underlined with the accent color. To avoid accidental
opens, `Ctrl+Click` on Linux/Windows or `Cmd+Click` on macOS opens the link in
the default browser. Only those schemes are allowed; `file://` and others are
ignored.

## Autocomplete

Inline autocomplete and the popup are documented in
[Autocomplete](autocomplete.md).

---

[← Back to the index](README.md)
