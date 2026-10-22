# `mcpm mod info`

```text
mcpm mod info [OPTIONS] [INPUT]...
```

Inspect one or more `.jar` or `.jar.disabled` files in argument order.
Directories are not accepted. Output includes the outer file path, enabled or
disabled status inferred from the filename, loader records, mod names, IDs,
versions, and declared dependencies. Recognized nested mods are included.

```sh
mcpm mod info example.jar
mcpm mod info example.jar another.jar.disabled --loaders fabric,neoforge
mcpm --raw mod info example.jar > info.txt
```

A file unrecognized by every loader is an error when no loader filter is
specified. With a filter, an unrecognized file is skipped. Loader failures are
reported to stderr; successful records are still displayed, but the command
returns a failure status. Processing continues with subsequent files.

Console and Raw modes are supported; use `--raw` or redirect stdout for Raw.
SSH uses the same modes and local formatting:

```sh
mcpm --ssh 'ssh user@host' mod info --directory /srv/mods --modid example
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'D:\mcpm.exe' --raw mod info --directory 'D:\mods' --all
```

Inputs match filenames, then exact mod IDs, then exact names. Use repeated
`--file`, `--modid`, or `--name` selectors to choose explicitly, or `--all` to
inspect the directory. `--directory` defaults to `.`. Stdin lines append in
order; `--input file|modid|name|unspecified` controls their interpretation.
`--all` cannot be combined with queries. With SSH, selection, filesystem access,
and loader parsing happen on the remote host; stdin is consumed locally before
the request. Colors and dependency path styles stay local. Remote diagnostics
follow `--remote-logs`; failures still produce a nonzero exit status.
`--parallelism <COUNT>` defaults to 10 and bounds file reads for `--all` and
lazy directory indexes used by name/ID queries, locally or over SSH. Multiple
independent file inputs are processed sequentially and are unaffected by this
option. Scan progress is reported after each completion, including failures and
unrecognized files; stderr TTY detection controls the local progress display.

Both endpoints require protocol version 3. See [Remote](../remote.md) for
structured `mod.info` requests.

For shared options, see [Loader selection](README.md#loader-selection). For colors, ignored-file hints, and `--raw`, see [Terminal output](../README.md#terminal-output).

[Command reference](../README.md) · [mod overview](README.md) · [简体中文](../../docs-zh/mod/info.md)
