# `mcpm mod list`

```text
mcpm mod list [OPTIONS] [PATH]...
```

List mod files, accepting any mixture of file and directory paths. With no
paths, the current directory (`.`) is used.

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
mcpm mod list --width 100 mods/
mcpm mod list -- -leading-dash.jar
```

| Option | Description |
| --- | --- |
| `--expanded` | Show every mod declaration, including recognized nested mods. In terminal output, also list filenames skipped by loader filtering. |
| `--index-type <TYPE>` | Use `flat` (default) for full, left-aligned indices, or `tree` to align the first item after file numbers, with compact nesting and nested name indentation. |
| `--width <WIDTH>` | Override terminal layout width, from 1 to 65535 columns. Ignored for raw or redirected output. |
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
