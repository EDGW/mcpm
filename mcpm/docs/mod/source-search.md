# Remote mod search expressions

`mcpm mod source search` and `mcpm server mods source search` share this syntax
across CurseForge, Modrinth, TUI, Console and Raw. Put keywords before fields.

```sh
mcpm mod source search 'create @version 1.20.1 @loader fabric' --source modrinth
mcpm mod source search 'create @version (1.20.1 | 1.21.1) @loader (forge | neoforge)' --source curseforge
mcpm server mods source search '@version * @loader *' --source modrinth
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
mcpm mod source search "\"Steam 'n' Rails\" @loader forge" --source curseforge
```

Regex, negation, version ranges and general nested boolean expressions are
unsupported. Unknown or repeated fields, missing values and unclosed quotes or
groups report a one-based character column. Each provider controls keyword
matching and sorting. CurseForge accepts at most 4 versions and 5 loaders per
search; unsupported loaders produce explicit errors.

In the TUI, press `/` to edit the top expression, Enter to search from the first
page, or Esc to discard edits. Syntax errors show a column in the search field
and move the cursor there while preserving existing results. Cancelling or failing
a request also preserves previous results and filters. Opening Files keeps all
version and loader alternatives, applied before file pagination.
