# `mcpm mod`

Inspect local Minecraft mod archives without modifying them.

```text
mcpm mod <COMMAND>
```

| Command | Description |
| --- | --- |
| [`list`](list.md) | List mod declarations from files and directories. |
| [`info`](info.md) | Inspect metadata and dependencies in one or more files. |
| [`deps`](deps.md) | Show direct dependency declarations and their archive sources. |
| [`dep-graph`](dep-graph.md) | Build graphs, validate snapshots, or display a mod dependency tree. |
| [`source search`](source-search.md) | Search CurseForge / Modrinth projects with shared expressions. |

## Loader selection

`list`, `info`, `deps`, and `dep-graph build` accept these options:

| Option | Description |
| --- | --- |
| `--loader <LOADER>` | Select one loader. Repeat to select more than one. |
| `--loaders <LOADERS>` | Select comma-separated loaders. Repeat or combine with `--loader`. |

Accepted values are `forge`, `neoforge`, and `fabric`.
Without either option, all three loaders are attempted. Selections are combined
as a union, with duplicates removed. Loader records are displayed in built-in
order: Forge, NeoForge, then Fabric.

```sh
mcpm mod list mods/ --loader fabric --loader neoforge
mcpm mod info example.jar --loader forge --loaders neoforge,fabric
```

A single archive can be recognized by multiple loaders. Their records remain
separate even when their mod IDs match. Recognition means metadata could be
read; it does not establish that the mod will run with that loader.

## Archive path styles

`mod list` and `mod deps` accept `--path-style <STYLE>`. For a nested mod in
`mods/example.jar!META-INF/jars/child.jar!lib/grandchild.jar`:

| Style | Display |
| --- | --- |
| `inner` | `grandchild.jar` |
| `full` (default) | `example.jar!META-INF/jars/child.jar!lib/grandchild.jar` |
| `outer` | `example.jar` |

The option controls nested filenames in `list` and all `Source` values in
`deps`. Ordinary outer declarations in `list` keep their outer filename. It
changes path formatting only; inner mod declarations remain included.
For nested declarations, the outer component is always the JAR filename,
without its filesystem directory. Inner components retain their archive member
paths. For an outer declaration in `deps`, `full` retains the supplied file path.
`mod info` has no archive source field and does not accept this option.

```sh
mcpm mod list mods/ --expanded --path-style inner
mcpm mod list mods/ --expanded --path-style full
mcpm mod deps example.jar --path-style outer
```

In colored terminal output, each archive separator (`!`) uses bright white,
while archive paths use gray. Raw and redirected output retain the same path
text without colors.

[Command reference](../README.md) · [简体中文](../../docs-zh/mod/README.md)
