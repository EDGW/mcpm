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

Search takes an optional plain-text query; quote multiple words as one argument.
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
between Search, Results, and the bottom Details panel. Results follows the mod list
layout and shows only Project Name (Primary Color) and slug (Secondary Color).
Details follows the selection and shows the full summary, source, ID, slug, and
project URL. Name and slug retain their primary and secondary colors; sources use
the configured CurseForge / Modrinth colors, and IDs are dimmed. Focus Details and
use arrows or PageUp/PageDown to scroll. Enter on a result opens its files with the same compatibility filters;
Esc returns to search. Multiple projects, `-N/--no-ui`, explicit columns, or
redirected streams use list output. Sorting, pagination, and file release filters
work like `mcpm mod source`.

For a single project, `--selected FILE_ID` fetches that file directly and shows
it first under `Selected`, even outside the current page or compatibility filters.
`--loader` (alias `--loaders`) accepts comma-separated or repeated loaders, matching any of them.
It stays visible while paging through other files and is omitted from the other
file cards when duplicated. Pagination and totals still describe the original
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

## Output

All three lists support `-C` column letters and `--columns` names. See each
command's `--help` for available columns. Raw or redirected stdout emits escaped
TSV without headers; `--raw` also disables progress indicators.

```sh
mcpm server mods source search create --source modrinth -N --columns name,id,url
mcpm server mods source files create --source modrinth --columns id,filename,size
mcpm --raw server mods source identify --source modrinth --all --columns path,project,id,status
```
