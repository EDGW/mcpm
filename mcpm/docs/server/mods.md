# Server mod inspection and caching

Server mod commands use the registered server's loader and mods directory. Local servers, explicit SSH connections and saved Remote workspaces share the same behavior.

- `--server PATH` selects the registered server; default: `.`.
- ITEM selection supports filenames, exact mod IDs and exact names, plus `--file`, `--modid`, `--name` and `--all`. Additional stdin lines use automatic matching; info/deps/cache also support `--input file|modid|name|unspecified`.
- File inputs are relative to the server's mods directory. `mods/` prefixes are accepted; parent traversal and absolute file inputs are rejected.
- With SSH, server metadata, file selection and cache paths resolve on the remote host. Output is rendered locally. Saved Remote workspaces select their stored connection automatically.
- Cache resources come from the Server runtime environment. They currently share the file host’s global cache (`~/.mcpm/cache/mods`); per-server cache settings are not yet available.
- Remote errors always appear. Remote logs require `--remote-logs` and follow `--log-level`.

## Info and dependencies

```sh
mcpm mods info --server /srv/minecraft --all --parallelism 4
mcpm --ssh 'ssh user@host' mods info --server /srv/minecraft --modid create
mcpm mods deps --server /srv/minecraft --modid create
```

Both commands support Console and Raw output. `--parallelism` defaults to 10 and bounds full scans and lazy modid/name indexing. Multiple independent file inputs remain sequential.

Info queries the Server runtime’s Mod cache by default:

- `--no-cache` disables cache access for this operation.
- Cache hits reuse available loader results; missing results are parsed directly. Queries do not import files or write metadata.
- Console displays lowercase fully cached, partially cached and total item counts, preceded by a blank line.

Deps retains direct identification without persistent cache lookup.

## List

```sh
mcpm --ssh 'ssh user@host' mods list --server /srv/minecraft --parallelism 4
mcpm mods list --server /srv/minecraft --modid create --expanded
mcpm mods list --server /srv/minecraft --no-cache --no-ui
```

- Modes: TUI, Console (`--no-ui`) and Raw.
- Empty selections list all files. Nested declaration selections retain their original archive identities.
- List uses the same Server cache and read-only query policy as info, including `--no-cache`. TUI source identification inherits the same effective cache target.
- Console and TUI display fully cached, partially cached and total item counts. TUI places these below the skipped-file summary; counts cover unique inspected files before display filtering.
- Filters, columns and layout stay local. TUI source lookup reuses the SSH session for remote file summaries and runs provider requests locally.

## Cache

```sh
mcpm mods cache --server /srv/minecraft --all
mcpm mods cache --server /srv/minecraft --modid create --metas meta
mcpm --ssh 'ssh user@host' mods cache --server /srv/minecraft --file example.jar --mode hard-link
printf '%s\n' create jei | mcpm mods cache --server /srv/minecraft --input modid
```

- Console shows progress and a final summary by default; `--details` adds per-file details. Errors always appear. Raw retains one structured record per selected file. Both reuse [`mod cache`](../mod/cache.md). Empty input prompts for a selection; use `--all` for the complete mods directory.
- Copies whole outer archives by default and stores metadata for the server's declared loader, including its complete nested tree. There are no loader override flags.
- Imports and source-result publication use the Server runtime’s Mod cache.
- `--mode copy|hard-link|prefer-hard-link` controls publishing new objects. Hard links share content protection with the original file. Existing BLAKE3 objects are reused without linking the source or replacing the cached inode.
- Missing/outdated metadata is identified on cached bytes. `--force` reparses the declared loader while retaining other loader results when possible.
- `--parallelism COUNT` defaults to 10 and bounds selection scans and cache import/identification.
- Duplicate paths are processed once. Object counts deduplicate by BLAKE3. Console reports objects added/reused and file/selection failures. Raw emits one JSON object per selected outer file.
- File errors are reported and later files continue. Any failure gives a nonzero CLI exit status; completed writes remain. Cache storage follows [weak-consistency guarantees](../cache.md).

## Remote protocol

Protocol version 4 requires matching client and remote binaries. Server requests reject the former `cache` directory override and `writing.cache_dir` fields.

- `server.mods.info` and `server.mods.deps` accept `server` (default `.`), `selection`, positive `parallelism` (default 10), and `no_cache` (default false), using the shared [inspection event stream](../remote.md#mod-info-and-deps-contents).
- `server.mods.list` accepts the same fields. Events are `scope {loaders,environment}`, `scan {completed,total}`, `error {error}`, ordered `item {index,record}`, and `done {files,input_errors,failed,cache}`. Records reuse mod-list records, with optional selected declaration keys (`records`, each containing `metadata` and `record` indices).
- Info/list use the Server runtime cache unless `no_cache:true`; deps always identifies directly. Completion `cache` contains `fully_cached`, `partially_cached`, and `total`. A list scope’s `environment` carries `{cache}` with the host-resolved directory selection or null when disabled. Subsequent TUI lookups reuse this target without opening remote paths locally.
- `server.mods.source.identify` accepts `{server,selection,no_cache,parallelism}` and returns scan/error events followed by `done {paths,failed,environment}`. The client uses this effective environment for read-only provider identification. Provider specifications and credentials stay local.
- List failure counts cover unique failed scanned files, including unselected index entries. Repeated selected rows do not increase those counts.
- `server.mods.cache` accepts `{server,selection,writing}`. `server` and `selection` are required; selection must be nonempty or `all:true`. `writing` accepts `mode` (`copy`, `hard_link`, `prefer_hard_link`; default `copy`), `metas` (optional `meta`/`ident` list; default both), `meta` (additional list), `no_metas` (default false), `force` (default false), and positive `parallelism` (default 10). Explicit meta selections replace the default pair; `no_metas` conflicts with both lists.
- The first cache data payload is `{cache}` with the host-resolved Mod-cache directory. Later lookup and identification publication reuse this target. Subsequent cache events use `{event,contents}`: `selection` wraps shared scan `progress`/`diagnostic` events; `progress` contains `{stage,completed,total}` for import, metadata, or ident; `stage_done` contains `{stage,success}`; `file` contains the same JSON record as Raw output; `done` contains `{files,failed_files,selection_failures,objects_added,objects_reused}`. File events retain selection order despite parallel completion. Import failure has null ID; metadata failure can retain a published object.
- Operation failures are represented in cache records and completion counts. A completed request may finish normally with nonzero failure counts; the CLI applies its nonzero exit policy. Handler/cancellation failures remain transport errors, and no completion is required after such errors.

## Cache result categories

`server mods cache` shares `--metas meta,ident` (default both) and
`--sources modrinth,curseforge` (default all) with `mod cache`. `meta` uses the
server's declared loader; `ident` queries the selected providers. Data is always
imported. With SSH/Remote, provider HTTP and API keys remain on the client;
verified source results are published on the file host through
`cache.mods.identification.write`. Identify remains read-only.
