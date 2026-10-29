# Customize colors

`mcpm-tools customize` opens the TUI by default, with Console and Raw commands also supported. Configuration lives in `~/.mcpm`; missing values use embedded defaults.

The top level directly lists TUI Dark, TUI Light, Console Dark, and Console Light, each selecting its own palette. terminal-colorsaurus detects the terminal background automatically; rendering uses the matching palette, falling back to dark when detection is unavailable. Left/Right previews; Enter applies and saves. Press n to enter an immutable ID and copy the selected category's current scheme. Name initially matches ID. Press r to change only Name, d and type DELETE to delete, or e to open Editing ID with Name, Types checkboxes, Primary, and Secondary. Select Name to edit it inline; Left/Right, Home/End, Backspace and Delete move or edit the text. Shortcut letters are text while Name is selected; Ctrl-C requests exit. Space toggles Types; Enter saves from every row. Types filters previews to TUI, Console, or both. Colors display TUI names in their actual colors. Bundled schemes cannot be deleted. Up/Down selects a field; Esc reverts drafts or cancels input; q / Ctrl-C requests exit. Unsaved changes show a status-bar confirmation: Y saves and quits, N discards and quits, Esc keeps editing. The status bar shows saved file paths in green, warnings in yellow, and errors in red (unless NO_COLOR is set).

All settings display English names, also accepted by get/set. Existing property paths remain accepted.

```sh
mcpm-tools customize
mcpm-tools customize --no-ui list
mcpm-tools customize --theme tui-dark scheme list
mcpm-tools customize --theme tui-dark scheme create ocean --name Ocean
mcpm-tools customize --theme tui-dark scheme use ocean
mcpm-tools customize scheme rename ocean "Ocean Blue"
mcpm-tools customize scheme delete ocean
mcpm-tools customize set "TUI Dark Primary Color" Cyan
mcpm-tools customize --raw get "Console Dark Primary Color"
```

Upper-left and upper-right panels show TUI Dark and Light example cards. Lower-left and lower-right simulate Console Dark and Light output. Narrow terminals stack panels; short terminals show the available portion with a height prompt. Previews preserve the terminal background and show a switch-mode hint only for a mismatched or unknown terminal mode. Console samples have a dim border and a blank line above them. There is no appearance setting.

Startup scans every `.toml` collection in `CONFIG_DIR/styles`. Each contains a `[[style]]` array. Invalid files or individual styles produce warning logs and are skipped without blocking other styles. Unavailable selections fall back to bundled defaults. `--log-level off` suppresses stderr warnings; file logs still record them. Files load in sorted order; later duplicate IDs in ordinary collections are skipped; custom.toml overrides take precedence.

```toml
[[style]]
version = 1
id = "ocean"
name = "Ocean"
types = ["tui"]
primary = "Cyan"
secondary = "Blue"
```

Style entries override bundled defaults by ID, including partial overrides. All panel-created and panel-edited schemes are saved to `styles/custom.toml`, loaded last to override matching IDs. Other collections remain untouched. Deleting an imported scheme records its ID in the top-level `deleted` array in `styles/custom.toml`, so it stays hidden after restart. Recreating the ID clears that record. Selections live in `settings.toml`, loader colors in `loader_colors.toml`. Writes replace individual files atomically; reserialization removes comments and original formatting from edited files.

Console names: black, red, green, yellow, blue, magenta, cyan, white, their bright_ variants, and default_color. TUI names: Black, DarkRed, DarkGreen, DarkYellow, DarkBlue, DarkMagenta, DarkCyan, Grey, DarkGrey, Red, Green, Yellow, Blue, Magenta, Cyan, White, Reset. An uppercase initial selects exact TUI names; a lowercase initial selects exact Console names. Red and red therefore have different brightness. Settings always display TUI names. Use `mcpm-tools debug palette` to inspect terminal colors.

Supplying a subcommand executes directly; --no-ui disables interaction and defaults to listing settings. --raw or redirected stdout emits English names and values as TSV. The TUI draws on stderr; stdin does not determine the mode. NO_COLOR disables styling while allowing edits.

IDs allow ASCII letters, digits, underscores and hyphens. They must be unique and cannot be edited after creation. Name is an editable display label. Use `scheme create ID --name NAME` to set both at creation.
