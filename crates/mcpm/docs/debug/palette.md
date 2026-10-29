# Debug palette

```sh
mcpm-tools debug palette
mcpm-tools debug palette --no-ui
mcpm-tools debug palette --raw
```

Displays the 8 ordinary ANSI colors, 8 bright ANSI colors, and default foreground,
each with Normal, Bold, and Dim samples. Colors use the terminal's palette and
default background, so the command can help compare light and dark terminal themes.

- **TUI** (default): samples show their `crossterm` names, such as
  `Color::DarkGreen` and `Color::Green`. Columns identify `Attribute::Bold` and
  `Attribute::Dim`. Narrow terminals display the samples vertically.
  Scroll with Up/Down, j/k, PageUp/PageDown, or Home/End; exit with q, Esc, or Ctrl+C.
- **Console** (`--no-ui` or `-N`): samples show their `owo-colors` expressions,
  such as `.green()`, `.green().bold()`, and `.green().dimmed()`.
- **Raw** (`--raw` or redirected stdout): succeeds without any output.

Without an interactive stdin or terminal stderr, the command falls back to Console
when stdout is a terminal. `NO_COLOR` disables colors. Default foreground is
`Color::Reset` in TUI and `.default_color()` in Console; it is not fixed white.
