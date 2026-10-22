# `mcpm mod list`

```text
mcpm mod list [OPTIONS] [PATH]...
```

List mod files, accepting any mixture of file and directory paths. With no
paths, the current directory (`.`) is used.

`--parallelism <COUNT>` bounds outer-JAR loading and loader metadata parsing (default 10, positive integers only; 1 scans sequentially). It applies to TUI, Console, Raw and SSH; SSH performs the scan on the remote host. Files run concurrently, while loaders and nested JARs within each file remain sequential. Progress follows actual completions, but final rows and numbering retain input order and directory path sorting. TUI opens after metadata loading completes. Duplicate normalized paths share cached results, including failures, while retaining a position per input; `.jar` and `.jar.disabled` remain distinct. The cache lasts only for the scan session and does not detect file changes.

Loading concurrency is independent of `--identify-parallelism`. List calls the identification business API without decoding identification packets. Defaults, runtime ownership, stdin paths and terminal progress belong to shared command infrastructure rather than either command implementation.

`--identify-parallelism` appears under Loading options and is marked TUI only.
Help groups options into Global options, Mode options, Loading options, Filter, Style options,
and Console-only options, in that order. Mode-specific options
are still accepted in other modes and ignored there. `--expanded` belongs to
Style options: it opens all TUI branches, prints all declarations in Console and
Raw, and additionally lists ignored filenames in Console.

In a terminal, the default TUI shows a scrollable mod tree and a compact Details
card below it. Each mod occupies two lines: its file path is on the second line,
left-aligned with Mod ID. File is omitted from the first-line columns. The selected
item has a background highlight across both lines. `>` marks a collapsed
branch and `v` an open branch; leaves have no arrow. Right expands one level,
Left collapses or selects the parent, Space toggles the branch, and `e` expands
all branches. `--expanded` starts with every branch open.

In TUI mode, outer JARs are queued automatically in list order, with
Modrinth and CurseForge (through Curse.tools) lookups for each file. Up to 10 provider lookups run concurrently by default while navigation remains
available. `--identify-parallelism <COUNT>` sets a positive concurrency limit;
use `1` for sequential identification. Below Index, the first position
belongs to Modrinth and the second to CurseForge: a spinner means identification
is running, and a checkmark means a verified match. Details shows each provider's
status in two-line entries. The first line shows the status and, on success,
dimmed project and file IDs. The second shows `Enter to view release files →`
for a match, or explains why the source is unavailable. Both lines select as one
field; copying retains the full result, including filename, match type and size. Failed requests do not stop later files;
exiting the TUI cancels pending work. Repeated file paths share identification
results. Nested JARs are not identified and do not inherit outer-file badges.
Console and Raw do not perform remote identification.

Select a matched Modrinth or CurseForge field in Details and press Enter to open
the project's files, with the identified version pinned as `Selected`. Command-line
`--loader/--loaders` filters carry over, matching any selected loader; an unfiltered
list leaves files unrestricted. Esc or q returns to the original row, Details field
and filter state. Esc cancels loading; request failures return to the list with an
error. Unmatched sources and nested JARs do not offer this action.

Provider colors come from `source_colors.toml` in `~/.mcpm`, with bundled defaults
in `mcpm/config/source_colors.toml`. The `source_colors.tui` and
`source_colors.console` tables contain `modrinth` and `curseforge` colors. These
colors are not editable through `mcpm customize`.

Use Up/Down, PageUp/PageDown, Home/End or a trackpad to navigate. `F` opens a
Filter card between Mods and Details. `-F/--filter <FILTER>` initializes it with
the same typed expression accepted by Console and Raw. For example:

```sh
mcpm mod list mods/ -F "@modid 'Fabric'"
mcpm mod list mods/ -F "@loaders (=Forge | =Fabric) & @name ~'API$'"
```

Plain or quoted text performs case-insensitive contains matching. `=` matches a
whole value; `~` introduces a quoted Rust regex. Whitespace or `&` means AND,
`|` means OR, AND binds more tightly, and parentheses group conditions. Fields
are `@name`, `@modid`, `@file`, and `@loaders`; Index is excluded. A field applies
to the next value or group: `@loaders (Forge | Fabric)` scopes both alternatives,
but `@loaders Forge | Fabric` leaves the second unscoped. Explicit fields within
a group override its inherited field. Quote phrases: `@name "Fabric API"`.
See [Filter syntax](filter.md) or `--help` for complete escaping and regex rules.

Fields use the primary color, values the secondary color, operators the important
text style, and quote/group delimiters the muted style. Escapes use the primary
color. Invalid expressions show a diagnostic with a character column, underline
the invalid token and match no rows. Parsing is shared with CLI validation and
result highlighting; only satisfied boolean branches contribute highlighted hits.

Console and Raw search all declarations, including hidden descendants, then
print matching rows. Without `--expanded`, they print the first matching
declaration per input file. An empty filter preserves normal list output.

Search automatically opens only the ancestor paths needed to reveal matching rows.
Matching a parent alone does not automatically open its children. Branches retain
their normal expansion state underneath the search view and restore it as soon as
they leave the matching paths, even before the filter is cleared.
Left/Right, Space, mouse arrows and `e` still control branches while filtering.
These manual overrides take priority over automatic expansion and last until the
branch leaves the search results; clearing the filter restores the normal tree.

Matched text uses the secondary color and reverses its original bold weight:
normal text becomes bold, while already bold text becomes regular. If a hit would
be hidden by truncation, the cell keeps its beginning and shows context around the
first hit, separated by ellipses. The complete hit takes priority over context;
only hits wider than the available cell width are shortened. This applies to the
table, the File line and Details; copying still uses the complete original value.

Filter updates live. Enter applies and returns focus to Mods; Esc or uppercase
`Q` closes the Filter card while retaining its conditions. Lowercase `q` is
ordinary filter text. `r` outside the filter clears it. Tab/Shift-Tab cycles
through the visible Mods, Filter and Details cards. In Details, Up/Down selects
a field and `c` copies its full value, even when displayed text ends with `...`.
The status line reports clipboard success or failure. On other fields, Enter opens/closes full
Details, `i` shows skipped files, and `q`, Esc or Ctrl-C exits outside the filter.
Mouse clicks select rows and fields; clicking an arrow toggles its branch.

`--no-ui` prints the Console list. `--raw` or redirected stdout produces TSV;
`--no-ui --raw` also uses Raw. TUI mode uses the detected terminal's TUI palette,
while Console uses its Console palette. NO_COLOR disables colors.

## SSH listing

Use `--ssh 'ssh -i key -p 2222 user@host'` to load mods on another machine and
display them locally. `--ssh-mcpm <PATH>` selects its mcpm executable; without
it, the remote command uses `mcpm`. These options are top-level global options.
They are supported by `list` TUI, Console and Raw, and by
[`mod source identify`](source-identify.md); other commands reject them.

The connection command contains the SSH executable, options and destination,
without a remote command. Quote its arguments using shell-style quoting.
mcpm opens one non-PTY SSH connection and starts one persistent `mcpm remote`
process. POSIX shells and Windows cmd/PowerShell are supported. For a custom
Windows executable, use a Windows path or a name ending in `.exe` (for example
`C:\mcpm.exe`). The executable is bootstrapped through the remote shell; command
requests use a command ID and opaque `contents` on stdin, preserving
spaces, quotes and shell characters. Paths and loader selection are serialized;
display options stay local. Stdin paths are appended locally as remote paths. All input paths
and the default `.` refer to the remote machine.

Each loaded item is sent immediately and advances the local progress indicator.
Results feed the same local TUI, Console and Raw renderers, including filters,
columns, loader selection and nested declarations. The receiver validates the
protocol version, request GUIDs, item count/order, JSON framing, completion status
and process exit status. Successful files remain available when other files fail;
the local command ultimately returns failure. TUI provider identification submits `mod.source.identify` on this same session.
The remote host reads each file once and returns size, SHA-1 and requested MurmurHash2 fingerprints.
The shared identify pipeline queries both providers locally, using local proxies and API keys;
`--identify-parallelism` bounds their combined network concurrency. Local display and
matched-file browsing reuse the usual TUI. Exiting cancels pending identification.

A CLI list invocation owns its session until the local view closes. A caller that
needs multiple commands can retain one SSH child process and send concurrent
requests to its `mcpm remote` process; see [Persistent remote sessions](../remote.md).
No local daemon or per-command SSH handshake is required.

```sh
mcpm mod --ssh 'ssh -i key -p 2222 user@host' list mods/ --expanded --no-ui
mcpm --raw mod list 'C:\mods' --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm.exe' -C NML
```

## Remote support

Registered command ID: `mod.list`. The [Remote subsystem](../remote.md) accepts
structured paths and loader selection and streams one result per input item.
Filters, columns, expansion and layout remain local frontend settings.

Paths are processed in argument order. A directory contributes its immediate
`.jar` and `.jar.disabled` files, sorted by path; subdirectories and other
filenames are skipped. Scanning is not recursive. A file argument is inspected
directly and must have one of those suffixes. Suffix matching is case-sensitive.
Symbolic links to regular files and directories are followed.

Repeated paths are processed repeatedly. For example, a file supplied directly
and through its containing directory appears twice. Numbering follows the
combined displayed file sequence.

```sh
mcpm mod list first.jar mods/ second.jar other-mods/
mcpm mod list --expanded mods/ --loaders forge,neoforge
mcpm mod list --no-ui --width 100 mods/
mcpm mod list -- -leading-dash.jar
```

| Option | Description |
| --- | --- |
| `-F, --filter <FILTER>` | Filter rows using the shared [Filter type](filter.md); initializes the editable TUI Filter card. |
| `--no-ui` | Print the Console list instead of opening the TUI. |
| `--parallelism <COUNT>` | Maximum simultaneous outer-JAR inspections (default: 10; must be positive). Applies to all modes and is sent to the remote file host with SSH. |
| `--identify-parallelism <COUNT>` | Maximum simultaneous provider lookups in TUI (default: 10; must be positive). Ignored by Console and Raw. |
| `--expanded` | Show every mod declaration, including recognized nested mods. In terminal output, also list filenames skipped by loader filtering. |
| `--index-type <TYPE>` | Use `flat` (default) for full, left-aligned indices, or `tree` to align the first item after file numbers, with compact nesting and nested name indentation. |
| `--width <WIDTH>` | Override Console layout width, from 1 to 65535 columns. Ignored by TUI, raw or redirected output. |
| `--path-style <STYLE>` | Format nested archive paths as `inner`, `full` (default), or `outer`. See [Archive path styles](README.md#archive-path-styles). |
| `-C <LETTERS>` | Select columns using uppercase letters. Repeat or combine with the long options. |
| `--columns <COLUMNS>` | Select comma-separated column names. |
| `--column <COLUMN>` | Select one column. Repeat to select multiple columns. |

## Columns

| Letter | Name | Content |
| --- | --- | --- |
| `I` | `index` | Hierarchical declaration index, based on its file, loader, native Mod, and nested JAR position. |
| `N` | `name` | Mod display name, or the filename stem when unrecognized. |
| `M` | `modid` | Mod identifier. |
| `F` | `filename` | Outer filename, or the complete `outer-jar-path!inner-jar-path` chain for a nested archive. |
| `L` | `loaders` | Recognizing loaders and identification failure indicators. |

All columns are shown by default. Explicit selections are combined as a union
and displayed in the fixed order `I`, `N`, `M`, `F`, `L`, regardless of the order
of selection arguments.

```sh
mcpm mod list mods/ -C NML
mcpm mod list mods/ --columns name,filename --column loaders
mcpm --raw mod list mods/ -C MF
```

The default view uses the first recognized declaration as the file's summary,
with additional declaration counts and loader names. `--expanded` displays
each declaration separately, and adds rows for failed loader attempts.

## Declaration indices

All numeric positions start at **1**. Indices identify declarations within the
displayed file sequence:

1. Assign file number `X` in display order.
2. Count successful loader metadata records and failed loader attempts together.
   If this count is one, the successful metadata prefix is `X`. If it is greater
   than one, append the loader ID, such as `X-forge` or `X-neoforge`, including
   when only one loader succeeded and another failed. Loaders that did not
   recognize the file and produced no error do not contribute to this count.
3. For metadata prefix `Y`, a single directly declared Mod uses `Y`. Multiple
   directly declared Mods use `Y-1`, `Y-2`, and so on.
4. Child JAR metadata uses `Y+1`, `Y+2`, and so on, independently of native Mod
   numbering. Apply the same rules recursively within each child.

For example, `1-neoforge+2-3` is the third native Mod in the second child JAR of
the NeoForge record for file 1. `1-fabric+1+1` is a single Mod in a grandchild
JAR. A single Mod in the only loader record for file 144 is simply `144`.
Child positions follow declaration order, including retained library-only
JARs that produce no Mod rows. `--path-style` does not change indices.

The summary view retains the first displayed declaration's index. Expanded
output gives every Mod row its own index; failed loader rows use `X-loader`.

`--index-type flat` keeps these complete indices and aligns them to the left.
Loader names in either index style use the same Loader colors as the Loaders
column, without coloring the surrounding numeric indices.
`--index-type tree` uses `[X]` for files, `[loader]` when multiple loader records
exist, `[N]` when a JAR directly declares multiple Mods, and `<N>` for child
JAR positions. The first item after the file number starts in the same column,
whether it is a Loader label, native Mod number, or nested JAR number. File
labels are padded as needed for this alignment; other labels are not padded
to equal widths. Deeper shared ancestors use compact two-space indentation per level, without
reserving space for their full labels. Every file starts a new
hierarchy. A single loader or a single directly declared Mod needs no extra
label, following the same omission rules as flat indices.
Repeated ancestor positions are marked with a gray `·` rather than left blank,
keeping the same alignment and compact spacing. Raw and uncolored output retain
the guide characters without color.

For example, Forge and NeoForge records can produce these index labels:

```text
[1][forge][1]
·  · [2]
·  · <1>[1]
·  · · [2]
·  [neoforge]
```

The same rule applies when a file has only one Loader and several native Mods:

```text
[144][1]
·    [2]
·    <1>[1]
·    · [2]
```

Use `--expanded` to display the complete hierarchy; changing the index type
alone does not expand a summary. In tree mode, names of outer declarations
start at the Name column's normal position; each nested JAR adds exactly two
spaces before the name. Wrapped continuation lines keep that indentation.
Loader and native Mod numbering do not add name indentation.

Column selection, path styles, and raw output remain available with either
index type. Raw output preserves first-item alignment and compact nesting within the index field
and nested indentation within the Name field, without terminal padding between
the tab-separated columns.

```sh
mcpm mod list mods/ --expanded --index-type flat
mcpm mod list mods/ --expanded --index-type tree
```

Without loader selection, an archive that no loader recognizes is displayed as
`[unrecognized]`. With loader selection, files with no recognized declarations
and no identification errors are skipped. Failed identification is reported
instead of being treated as a filtered-out file.

Invalid or unreadable input paths are reported while other paths continue to
be processed. A directory scan that fails contributes no entries from that
directory. The command returns a failure status if an input path or loader
identification failed, even when other results were displayed.

For shared options, see [Loader selection](README.md#loader-selection). For colors, ignored-file hints, and `--raw`, see [Terminal output](../README.md#terminal-output).

[Command reference](../README.md) · [mod overview](README.md) · [简体中文](../../docs-zh/mod/list.md)
