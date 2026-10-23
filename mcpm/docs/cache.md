# Mod cache

`mcpm cache mods` explicitly stores files under `~/.mcpm/cache/mods`.
Mod info/list query this cache by default. `--cache-dir` selects another cache;
`--no-cache` disables cache lookup. Inspections do not import files or save identification results. Downloads and source searches do
not automatically use it. All commands support Console and Raw. Redirected stdout
or `--raw` produces one JSON object per line; an empty `list` produces no rows.
Cache management commands are local and do not support SSH.

Use [`mcpm mod cache`](mod/cache.md) to select files by filename/ID/name or stdin and store both files and identification metadata. For registered servers, [`server mods cache`](server/mods.md#cache) inherits the server scope and loader and supports SSH/Remote workspaces; server mods list/info also default to cache lookup on the file host.

## Usage

```sh
mcpm cache mods init --directory /path/to/cache
mcpm cache mods import example.jar another.jar
mcpm cache mods import example.jar --mode hard-link
mcpm cache mods list
mcpm cache mods info <BLAKE3>
mcpm cache mods refresh <BLAKE3> --loader fabric
mcpm cache mods refresh <BLAKE3> --force
mcpm cache mods verify <BLAKE3>
mcpm cache mods export <BLAKE3> restored.jar another-copy.jar
mcpm cache mods export <BLAKE3> linked.jar --mode hard-link
mcpm cache mods remove <BLAKE3>
mcpm cache mods rebuild-index
```

Use `--directory PATH` before or after a mods subcommand to select another cache.
The default root initializes when absent or empty. Explicit roots must already be
initialized. Nonempty unmarked roots, malformed manifests and unsupported versions
are rejected. Directory format version is 2; version 1 GUID caches are not migrated
or adopted automatically. Select a new directory to retain an old cache.

Objects use their complete 64-character lowercase BLAKE3 as their ID. Equal BLAKE3
means the same content and one object. Import defaults to copying; it does not
identify mods. New objects initially have `missing_digest`, and can be exported.

Import accepts multiple files. Export accepts one BLAKE3 followed by one or more
destination paths. Both process inputs in order, emit a record per successful item,
and stop at the first failure. Earlier successful items remain; batches do not roll back.

| Command | Behavior |
| --- | --- |
| `init` | Initialize an absent or empty directory; accept an already valid cache |
| `list` | Show complete and unavailable objects without parsing JARs |
| `info BLAKE3` | Console: File, Size, SHA-256, then recognized loaders’ declarations in `mod info` format; Raw: complete cache record |
| `refresh BLAKE3` | Identify missing/outdated results; all built-in loaders by default |
| `refresh BLAKE3 --force` | Reparse selected loaders even when current results exist |
| `verify BLAKE3` | Check object contents and metadata; `--structure-only` skips full hashing |
| `export BLAKE3 PATH...` | Export to absent destinations outside the cache; parents must exist |
| `remove BLAKE3` | Delete cache names only; report whether anything was removed |
| `rebuild-index` | Rebuild the optional imohash candidate index from stored objects |

`refresh` accepts the same `--loader` / comma-separated `--loaders` selection as
mod inspection. Loader failures leave the previous descriptor intact. Raw
`digest.loaders` distinguishes unchecked loaders (absent key), checked but
unrecognized files (`null`), and recognized ModMeta snapshots (objects).
Non-recognition is reusable; `--force` rechecks it.

## Import and hard links

- Import checks content before choosing a transfer strategy. An existing valid
  object is reused without replacing it, staging files, attempting hard links or
  changing source permissions. Reimporting an independent copy does not replace
  the source or link it to the existing object.
- Mode applies only when an object must be added: `--mode copy` is the default;
  `hard-link` requires a hard link; `prefer-hard-link` falls back to copying only
  for filesystem capability failures. Even `hard-link` can reuse an existing
  object when the source is on another filesystem.
- Raw import records contain `published`: true for a newly published object,
  false for reuse. `method` is `copy` or `hard_link` for publication, and `null`
  for reuse. Console displays added/reused and the method when applicable.
- Hard-link imports and exports share content and write protection. Update a linked
  file by writing a new file and replacing your filename, rather than overwriting,
  appending or truncating it. Protection also applies to a hard-link import source.
- Exports never overwrite a destination or create its parents. Removing an object
  leaves exported copies and hard links intact. There is no automatic eviction or
  backup-reference tracking.

## Availability and concurrency

- `available`: valid identification digest, including explicit non-recognition;
  at least one loader has been checked.
- `not_found`: neither data nor descriptor exists.
- `unavailable`: missing descriptor, invalid/incompatible data, or a read failure.
- `info` presents these as normal query results, with a Console explanation or
  structured Raw status. `verify` fails for unavailable or missing objects.
- `refresh` creates a missing digest. Invalid content descriptors are rejected;
  outdated parser results can be refreshed.
- Full verification and export check content against the BLAKE3 ID, including
  objects without an identification digest. Structural checks cannot detect
  same-size content changes. A missing digest still has `missing_digest` status.
- Cache completeness is best effort: concurrent writes may be lost or disappear.
  Every result accepted for reuse must still be correct. Data is immutable after
  publication; metadata is published as a complete atomic snapshot.
- There is no global reader/writer lock. Concurrent deletion may cause a miss;
  concurrent publication can recreate an object after deletion. Successful import
  describes the operation, not a guarantee of future persistence.
- The imohash index is a rebuildable acceleration structure. Missing/stale entries
  do not prove a content match; lookup validates candidates and falls back to full
  BLAKE3. Index recovery uses a separate nonblocking coordination lock; SQLite
  transactions and initialization may synchronize internally. Busy index writes
  can be skipped without blocking object or metadata operations.

[Command reference](README.md) · [简体中文](../docs-zh/cache.md)
