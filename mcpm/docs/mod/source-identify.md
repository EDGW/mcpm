# `mcpm mod source identify`

Identify files by their contents on a configured provider in Console, Raw or SSH Remote mode:

```sh
mcpm mod source identify --source modrinth --parallelism 4 a.jar b.jar
mcpm --raw mod source identify --source curseforge a.jar b.jar
mcpm --ssh 'ssh user@host' mod source identify --source modrinth /srv/mods/a.jar /srv/mods/b.jar
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm.exe' mod source identify --source curseforge 'D:\mods\a.jar'
```

`--source` accepts `modrinth`, `curseforge` (Curse.tools), or `curseforge@KEY` (official API). Inputs are literal file paths, with one additional path per stdin line. With SSH, paths, file reads and digest computation belong to the remote host. Only size, SHA-1, requested MurmurHash2 fingerprints and read errors cross the connection; JAR bytes stay remote. Provider requests and match verification run locally, using local proxies and API keys.

`-p/--parallelism <COUNT>` must be positive and defaults to `10`, sharing the default with `mod list --identify-parallelism`. Completion immediately advances the stderr progress bar, using the mod-list style. Rows and per-file errors retain input order even when lookups finish out of order. Use `1` for sequential checks. Duplicate physical paths share a lookup but retain each original output position. `--raw` disables progress. Redirected stdout emits TSV; stderr independently controls progress visibility.

Unmatched files produce `unmatched` rows, verified matches produce `matched` rows. Per-file read/request errors do not stop later inputs and result in a nonzero exit status. Errors preserve individual `caused by:` contexts; remote file errors are displayed as `remote error:` only with the global `--remote-logs` option, and local provider failures use `error:` regardless of that option.

Default columns are index, path, source, project, id, filename, size, role and status. Select columns with `--columns` or `-C`. Presentation options remain local.

The Remote command ID is `mod.source.identify`. Standalone identification and mod-list TUI lookups share one scheduler. An SSH list acquires summaries on its existing session and queries providers locally through this same scheduler. Each file is read once for both providers. `--identify-parallelism` bounds both providers together. Exiting cancels local queries and unfinished remote reads. See the [remote protocol](../remote.md).

`mcpm server mods source identify` shares local parallelism and ordered output, but server commands do not accept SSH options yet.
