# `mcpm mod info`

```text
mcpm mod info [OPTIONS] <FILE>...
```

Inspect one or more `.jar` or `.jar.disabled` files in argument order.
Directories are not accepted. Output includes the outer file path, enabled or
disabled status inferred from the filename, loader records, mod names, IDs,
versions, and declared dependencies. Recognized nested mods are included.

```sh
mcpm mod info example.jar
mcpm mod info example.jar another.jar.disabled --loaders fabric,quilt
mcpm --raw mod info example.jar > info.txt
```

A file unrecognized by every loader is an error when no loader filter is
specified. With a filter, an unrecognized file is skipped. Loader failures are
reported to stderr; successful records are still displayed, but the command
returns a failure status. Processing continues with subsequent files.

For shared options, see [Loader selection](README.md#loader-selection). For colors, ignored-file hints, and `--raw`, see [Terminal output](../README.md#terminal-output).

[Command reference](../README.md) · [mod overview](README.md) · [简体中文](../../docs-zh/mod/info.md)
