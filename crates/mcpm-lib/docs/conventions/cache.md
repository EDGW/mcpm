# Mod and source cache conventions

All content, directory management, atomic publication, transfers and imohash lookup
are owned by `local::cache`. See [Generic local cache](local-cache.md) for the version
3 directory protocol and public storage API.

## Responsibilities and selection

- `local::cache::mods::ModsCacheDir` is the Mod cache entry point, privately wrapping the generic `CacheDir`.
- Generic caching owns content and named metadata; `meta` and `ident` are Mod-specific metadata names.
- Import stores content only. `ModsCacheDir::verify` checks content structure/BLAKE3 without interpreting metadata.

- `local::cache::mods::meta` owns Loader snapshots and Mod refresh policy.
- `local::cache::mods::ident` owns source `FileMatch` snapshots and source refresh policy.
- Both use `CacheDir` and typed `Codec` operations. Neither implements file storage,
  directory naming, permissions, hard-link policy or an independent quick index.
- Every persisted metadata type implements `Codec`: `CacheDirMetadata`, `CacheDigest`
  and the source snapshot. JSON interpretation occurs only in the corresponding codec.
- `ModsCacheSelection::Default.open()` initializes an absent/empty `~/.mcpm/cache/mods`.
  `Directory { path }.open()` requires an already initialized valid root. The default
  location is retained, but version 1/2 roots are never silently adopted or migrated.

## Runtime resources

- `local::runtime::RuntimeEnvironment` composes operation resources above the cache layer. Storage and metadata codecs do not depend on it.
- `new()` performs no filesystem access. `cache.global`, `cache.mods`, `cache.backup` and `cache.stash` initially share one `Arc<RuntimeCache>`; only `mods` is optional.
- `RuntimeCache` retains its `ModsCacheSelection` and opens through that owner on first use. Successful opens are shared by environment clones; failed opens retain their error context and can be retried.
- `with_mods_cache(None)` disables Mod queries with no fallback. `require_mods_cache()` rejects explicit writes when disabled. Backup creation uses `cache.backup`.
- `MinecraftServer::runtime()` borrows the Server-owned environment. It currently supplies the global defaults without reading or writing per-server settings.
- Standalone Mod command adapters select resources from existing CLI options. Server adapters use the Server environment and apply only operation policy, such as `--no-cache`.
- Cache management remains an explicit `ModsCacheDir` API. Runtime resources do not change query/write guarantees or metadata ownership.

## Loader metadata

`<key>.meta.json` contains format version 2, the BLAKE3 key, size, SHA-256,
identification-rules `metadata_version` (currently 1), and a Loader map.

| Stored Loader state | Typed state | Meaning |
| --- | --- | --- |
| Missing key | `CachedIdentification::Missing` | Not checked |
| `null` | `Unrecognized` | Successfully checked without recognition |
| Object | `Recognized` | A validated `CachedModMeta` |

- Runtime `ModMeta` and persisted `CachedModMeta` convert through their typed owner;
  no runtime Loader implementations enter the generic cache.
- Use `mod_metadata`, `list_metadata`, `verify_metadata` and `refresh_metadata` on `ModsCacheDir`.
  These explicit Loader metadata operations are not generic CacheDir methods.
- `mod_metadata` checks snapshot structure and content association without hashing
  the complete file. `verify_metadata(Content)` also verifies complete BLAKE3 and
  the saved SHA-256. Structure-only verification cannot detect same-size corruption.
- Refresh runs on pinned cache bytes, collects every selected Loader result before
  publication, and rechecks exact-byte provenance before committing. A failure
  preserves the previous snapshot or its absence.
- `MissingOrOutdated` reuses current checked results, including non-recognition;
  `Force` reruns selected Loaders. Unselected current results are preserved. A rules
  version change discards old results. Malformed current snapshots are rejected.
- The raw Loader commit remains crate-private. Callers must establish that all
  supplied results came from the exact content under current rules; hashes alone
  cannot establish semantic provenance. Frontends never submit raw Loader results.

## Source identification

`<key>.ident.json` is an independent format-1 snapshot containing BLAKE3,
size, SHA-1 and a nonempty source map. Its validity does not depend on Loader metadata.

- Use `ident_info`, `write_ident_info` and `refresh_ident_info` on the same `ModsCacheDir`.
- The codec validates source keys, references, filenames, size and SHA-1. Every map
  key agrees with its result's source; only successful validated matches are stored.
- Missing means unchecked. Errors and unmatched results never become negative entries.
- `Source::identification_cache_id` owns persistent namespaces. CurseForge
  configurations share `curseforge`; credentials and endpoints are never persisted.
- A refresh reads the actual pinned cache content, validates its BLAKE3, computes
  the source-required summary, queries the source and publishes only a verified match.
  Failure or no match preserves earlier successful records.
- `refresh_ident_info_with_outcome` also returns the transient `RefreshStatus`
  (`Reused`, `Matched` or `Unmatched`) alongside the resulting `CacheIdentInfo`.
  An unmatched refresh may still have an available snapshot from an earlier match;
  without earlier matches it creates no snapshot. Refresh status is never persisted.
- `write_ident_info` accepts a trusted provider result for client/remote orchestration.
  The caller owns provider provenance; the library checks actual BLAKE3, size and SHA-1.
- Consumers validate cached matches against the queried file summary before reuse
  or transport. An additional-file result retains the actual additional download.

## Availability, lookup and concurrency

- Loader and source observations use independent `CacheStatus` values. Missing
  metadata, invalid data and unexpected I/O remain distinguishable.
- A regular content object is required for every reusable snapshot. Orphan JSON
  never establishes availability; generic `scan` and `remove` still observe it.
- Mod opening and source lookup use `ModsCacheDir::find_metadata` and `find_ident_info`,
  internally calling generic `CacheDir::find_content` before the typed reader. Pinned handles avoid repeating a complete hash after a hit.
- These queries never import, link, protect files, publish results or repair indexes.
  Unusable business records fall back to normal identification. Missing/invalid optional
  indexes fall back to full BLAKE3; imohash alone is never equality evidence.
- Metadata is published as complete atomic snapshots. A reader may accept an already
  opened complete old snapshot after replacement. Concurrent writers can lose updates,
  but may never publish partial JSON or unrelated results. There is no batch transaction.
- Async source refresh contains synchronous hashing and storage. Batch callers use
  blocking workers and keep the provider HTTP runtime alive for all requests.

[简体中文](../../docs-zh/conventions/cache.md)
