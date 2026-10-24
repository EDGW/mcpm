# `mcpm mod cache`

Explicitly store selected mod archives and their identification metadata.

```text
mcpm mod cache [OPTIONS] [INPUT]...
```

Supports Console and Raw. Redirected stdout or `--raw` emits one JSON object per
selected file. This command is local; SSH options are rejected.

## Select files

Use the same input rules as [mod info](info.md):

- Positional inputs match a filename, then an exact mod ID, then an exact mod name.
- Repeat or mix `--file FILE`, `--modid ID`, and `--name NAME`; queries preserve
  their command-line order.
- `--directory PATH` selects the source directory, defaulting to `.`. Relative
  filenames resolve there; name/ID queries search that directory.
- `--all` selects all `.jar` and `.jar.disabled` files directly in the source
  directory. It cannot be combined with other queries, including stdin.
- Redirected stdin appends one query per line. `--input file|modid|name|unspecified`
  controls only stdin; `unspecified` uses filename/ID/name matching.
- A nested declaration selects its complete outer JAR, including all declarations
  and nested metadata for the requested loaders.
- Repeated selections of the same normalized path are processed once, in their
  first-selection position. Different files with identical content share one
  BLAKE3 cache object.

```sh
mcpm mod cache example.jar another.jar.disabled
mcpm mod cache --directory ./mods --modid zeta
mcpm mod cache --directory ./mods --name 'Example Mod'
mcpm mod cache --directory ./mods --all --parallelism 4
printf '%s\n' example.jar another.jar | mcpm mod cache --input file
```

A positional directory is not a file input; use `--directory PATH --all`.

## Storage and identification

| Option | Behavior |
| --- | --- |
| `--cache-dir PATH` | Target cache, separate from the source `--directory` |
| `--mode copy` | Default: copy new content without linking the source |
| `--mode hard-link` | New objects require a hard link; source content becomes write-protected too |
| `--mode prefer-hard-link` | Attempt a hard link; copy only on filesystem capability failures |
| `--loader` / `--loaders` | Select loaders; all built-ins by default |
| `--metas meta,ident` / `--meta META` | Default both; explicit comma-separated or repeated selections replace defaults |
| `--no-metas` / `--metas ,` | Import content without calculating either metadata kind |
| `--sources modrinth,curseforge` / `--source SOURCE` | Default both; explicit selections replace defaults; `curseforge@KEY` is supported |
| `--force` | Refresh every requested loader/source result even when cached |
| `--parallelism COUNT` | Maximum files in parallel during name/ID scanning and import/identification; default 10 |

The default target is `~/.mcpm/cache/mods`, initialized when absent or empty.
Explicit targets must already be valid; initialize them with
`mcpm cache mods init --directory PATH`. Version 1 caches are not migrated.

Existing content is reused without replacing its identity or linking another
source to it. Import mode applies when creating an object. Hard links share bytes
and permissions; update linked files by replacing their names, never by
overwriting, truncating or appending. A losing concurrent hard-link publisher can
leave source content protection applied.

After importing, identify the cached bytes and fill missing or outdated results.
Already checked current results are retained unless `--force` requests a refresh.
A successful unrecognized Loader result is cached explicitly and counts as success. Source identification persists only verified matches; unmatched/error results are not saved.
Selecting by ID/name can require a discovery scan before cached-byte
identification; scan metadata is not directly persisted.

```sh
mcpm cache mods init --directory /path/to/cache
mcpm mod cache example.jar --cache-dir /path/to/cache --loader fabric
mcpm mod cache example.jar --mode hard-link
mcpm mod cache example.jar --force
```

[cache mods import](../cache.md) remains a lower-level path-based import without
identification. `mod info/list` reuse existing metadata by default (`--no-cache` disables this); they do not
store files or identification results.

Both categories require the same cached content file. Loader results use `meta`; source
matches use `ident`. Either may be absent or unusable independently.
All CurseForge configurations share one cache namespace, without keys/endpoints.
`--metas ident` imports the data but does not create Loader metadata. Provider
requests use one client runtime for the complete batch.

```sh
mcpm mod cache --directory ./mods --all --metas meta
mcpm mod cache example.jar --meta ident --source modrinth
mcpm mod cache example.jar --no-metas
```

## Results and failures

- Results are emitted in selection order; worker completion order may differ.
- One file failure does not stop later files. Completed writes are retained.
- An import failure has no successful import result. A metadata failure may leave
  an imported cached content file without usable metadata; its record retains the BLAKE3,
  publication result and observed availability.
- Recognition failures are not cached as unrecognized results.
- Query and discovery-scan errors are reported on stderr and contribute to failure.
  Any file or selection failure makes the command exit nonzero.
- Console shows separate import, meta, and ident progress bars for selected stages. Each bar disappears when its stage ends; a successful stage then prints a complete sentence. `--details` adds
  per-file source path, BLAKE3, added/reused, transfer method and metadata status.
  Errors always appear. After a blank line, a lowercase summary separates file
  totals from unique added/reused objects. An object added during this command is
  counted only as added. File failures and selection/discovery failures are separate
  stage counts; the same file can fail during both discovery and storage.
- Raw emits JSON fields `path`, `id`, `published`, `method`, `status`, `reason`,
  `data_path`, `digest_path`, `metadata_requested`, `identification`, `success`, `failed_stage`, `error`, and `error_remote`. `identification` maps each requested source to matched/unmatched/failed, with individual errors and origins. A reused
  object's method is null. Import failures have no ID. Raw has no summary row.

The cache allows missing writes and lost concurrent metadata updates, while every
usable result must be correct. It is not a transactional backup store.


For registered Local or Remote servers, use [`server mods cache`](../server/mods.md#cache) to inherit the server mods scope and declared loader.

[Command reference](../README.md) · [简体中文](../../docs-zh/mod/cache.md)
