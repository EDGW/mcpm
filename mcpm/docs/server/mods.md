# Server mod inspection and caching

Server mod commands use the registered server's loader and mods directory. Local servers, explicit SSH connections and saved Remote workspaces share the same behavior.

- `--server PATH` selects the registered server; default: `.`.
- ITEM selection supports filenames, exact mod IDs and exact names, plus `--file`, `--modid`, `--name` and `--all`. Additional stdin lines use automatic matching; info/deps/cache also support `--input file|modid|name|unspecified`.
- File inputs are relative to the server's mods directory. `mods/` prefixes are accepted; parent traversal and absolute file inputs are rejected.
- With SSH, server metadata, file selection and cache paths resolve on the remote host. Output is rendered locally. Saved Remote workspaces select their stored connection automatically.
- Remote errors always appear. Remote logs require `--remote-logs` and follow `--log-level`.

## Info and dependencies

```sh
mcpm server mods info --server /srv/minecraft --all --parallelism 4
mcpm --ssh 'ssh user@host' server mods info --server /srv/minecraft --modid create
mcpm server mods deps --server /srv/minecraft --modid create
```

Both commands support Console and Raw output. `--parallelism` defaults to 10 and bounds full scans and lazy modid/name indexing. Multiple independent file inputs remain sequential.

Info queries the file host's default cache (`~/.mcpm/cache/mods`) by default:

- `--cache-dir PATH` selects an initialized cache on that host.
- `--no-cache` disables cache access; it conflicts with `--cache-dir`.
- Cache hits reuse available loader results; missing results are parsed directly. Queries do not import files or write metadata.
- Console displays lowercase fully cached, partially cached and total item counts, preceded by a blank line.

Deps retains direct identification without persistent cache lookup.

## List

```sh
mcpm --ssh 'ssh user@host' server mods list --server /srv/minecraft --parallelism 4
mcpm server mods list --server /srv/minecraft --modid create --expanded --cache-dir /srv/cache/mods
mcpm server mods list --server /srv/minecraft --no-cache --no-ui
```

- Modes: TUI, Console (`--no-ui`) and Raw.
- Empty selections list all files. Nested declaration selections retain their original archive identities.
- List uses the same default read-only cache policy, `--cache-dir` and `--no-cache` as info.
- Console and TUI display fully cached, partially cached and total item counts. TUI places these below the skipped-file summary; counts cover unique inspected files before display filtering.
- Filters, columns and layout stay local. TUI source lookup reuses the SSH session for remote file summaries and runs provider requests locally.

## Cache

```sh
mcpm server mods cache --server /srv/minecraft --all
mcpm server mods cache --server /srv/minecraft --modid create --cache-dir /srv/cache/mods
mcpm --ssh 'ssh user@host' server mods cache --server /srv/minecraft --file example.jar --mode hard-link
printf '%s\n' create jei | mcpm server mods cache --server /srv/minecraft --input modid
```

- Console shows progress and a final summary by default; `--expanded` (`-E`) adds per-file details. Errors always appear. Raw retains one structured record per selected file. Both reuse [`mod cache`](../mod/cache.md). Empty input prompts for a selection; use `--all` for the complete mods directory.
- Copies whole outer archives by default and stores metadata for the server's declared loader, including its complete nested tree. There are no loader override flags.
- `--cache-dir PATH` selects an initialized target cache; default: the file host's `~/.mcpm/cache/mods`.
- `--mode copy|hard-link|prefer-hard-link` controls publishing new objects. Hard links share content protection with the original file. Existing BLAKE3 objects are reused without linking the source or replacing the cached inode.
- Missing/outdated metadata is identified on cached bytes. `--force` reparses the declared loader while retaining other loader results when possible.
- `--parallelism COUNT` defaults to 10 and bounds selection scans and cache import/identification.
- Duplicate paths are processed once. Object counts deduplicate by BLAKE3. Console reports objects added/reused and file/selection failures. Raw emits one JSON object per selected outer file.
- File errors are reported and later files continue. Any failure gives a nonzero CLI exit status; completed writes remain. Cache storage follows [weak-consistency guarantees](../cache.md).

## Remote protocol

Clients and remote binaries should be upgraded together; older strict list clients may reject new completion fields.

- `server.mods.info` and `server.mods.deps` accept `server` (default `.`), `selection`, positive `parallelism` (default 10), and optional `cache`, using the shared [inspection event stream](../remote.md#mod-info-and-deps-contents).
- `server.mods.list` accepts the same fields. Events are `scope {loaders}`, `scan {completed,total}`, `error {error}`, ordered `item {index,record}`, and `done {files,input_errors,failed,cache}`. Records reuse mod-list records, with optional selected declaration keys (`records`, each containing `metadata` and `record` indices).
- For both queries, omitted/null `cache` disables lookup for protocol compatibility. CLI list/info send `{"kind":"default"}` by default, or `{"kind":"directory","path":"..."}` for an explicit root. Completion `cache` contains `fully_cached`, `partially_cached`, and `total`; consumers accept its omission from older remotes.
- List failure counts cover unique failed scanned files, including unselected index entries. Repeated selected rows do not increase those counts.
- `server.mods.cache` accepts `{server,selection,writing}`. `server` and `selection` are required; selection must be nonempty or `all:true`. `writing` accepts `cache_dir` (default null), `mode` (`copy`, `hard_link`, `prefer_hard_link`; default `copy`), `force` (default false), `contents` (`metadata`, `ident`; default both), and positive `parallelism` (default 10).
- Cache events use `{event,contents}`: `selection` wraps shared scan `progress`/`diagnostic` events; `progress` contains `{completed,total}`; `file` contains the same JSON record as Raw output; `done` contains `{files,failed_files,selection_failures,objects_added,objects_reused}`. File events retain selection order despite parallel completion. Import failure has null ID; metadata failure can retain a published object.
- Operation failures are represented in cache records and completion counts. A completed request may finish normally with nonzero failure counts; the CLI applies its nonzero exit policy. Handler/cancellation failures remain transport errors, and no completion is required after such errors.

## Cache result categories

`server mods cache` shares `--contents metadata,ident` (default both) and
`--sources modrinth,curseforge` (default all) with `mod cache`. `metadata` uses the
server's declared loader; `ident` queries the selected providers. Data is always
imported. With SSH/Remote, provider HTTP and API keys remain on the client;
verified source results are published on the file host through
`cache.mods.identification.write`. Identify remains read-only.
