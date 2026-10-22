# Server mod sources

Search remote projects, browse published files, or identify local mod archives:

```sh
mcpm server mods source search create --server ./my-server --source modrinth
mcpm server mods source files create --server ./my-server --source curseforge
mcpm server mods source identify --server ./my-server --source modrinth --all
```

All commands accept `--server PATH`, defaulting to the current directory, and
require a registered server with a supported mod loader. `--source` accepts
`modrinth`, `curseforge` (Curse.tools), or `curseforge@KEY` (official API).

## Search and files

Minecraft version and loader filters default to the registered server's metadata.
Explicit `--game-version` and `--loader` values override the corresponding default.
`--all-versions` disables both filters and conflicts with either explicit filter.
These filters select declared compatibility; they do not check mod dependencies.

Search accepts an optional [search expression](../mod/source-search.md): keywords
followed by `@version VALUE` and/or `@loader VALUE`. Use `(A | B)` for alternatives
and `*` to clear a field. Expression fields override CLI options and server defaults;
quote the entire expression for your shell.
Files takes remote project IDs or exact slugs, with additional projects read from
redirected stdin, one per line. These identifiers refer to remote projects, not
local mod IDs.

```sh
mcpm server mods source search "Steam 'n' Rails" --source modrinth
mcpm server mods source files 688231 --source curseforge --all-versions
mcpm server mods source files create --source modrinth --game-version 1.21.1
printf '%s\n' create ZzjhlDgM | mcpm server mods source files --source modrinth
```

Search and single-project files use the TUI when all streams are terminals.
The search field stays at the top: press `/` to edit, Enter to submit a new
search from the first page, or Esc to discard edits. Tab / Shift+Tab switches
between Search, Results, and the bottom Details panel. Results reuses the mod list
layout: each result shows Project Name (Primary Color) and loaders on the first
line, then the reported Minecraft versions (dimmed, with muted commas) across the second line.
Versions show up to three [catalog-backed abbreviations](../mod/source-search.md)
followed by `(all N versions)` counting distinct versions; Details wraps
the full sorted identifiers without abbreviating them.
Details follows the selection and shows loaders, Minecraft versions, categories,
summary, source, ID, slug, and project URL. Loaders use their configured colors.
Compatibility metadata comes from the source response; CurseForge may report only
recent files, and empty metadata means unreported. Name and slug retain their primary and secondary colors; sources use
the configured CurseForge / Modrinth colors, and IDs are dimmed. Details uses the
shared field list: arrows or j/k selects fields, PageUp/PageDown moves by a screen,
and Home/End selects the first/last field. Long values use an ellipsis; c copies
the full value. Enter expands/collapses Details, and Esc closes expanded Details
before exiting. Mouse clicks and scrolling are supported.
Enter on a result opens its files with the same compatibility filters;
Esc returns to search. Multiple projects, `-N/--no-ui`, explicit columns, or
redirected streams use list output. Sorting, pagination, and file release filters
work like `mcpm mod source`.

Search also provides [field value suggestions](../mod/source-search.md): loader
choices are Forge, NeoForge and Fabric; versions load from the current source API
and are cached for the session. Up/Down selects, Enter/Tab inserts, and Esc closes
the popup. Completion only edits the draft; Enter again submits the search.

Files uses the shared mod list layout, with a compact list above a Details panel
and no interactive Filter card. Headers and files occupy two lines: Index, Name,
Minecraft, and Loaders on the first; Version and Release on the second. Values
align with their headers without repeating field labels in each row. Filenames
are available in Details. Tab / Shift+Tab switches between the list and Details;
Up/Down or j/k moves through files or detail fields, and PageUp/PageDown moves by
a screen. Left/Right or p/n fetches the previous/next remote page. Home/End moves
to the first/last file or field. Enter expands/collapses Details, c copies the
selected detail field, and Esc closes expanded Details before returning to the
previous screen; q returns immediately. Mouse clicks and scrolling are supported.

For a single project, `--selected FILE_ID` fetches that file directly and shows
it first under `Selected`, even outside the current page or compatibility filters.
`--loader` (alias `--loaders`) accepts comma-separated or repeated loaders, matching any of them.
It stays visible while paging through files. In the TUI, the same publication also
keeps its original list position: its Name uses primary color, while other file
names use secondary color. Without `--selected`, names use primary color.
Console and Raw output continue to omit the duplicate pinned publication. Pagination and totals still describe the original
filtered results; the pinned file does not consume the page limit. A missing file
or a file belonging to another project is an error. This also works with standalone
`mcpm mod source files`.

```sh
mcpm mod source files sodium --source modrinth --selected yaoBL9D9
mcpm --raw mod source files sodium --source modrinth --selected yaoBL9D9 --columns selected,id,filename
```

Raw output puts the selected file first and keeps the default TSV columns.
Use `--columns selected` (or `-C S`) to include a `true`/`false` selection flag;
the selected file has an empty `index` because it is outside pagination.

## Identify

Inputs follow the existing server mod selection rules: filename, then exact
mod ID, then exact display name. `--file`, `--modid`, and `--name` select an
explicit interpretation. Paths are relative to the server's mods directory;
`mods/` prefixes are accepted. Parent traversal and absolute paths are rejected.
Use standalone `mcpm mod source identify` for arbitrary local paths.

```sh
mcpm server mods source identify create.jar --source curseforge
mcpm server mods source identify --modid create --source modrinth
printf '%s\n' create | mcpm server mods source identify --source modrinth --input modid
```

Use `--all` to select every `.jar` and `.jar.disabled` file in the mods directory.
With no selection, the command performs no lookup and shows guidance on terminal
stderr. Stdin appends one selection per line; `--input` controls its interpretation.
Repeated selections of the same file are identified once.

Identification hashes complete local files and does not filter by Minecraft
version or loader. File and `--all` selections do not require readable mod
metadata. Name and mod ID selections parse metadata to resolve paths; nested
declarations select their containing outer archive.

Unmatched files produce `unmatched` rows. Selection, read, or request errors are
reported on stderr; remaining inputs are processed and the command exits nonzero.

`--parallelism <COUNT>` defaults to 10, sharing the mod-list default. Lookups run concurrently while rows and per-file errors retain selection order; stderr progress uses the mod-list style. SSH options are supported by standalone `mod source identify`, `mod list`, `mod info` and `mod deps`.

## Output

All three lists support `-C` column letters and `--columns` names. See each
command's `--help` for available columns. Raw or redirected stdout emits escaped
TSV without headers; `--raw` also disables progress indicators.

```sh
mcpm server mods source search create --source modrinth -N --columns name,id,url
mcpm server mods source files create --source modrinth --columns id,filename,size
mcpm --raw server mods source identify --source modrinth --all --columns path,project,id,status
```
