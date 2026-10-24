# Generic local cache

`local::cache` stores immutable content and independently encoded metadata. It has
no dependency on loaders, mod identification, sources, credentials or presentation.
Mod and source workflows can build typed adapters over this storage layer.

## Root and layout

```text
<cache>/
├── .mcpm/
│   ├── metadata.json          # {"version":3}
│   ├── index.sqlite3          # optional derived imohash index
│   ├── index.sqlite3-wal
│   ├── index.sqlite3-shm
│   └── index.lock             # index recovery coordination
└── <first-two-hex>/
    ├── <complete-blake3>      # immutable content
    └── <complete-blake3>.<metaname>.json
```

- `Blake3Hash` is exactly 32 bytes, displayed, parsed and serialized as 64 lowercase
  hexadecimal characters. Equal keys identify the same content.
- `CacheDir::new(path)` opens an existing valid root. It does not initialize it.
- `CacheDir::initialize(path)` creates an absent or empty root, or accepts a valid
  root. It rejects nonempty uninitialized roots and unsupported manifests. The
  parent contains a persistent `.mcpm-cache-init.lock` coordinating initialization;
  initialization can require parent write access and wait for concurrent initializers.
- Opening retains a canonical absolute root. Managed shards, content, metadata and
  index paths must not be symbolic links. Coordination files must remain in place
  while operations are active.
- Version 2 Mod cache roots use a different manifest and `.dat` names. This API
  never adopts or migrates them; migration must be explicit.

## Operations and results

| Operation | Behavior |
| --- | --- |
| `import` | Returns the full content key; identical existing content is verified and reused |
| `import_with_outcome` | Also reports whether this call published content and the actual transfer method |
| `export_file` | Exports one target and returns its actual method; missing content is an error |
| `export` | Exports to absent destinations with existing parents outside the cache |
| `export_with_outcomes` | Also reports the resolved destination and actual method for each completed target |
| `entry(key)` | Observes regular content and sorted regular metadata names; absent content returns `None` |
| `list()` | Sorted regular content keys; ignores orphan metadata and malformed names |
| `inspect(key)` | Structure only: content kind/size and named metadata kinds, including orphan metadata |
| `scan()` | All owned keys, including invalid object types and orphan metadata; each row retains inspection errors |
| `check(key)` | Full BLAKE3 verification; `None` for absence, `Some(())` for success, errors for corruption or I/O failure |
| `open_content_with_mode(key, mode)` | Pins content using explicit structure-only or complete verification |
| `open_content(key)` | Pins and fully verifies read-only content, returning `None` for absence |
| `find_file(path)` | Read-only lookup of a file's cached key |
| `find_content(path)` | Same lookup, retaining a pinned content handle and its verification method |
| `rebuild_index()` | Explicitly rebuilds the optional index from verified content; skips invalid objects |
| `try_read_metadata<T>` | Fully verifies content before decoding a named snapshot through its `Codec` |
| `write_metadata<T>` | Fully verifies content and atomically replaces a complete encoded snapshot |
| `remove(key)` | Removes content and all owned metadata names, including orphan snapshots; other hard links survive |

- `CacheLinkPreference` supports `Copy`, `HardLink` and `PreferHardLink`. Fallback
  to copying is allowed only for cross-filesystem or unsupported hard-link operations.
- New imports and exports are staged and verified before publication without
  overwriting existing content. Import outcomes distinguish this call's publication
  from reuse; an existing object is never replaced merely to associate another source.
- Import protects content from ordinary in-place writes. Hard links share protection,
  including the original source. Copies exported from the cache remain writable.
- All export destinations are preflighted. Each target is published independently;
  a later failure leaves already completed exports in place. For per-target error
  handling, call the operation with one destination at a time.
- Removal never recursively deletes directories or adopts unknown names. A failure
  can leave a partially removed object; concurrent publication may recreate names.
- File operations retain their native causes in `anyhow` context. Hash mismatch is
  additionally identifiable as `ContentHashMismatch`.

## Content handles and metadata ownership

- `CacheContent` exposes the key, canonical content path, size, read-only file and
  `verification()`. Its file cursor initially points to zero; reads and clones share
  that cursor. Call `validate()` around path-based work to detect ordinary replacement
  and content changes while retaining the original observation.
- `CacheContent::try_read_metadata<T>()` decodes metadata without hashing content
  again. This is the reusable path after `open_content` or `find_content`.
- Metadata names are nonempty strings such as `loaders`, `loader.details` or `加载器信息`.
  Unicode, spaces and dots are preserved. Path separators, control characters and
  the Windows filename characters `:`, `*`, `?`, `"`, `<`, `>` and `|` are rejected.
  The cache appends `.json` when building file paths and removes only this suffix
  when returning names; `loader.details` is stored as `<blake3>.loader.details.json`.
- All metadata file access goes through `CacheDir` or `CacheContent`. Adapters and
  consumers use these interfaces instead of opening or writing metadata paths.
- `Codec` implementations are the sole interpretation owners of their snapshots.
  The cache handles bytes and atomic publication; adapters own schema versions,
  availability, refresh policy and the semantic association with actual content.
- Missing content or missing metadata returns `None`. Malformed metadata remains
  an error. An orphan snapshot is never usable without its content object.
- Metadata writes publish complete snapshots. Concurrent updates can lose results,
  but readers never accept a partially written snapshot. There is no batch transaction.

## Fast lookup and index recovery

- The shared imohash-1.0 sampler uses 16 KiB at the beginning, midpoint and end of
  files at least 128 KiB long. Smaller inputs are sampled completely. Reader APIs
  operate on pinned handles without converting paths to UTF-8 or reopening by name.
- SQLite stores only `(imohash, blake3)` pairs. It stores no business metadata or
  persistent native file identities. Contents and typed snapshots remain authoritative.
- `find_content` samples the input, queries candidates and compares pinned native
  file identities. A shared identity uses `ContentVerification::FileIdentity` and
  avoids full hashing. imohash alone never establishes equality.
- Independent copies, stale/missing candidates and unavailable indexes fall back
  to full BLAKE3 verification. The returned handle reports `Blake3` for this path.
- Lookup does not import, protect, publish snapshots, update the index or repair it.
  SQLite can manage WAL runtime files internally while accessing an existing database.
- Imports best-effort record candidates and removals best-effort forget them. Busy,
  damaged or incompatible indexes do not fail otherwise valid content operations.
  Only an explicit `rebuild_index` repairs derived state.
- Connections retain shared recovery leases. Rebuild requires a nonblocking exclusive
  lease before replacing the database/WAL/SHM, so it never unlinks a cooperating live
  connection. WAL transactions use zero busy timeout; optional updates may be dropped.
- Rebuild can be incomplete after concurrent content changes. Every candidate is
  checked against current content access; index presence alone is not availability.
- Identity-based matching relies on content immutability. Sampling cannot detect
  every in-place edit; owners, administrators and preexisting writable handles can
  bypass protection. `check` remains the explicit complete integrity check.

[简体中文](../../docs-zh/conventions/local-cache.md)
