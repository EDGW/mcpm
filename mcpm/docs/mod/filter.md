# Filter type

`mcpm mod list` and `mcpm server mods list` accept `-F/--filter <FILTER>` in
TUI, Console and Raw. The same expression can be edited in the TUI Filter card.

| Syntax | Meaning |
| --- | --- |
| `Fabric`, `'Fabric API'`, `"Fabric API"` | Case-insensitive contains matching. |
| `=fabric`, `='Fabric API'` | Case-insensitive whole-value matching. |
| `~'^Fabric.*API$'` | Rust regex, case-insensitive by default. |
| `@name`, `@modid`, `@file`, `@loaders` | Scope the next value or parenthesized group. |
| `A B`, `A & B` | AND, with higher precedence than OR. |
| `A \| B` | OR. |
| `(A \| B) & C` | Group conditions. |

Unscoped values search Name, Mod ID, File and Loaders, including hidden
descendants. Index is excluded. Exact matching compares the complete field;
Loaders compares each loader name without its display brackets. File uses
the displayed archive path selected by `--path-style`. Empty filters match
everything; `=""` matches an empty field.

Field groups inherit scope, while an explicit inner field overrides it:

```text
@loaders (Forge | Fabric)
@loaders (=Forge | =Fabric)
@name (Fabric | @modid =sodium)
```

The first expression also matches NeoForge because Forge is a substring.
The second matches only the two full loader names. Scope ends after the group:
`@loaders Forge | Fabric` leaves Fabric unscoped. Quote phrases rather than
relying on scope continuing across spaces.

Outside quotes, backslash escapes `@`, `(`, `)`, `&`, `|`, `=`, `~`, either
quote, backslash and whitespace. Inside quotes it escapes the matching quote
and backslash; other backslashes remain literal. For example:

```text
@file user\@host.jar
@file a\(b\)\&c\|d.jar
@name "He said \"hello\""
@name 'Mod (client | server)'
@modid ~'^\d+$'
```

Regex searches substrings; `^` and `$` anchor whole values. `(?-i)` makes a
regex case-sensitive. Look-around and backreferences are unsupported. Zero-width
matches select rows without highlighting text. Unknown fields, missing values,
invalid regex and unclosed groups/quotes report a character column. TUI marks
the error and matches no rows until the expression is valid again.

Quote the complete expression for your shell:

```sh
mcpm mod list mods/ -F "@modid 'Fabric'"
mcpm server mods list --all -F "@loaders (=Forge | =Fabric)"
mcpm --raw server mods list --file example.jar -F "@name ~'API$'" --expanded
```

Server input options `--file`, `--modid`, `--name` and `--all` first resolve the
input files; Filter only searches that selection. They are separate systems,
and `--all` can be combined with `-F`. Console and Raw search all declarations
and print matching rows, retaining the first match per input file unless
`--expanded` is set. TUI retains ancestor rows as context and can clear or edit
the initial filter without losing the input selection.
