# `mcpm-tools mods info`

```text
mcpm-tools mods info [OPTIONS] [INPUT]...
```

Inspect one or more `.jar` or `.jar.disabled` files in argument order.
Directories are not accepted. Output includes the outer file path, enabled or
disabled status inferred from the filename, loader records, mod names, IDs,
versions, and declared dependencies. Recognized nested mods are included.

```sh
mcpm-tools mods info example.jar
mcpm-tools mods info example.jar another.jar.disabled --loaders fabric,neoforge
mcpm-tools --raw mods info example.jar > info.txt
```

A file unrecognized by every loader is an error when no loader filter is
specified. With a filter, an unrecognized file is skipped. Loader failures are
reported to stderr; successful records are still displayed, but the command
returns a failure status. Processing continues with subsequent files.

Console and Raw modes are supported; use `--raw` or redirect stdout for Raw.
SSH uses the same modes and local formatting:

```sh
mcpm-tools --ssh 'ssh user@host' mods info --directory /srv/mods --modid example
mcpm-tools --ssh 'ssh user@windows' --ssh-mcpm 'D:\mcpm-tools.exe' --raw mods info --directory 'D:\mods' --all
```

Inputs match filenames, then exact mod IDs, then exact names. Use repeated
`--file`, `--modid`, or `--name` selectors to choose explicitly, or `--all` to
inspect the directory. `--directory` defaults to `.`. Stdin lines append in
order; `--input file|modid|name|unspecified` controls their interpretation.
`--all` cannot be combined with queries. With SSH, selection, filesystem access,
and loader parsing happen on the remote host; stdin is consumed locally before
the request. Colors and dependency path styles stay local. Remote errors are
always displayed; remote logs require `--remote-logs` and follow `--log-level`.
Failures produce a nonzero exit status.
`--parallelism <COUNT>` defaults to 10 and bounds file reads for `--all` and
lazy directory indexes used by name/ID queries, locally or over SSH. Multiple
independent file inputs are processed sequentially and are unaffected by this
option. Scan progress is reported after each completion, including failures and
unrecognized files; stderr TTY detection controls the local progress display.

Both endpoints require protocol version 3. See [Remote](../remote.md) for
structured `mod.info` requests.

For shared options, see [Loader selection](README.md#loader-selection). For colors, ignored-file hints, and `--raw`, see [Terminal output](../README.md#terminal-output).

## Identification cache

- Caching is enabled by default using `~/.mcpm/cache/mods`. `--cache-dir PATH` selects an explicitly initialized directory; `--no-cache` disables cache lookup and identifies files directly. The two options are mutually exclusive.
- Default roots initialize when absent or empty. Explicit roots must already be valid; run `mcpm-tools cache mods init --directory PATH` first. Nonempty unmarked directories, malformed manifests and unsupported directory versions fail.
- Lookup uses an imohash candidate index and current native file identity for hard links. If no identity matches, full BLAKE3 lookup also permits independent copies with identical contents to reuse metadata.
- Cached non-recognition is reused too. Missing, stale or invalid results are identified normally. Inspection only reads existing cache objects: it never imports files, establishes hard links or writes metadata. Use [mods cache](cache.md), or explicit cache import and refresh commands, to store results.
- Console shows `A fully cached · B partially cached · C total items` after a blank line; list TUI shows the same totals below its skipped-files status line. Full means every requested loader result came from cache, including non-recognition; partial means some did. Totals count unique normalized file inspections, including unrecognized/failed files, before filtering; duplicate queries and nested declarations do not increase them. With caching disabled, both hit counts are zero. Raw text output has no summary line.
- With SSH, cache selection and paths are interpreted on the remote host. Ordinary local cache-management commands remain local.

```sh
mcpm-tools mods info example.jar
mcpm-tools mods info example.jar --no-cache
mcpm-tools mods info example.jar --cache-dir /path/to/cache
```

[Command reference](../README.md) · [mod overview](README.md) · [简体中文](../../docs-zh/mod/info.md)
