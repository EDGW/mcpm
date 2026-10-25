# `mcpm mod source identify`

Identify files by their contents on a configured provider in Console, Raw or SSH Remote mode:

```sh
mcpm mod source identify --source modrinth --parallelism 4 a.jar b.jar
mcpm --raw mod source identify --source curseforge a.jar b.jar
mcpm --ssh 'ssh user@host' mod source identify --source modrinth /srv/mods/a.jar /srv/mods/b.jar
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm.exe' mod source identify --source curseforge 'D:\mods\a.jar'
```

`--source` accepts `modrinth`, `curseforge` (Curse.tools), or `curseforge@KEY` (official API). Inputs are literal file paths, with one additional path per stdin line. With SSH, paths, file reads and digest computation belong to the remote host. Size, SHA-1, requested MurmurHash2 fingerprints, validated cache matches and read errors cross the connection; JAR bytes stay remote. Provider requests and match verification run locally, using local proxies and API keys.

`-p/--parallelism <COUNT>` must be positive and defaults to `10`, sharing the default with `mod list --identify-parallelism`. Completion immediately advances the stderr progress bar, using the mod-list style. Rows and per-file errors retain input order even when lookups finish out of order. Use `1` for sequential checks. Duplicate physical paths share a lookup but retain each original output position. `--raw` disables progress. Redirected stdout emits TSV; stderr independently controls progress visibility.

Unmatched files produce `unmatched` rows, verified matches produce `matched` rows. Multiple verified candidates without a unique match also produce `unmatched` and do not save a new identification. Per-file read/request errors do not stop later inputs and result in a nonzero exit status. Errors preserve individual `caused by:` contexts; remote file errors always use `remote error:`, and local provider failures use `error:`. Neither depends on `--remote-logs` or `--log-level`.

- Console displays labeled field groups separated by blank lines. Unmatched files omit unavailable match details.
- Raw emits one TSV row per input. Unavailable match fields are empty; controls and literal backslashes are escaped.
  Headers are omitted by default; `--headers` adds one header row in Raw only.
- Default columns are index, path, source, project, id, filename, size, role and status.
  Combine `--columns A,B`, repeated `--column A`, or `-C IPOJFABRS` to replace the default selection.
  The compact letters correspond to the default columns in order. Columns cannot be empty.
- Presentation options remain local, including when using SSH.

The Remote command ID is `mod.source.identify`. Standalone identification and mod-list TUI lookups share one scheduler. An SSH list acquires summaries on its existing session and queries providers locally through this same scheduler. Each file is read once for both providers. `--identify-parallelism` bounds both providers together. Exiting cancels local queries and unfinished remote reads. See the [remote protocol](../remote.md).

`mcpm server mods source identify` shares local parallelism and ordered output, including SSH and registered Remote routing.

## Read-only cache

- Default: query the file host's `~/.mcpm/cache/mods` before provider requests.
- Standalone `mod source identify`: `--cache-dir PATH` chooses an initialized cache on that host; `--no-cache` disables lookup. They conflict.
- `server mods source identify` uses the Server runtime’s Mod cache and accepts `--no-cache` to disable lookup for this operation.
- A hit requires cached content file, a valid `ident`, and matches validated against the queried file summary. Loader metadata can be missing or corrupt.
- A miss queries the provider without writing anything. Use `mod cache --metas ident` or `server mods cache --metas ident` to populate results explicitly.
- Official API keys and Curse.tools reuse the same `curseforge` results. Credentials are never persisted or sent over SSH.
- Standalone/server Identify and mod-list TUI share this behavior; list TUI inherits its list command's cache selection.
