# Remote mod search expressions

`mcpm-tools mod source search` and `mcpm mods source search` share this syntax
across CurseForge, Modrinth, TUI, Console and Raw. Put keywords before fields.

```sh
mcpm-tools mod source search 'create @version 1.20.1 @loader fabric' --source modrinth
mcpm-tools mod source search 'create @version (1.20.1 | 1.21.1) @loader (forge | neoforge)' --source curseforge
mcpm mods source search '@version * @loader *' --source modrinth
```

| Syntax | Meaning |
| --- | --- |
| `create railways` | Send the whole keyword phrase to the provider, without interpreting boolean AND. |
| `@version 1.20.1` | Match an exact Minecraft version. |
| `@loader fabric` | Match an exact loader name, ignoring case. |
| `@version (1.20.1 \| 1.21.1)` | Match either version. |
| `@loader (forge \| neoforge)` | Match either loader. |
| `@version *` / `@loader *` | Clear that filter, including CLI and server defaults. |

Different fields use AND; alternatives in one group use OR. Duplicate alternatives
are removed. `forge` does not match `neoforge`. Specify each field only once;
use a group for alternatives. Keywords are optional. An empty expression browses
using the default filters.

Precedence: expression fields > `--game-version` / `--loader` > server defaults.
Removing a field restores its default; use `*` to remove a default restriction.
`--all-versions` removes server defaults, while expression fields still apply.

Quote keyword phrases containing `@`, `(`, `)`, `|`, or `&`, or escape characters
with a backslash. Single and double quotes are accepted and removed before the
request. Write a literal backslash as `\\`. In the search field, enter
`"Steam 'n' Rails" @loader forge` to preserve the apostrophes. A shell command
needs an additional layer of quoting or escaping:

```sh
mcpm-tools mod source search "\"Steam 'n' Rails\" @loader forge" --source curseforge
```

Regex, negation, version ranges and general nested boolean expressions are
unsupported. Unknown or repeated fields, missing values and unclosed quotes or
groups report a one-based character column. Each provider controls keyword
matching and sorting. CurseForge accepts at most 4 versions and 5 loaders per
search; unsupported loaders produce explicit errors.

In the TUI, press `/` to edit the top expression, Enter to search from the first
page, or Esc to discard edits. Like mod list, the editor colors fields and escapes
with the primary color, values with the secondary color, operators as important
text, and delimiters as muted text. Draft syntax errors are underlined in red
without submitting a request. Syntax errors show a column in the search field
and move the cursor there while preserving existing results. Cancelling or failing
a request also preserves previous results and filters. Opening Files keeps all
version and loader alternatives, applied before file pagination.

At `@loader` or `@version`, a popup below Search suggests values for the field at
the cursor, including alternatives inside `(A | B)`. Loader suggestions use the
existing Forge, NeoForge and Fabric choices. Versions come from the current
source's API, fetched in the background when Search opens and cached for the search session; typing stays
responsive while loading. Typing a prefix filters the list. Use Up/Down or
PageUp/PageDown to select, Enter/Tab to insert, or click a candidate. Inserting
only changes the draft; press Enter again to search. Esc closes the popup first;
another Esc discards edits. Failed version loads allow manual input and Ctrl-R to
retry. Both popup borders use the secondary color; version entries use normal text.

Results and Details reuse the shared mod list / Files browser widgets. Tab switches
between Search, Results and Details; mouse clicks and scrolling are supported.
Each result shows the project name and loaders, with the reported Minecraft
versions on a full-width second line. Details includes loaders, versions,
categories, and slug. Both loader fields show only forge, neoforge and fabric,
using their configured colors; supported versions are
dimmed, with muted commas. Series sort by their first two numeric components in
descending order; numeric patches ascend within each series. Results abbreviate
patches as `1.20.1/3/5~8/10/6-snapshot`, keeping nonnumeric suffixes last. Complete
catalog coverage becomes `1.20.x`, or `1.20(.x)` when `1.20` is also supported.
For non-1 prefixes, complete coverage of every `22.*` identifier becomes `22.x`,
regardless of segment count. The list shows at most three abbreviated items,
followed by `(all N versions)` counting distinct original identifiers, not items.
Unknown formats remain intact at the end. Without
a loaded catalog, wildcards are not inferred. Details wraps the full sorted
Minecraft identifiers over multiple lines and copies the complete field.
Metadata comes from the source response; CurseForge may report
only recent files. Missing metadata means unreported, and version/loader sets do
not guarantee every combination is compatible.
In Details, Up/Down or j/k selects a row (including wrapped Minecraft rows), PageUp/PageDown moves by a screen,
and Home/End selects the first/last field. Long values use an ellipsis; c copies
the full selected value. Enter expands/collapses Details, and Esc closes expanded
Details before exiting. Enter on a result opens Files.
