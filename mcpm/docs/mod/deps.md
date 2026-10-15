# `mcpm mod deps`

```text
mcpm mod deps [OPTIONS] <FILE>...
```

Show direct dependency declarations for every recognized mod in one or more
`.jar` or `.jar.disabled` files. Files are processed in argument order;
directories are not accepted. Output is grouped by outer file, loader, and mod,
and includes each mod's version and declaring archive path.

Use `--path-style inner|full|outer` to change the `Source` path display. The
default is `full`; see [Archive path styles](README.md#archive-path-styles).

```sh
mcpm mod deps example.jar
mcpm mod deps example.jar another.jar --loader neoforge
mcpm --raw mod deps example.jar > dependencies.txt
```

Nested sources use `!` between archive levels, retaining the outer filename and
each inner archive member path. The same format is used in the `list` filename
column for nested mods. For example:

```text
example.jar!META-INF/jars/child.jar!META-INF/jars/grandchild.jar
```

Relations retain the categories `required`, `optional`, `recommended`,
`suggested`, `breaks`, and `conflicts`. Each declaration references one target.
Version conditions use parentheses with `&&` for conjunction and `||` for
alternatives. The `side`, `ordering`, and `reason` fields appear in brackets.
Ordering describes the declaring mod relative to the target.

```text
required: render_api: (>=1 && <2)
optional: client_api: >=2 [side=client; ordering=after target]
breaks: legacy_api: <1
```

These are declarations, not resolved dependencies. The command does not search
for installed candidates, evaluate version compatibility, or determine which
nested candidates would be loaded. `--tree` and `--dir` are not implemented.

If a loader fails, successful records are retained and the file is marked
`Identification: incomplete`, with failed loader IDs listed. The command
returns a failure status and continues with other files. Unrecognized files
without a loader filter are errors; unrecognized files with a filter are
skipped. `deps` currently does not print an ignored-file hint.

For shared options, see [Loader selection](README.md#loader-selection). For colors, ignored-file hints, and `--raw`, see [Terminal output](../README.md#terminal-output).

[Command reference](../README.md) · [mod overview](README.md) · [简体中文](../../docs-zh/mod/deps.md)
