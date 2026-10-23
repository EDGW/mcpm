# Mod cache conventions

The cache stores immutable mod-file content and reusable identification results.
It is owned by `local::mcmod::cache`; loaders identify files independently of cache
policy, and frontends select the cache and present typed results.

## Consistency contract: partial but correct

**Cache completeness is best effort. Every represented result accepted for reuse
must be correct.**

- Writes may be skipped, disappear, or lose concurrent updates. There is no
  requirement to retain every write or eventually merge every loader result.
- A represented result must refer to the correct content and a supported
  identification version. Successful non-recognition is a positive assertion;
  missing data or failed identification must never be converted to non-recognition.
- Concurrent writers may publish valid snapshots based on the same older snapshot.
  For example, a Forge update may overwrite a Fabric update and lose Fabric's
  result. Publishing a partial JSON document or unrelated metadata is forbidden.
- Readers reject malformed, incompatible or incorrectly associated records.
  Missing or unusable cache data falls back to identification; existence alone
  does not establish availability.
- A successful publication describes that operation. It does not promise future
  existence, backup durability, or exclusion of concurrent deletion.
- Concurrent deletion and publication can leave orphaned halves or recreate an
  object after removal. Removal does not prohibit concurrent resurrection.
- Explicit operation failures remain errors. The best-effort contract does not
  authorize reporting a failed object publication as successful.

## Content identity and directory protocol

`CacheId` is the complete BLAKE3-256 of the file: exactly 64 lowercase hexadecimal
characters. Equal BLAKE3 values mean the same content; the cache does not implement
collision disambiguation. One content ID has one canonical data name.

```text
<cache-dir>/
├── .mcpm/
│   ├── mcpm.json              # {"version":2}
│   ├── index.sqlite3          # optional derived index
│   ├── index.sqlite3-wal      # SQLite may create these runtime files
│   ├── index.sqlite3-shm
│   └── index.lock             # index recovery coordination only
└── <first-two-hex-characters>/
    ├── <complete-blake3>.dat
    └── <complete-blake3>.data.json
```

- `.dat` is the content source of truth. Published contents must not change in place.
- `.data.json` stores identification results and verification information. It may
  be absent; data without metadata remains exportable after content verification.
- The directory manifest has `CACHE_DIRECTORY_VERSION = 2`. Version 1 GUID caches
  are rejected; initialization never migrates, deletes or adopts their contents.
- `ModsCache::user_default()` selects `~/.mcpm/cache/mods`. Access initializes an
  absent or empty root; a nonempty root without its manifest is broken.
- `ModsCache::new(path)` selects an explicit root. Access requires an existing
  valid manifest, even for an empty directory. `initialize()` explicitly creates
  an absent/empty root or accepts an already valid root.
- Malformed manifests and unsupported versions are directory errors, separate
  from per-object `CacheStatus`. Construction alone performs no I/O.
- First initialization uses a persistent `.mcpm-cache-init.lock` in the root's
  parent. Acquiring this bootstrap lock blocks and requires parent write access.
  Valid-root access does not acquire it. Do not delete coordination files while
  operations are active.
- Managed cache paths do not use symbolic links. Staging files and directories
  are temporary implementation details, not stored objects.

## Metadata protocol and interpretation ownership

A digest contains these fields:

| Field | Meaning |
| --- | --- |
| `version` | Digest format version; currently `CACHE_FORMAT_VERSION = 2` |
| `blake3` | Content ID; must agree with the canonical object name |
| `size` | File size in bytes |
| `sha256` | SHA-256 for display and content/provenance verification; not another object ID |
| `metadata_version` | Identification rules version; currently `MOD_METADATA_VERSION = 1` |
| `loaders` | Per-loader identification results |

The three loader states are distinct:

| Persisted state | Typed result | Meaning |
| --- | --- | --- |
| Missing loader key | `CachedIdentification::Missing` | Not checked |
| `null` | `CachedIdentification::Unrecognized` | Successfully checked, not recognized |
| Metadata object | `CachedIdentification::Recognized` | Recognized `CachedModMeta` |

- A nonempty all-null `loaders` map is valid. Identification errors never become
  null. An empty map does not provide an available identification digest.
- `ModMeta` holds runtime loader instances; `CachedModMeta` holds persisted loader
  IDs, direct declarations, dependencies and recursive relative archive paths.
  Capture and restoration are typed conversions without JSON or filesystem work.
- Restoring runtime metadata uses the caller's loader registry. Cache abstractions
  do not import concrete loader implementations.
- `cache::directory` is the sole manifest codec; `cache::storage` is the sole
  object-digest codec; `cache::index` owns SQL schema, index versions and sampling.
- Every interpretation of persisted data goes through its owner. Extend the
  owner's interface when needed; never add a second decoder or inspect raw JSON
  in `open()`, loaders or the CLI.
- `CacheDigest::identification()` and `identifications()` expose loader states
  without requiring consumers to interpret null/missing serialization details.

## Access handles and availability

| Handle | Operations | Receiver |
| --- | --- | --- |
| `ModsCache` | Select root, initialize, open reader/writer | Constructor or `&self` |
| `CacheReader` | `get`, `list`, `verify`, `export` | `&self` |
| `CacheWriter` | `import`, `import_with_mode`, `refresh_metadata`, `remove`, `rebuild_index` | `&mut self` |
| `CacheWriter` | All Reader operations | Read-only `Deref` |

- Reader/Writer distinguish capabilities. Neither retains a global or object
  lock; a Writer's lifetime is not an exclusive transaction.
- Independent handles can interleave every operation. `&mut self` does not exclude
  other instances, threads or processes. There is no snapshot across calls or
  batch rollback guarantee.
- `CacheEntry` is an observation; its paths and state can change immediately.
- There is no shared-to-exclusive object-lock upgrade or reentrancy protocol.
  SQLite, index recovery and initialization have their separate synchronization.

| Status | Meaning |
| --- | --- |
| `Available` | Data and digest structure, size and metadata version are valid, with at least one checked loader |
| `NotFound` | Neither data nor digest exists |
| `Unavailable(MissingDigest)` | Data exists; digest is absent |
| `Unavailable(InvalidData)` | Missing data, wrong file type, malformed/incompatible metadata or failed integrity check |
| `Unavailable(UnexpectedError(String))` | Unexpected filesystem/OS access failure |

`get` checks structure and size without full hashing; it cannot detect same-size
content corruption. `list` includes orphaned halves and per-object unavailable
states while ignoring temporary names. Enumeration errors return `CacheError`.
`verify(Structure)` checks structure and size; `verify(Content)` hashes available
or missing-digest data and verifies its BLAKE3 name and any saved fingerprint.
Correct bytes without metadata still have `MissingDigest` status. Verification
never repairs data or changes saved fingerprints to accept modified bytes.

## Explicit mutations and export

### Import and duplicate publication

- `import()` defaults to `Copy` and returns the content ID. `import_with_mode()`
  supports `Copy`, `HardLink`, `PreferHardLink` and returns `ImportOutcome`.
- Import does not identify mods or create a digest. Pin the source and compute
  its BLAKE3 and imohash before choosing a transfer strategy. If the canonical
  data name already exists, verify its content ID and reuse it without staging,
  attempting a hard link or changing source permissions. Never replace its
  identity. An invalid existing target is an error.
- Transfer mode applies only to new objects. Thus an existing object can be reused
  even when `HardLink` could not link the source across filesystems. After a miss,
  stage bytes on the cache filesystem using the selected mode, verify their hash
  equals the initial source ID, protect contents, and publish `.dat` atomically
  without overwriting. Validate source and staging against their retained
  observations across hashing, protection and publication.
- Copy import flushes newly written bytes before staging publication. Hard-link
  import publishes existing bytes without writing or additionally flushing a
  read-only source handle; it does not guarantee source durability after power loss.
- Competing publishers use the same no-overwrite rule; a losing publisher validates
  and reuses the winner's data. Temporary names are normally cleaned up.
- `published = true` means this call published `.dat`, with `method = Some(Copy)`
  or `Some(HardLink)`. Reuse returns `published = false`, `method = None`.
- Reusing content does not mean the source became a hard link. Independent copies
  remain independent; import never replaces a source filename to associate it.
- Hard-link staging verifies native identity. Protection affects all existing
  links, including the source; it may remain even if publication later fails or
  loses a race. The initial existing-object path avoids these side effects.
- `PreferHardLink` copies only for cross-filesystem/unsupported-operation failures,
  not permission failures or arbitrary I/O errors. The filesystem helper retains
  native errors; import owns fallback policy.
- Concurrent external source edits cannot be made into an instantaneous backup
  snapshot by ordinary copying. Callers requiring that guarantee stabilize inputs.

### Refresh and internal metadata writes

- `refresh_metadata` identifies cached bytes with the caller's selected loaders;
  it does not read the original import source. An empty selection changes nothing.
- `MissingOrOutdated` skips checked current results, including non-recognition;
  `Force` reparses selected loaders. All selected attempts must succeed before
  publication. Failures preserve the previous digest or its absence.
- Preserve unselected current results. A metadata-version change discards old
  results; only results checked with current rules are added back. Reject malformed
  current records instead of silently blessing them.
- Fingerprint before identification, retain/validate the source observation, then
  recheck BLAKE3, SHA-256 and size through the shared commit path before publication.
- `CacheWriter::write_metadata` is **`pub(crate)` only and must never be exposed or
  used outside this crate under any circumstances**. Incorrect metadata associations
  destroy data consistency. Every call site must state this restriction and
  establish that supplied results came from the exact content under current rules.
- This trusted interface takes a content ID, the expected SHA-256 and typed loader
  results. Hash/structure checks do not prove semantic provenance; callers must.
  `None` asserts successful non-recognition; omitted loaders preserve current
  results. An empty update does not create a missing digest.
- `cache::storage` encodes one complete snapshot into a private temporary file,
  flushes it and atomically replaces the digest. Identical encoded bytes avoid
  publication. Concurrent merges may lose updates; they do not require a merge lock.
- `.dat` is never modified. A concurrent removal may leave a digest-only object;
  readers reject the incomplete pair.

### Export and removal

- Export is a Reader operation independent of recognition. Missing digests are
  accepted; malformed content descriptors are rejected. Check BLAKE3 and, when
  present, the saved fingerprint even if metadata is stale.
- Every destination must be absent, outside the cache, with an existing parent.
  Copy export stages and verifies independent bytes before no-overwrite publication.
  Hard-link export verifies content, creates the link and verifies the result.
- `HardLink` requires linking; `PreferHardLink` falls back only on filesystem
  capability failures. Return the method actually used.
- `remove` deletes the two cache names and best-effort index entries. Other hard
  links remain valid. Missing names return false; partial removal errors are reported.
- No eviction, reference counting or backup-reference tracking is implied.
  Callers own batch ordering and partial-success handling.

## Read-only cache-aware identification

- `ModOpenContext::default()` disables cache I/O. Selecting default/explicit cache
  validates the directory without scanning all objects; one context can be shared
  across parallel opens. Default-root initialization remains possible at selection.
- `CacheLookup::find` pins the input file, obtains native identity and computes
  imohash. A resolved canonical `.dat` path can supply a direct candidate alongside
  candidates from the quick index. Compare current pinned input/candidate
  identities and validate observations before accepting the fast path.
- Unix identity is device plus inode; Windows uses volume plus File ID. A pinned
  handle prevents identity reuse while comparing. Paths alone and sampled hashes
  do not establish equality.
- Without an identity match, compute full BLAKE3 and look up the canonical object.
  Independent copies of identical content can therefore reuse metadata too.
- Cache identity matching depends on immutable contents. Sampling cannot detect
  every in-place edit; native identity alone is not an integrity checksum.
- `open()` reuses valid recognized and unrecognized results. Missing, unavailable
  or unusable loader results run normal identification with the existing error
  callback. Loaders do not implement cache policy.
- Opening never imports files, creates hard links, changes content permissions,
  writes metadata, updates the index, or repairs it. Explicit operations store data
  and refresh results. SQLite may manage its own WAL runtime files internally.
- `ModOpenReport` contains `ModFile` and disabled/miss/matched cache usage, including
  object status and per-loader usage. It has no writeback result. Input paths and
  enabled state remain on the returned file; nested paths remain relative.
- Full/partial/none reuse counts the requested loader results, including cached
  non-recognition. Empty loader selection is not a hit. Frontends aggregate file
  inspections before filtering/expanding declarations; opening has no added/updated
  cache statistics.

## Optional imohash index and recovery

- `cache::index` owns imohash-1.0: MurmurHash3 x64-128, 16 KiB samples at beginning,
  midpoint and end for files at least 128 KiB; smaller files are fully sampled.
  The index version fixes both schema and algorithm parameters; currently it is 1.
- SQLite stores only `(imohash BLOB(16), blake3 BLOB(32))` associations, unique as
  pairs, with a reverse BLAKE3 index. It stores no ModMeta, absolute paths or durable
  native identities. Content and digests remain the source of truth.
- Colliding imohashes yield more candidates, never proof of equal content. Entries
  can be absent or stale; candidates are validated and misses fall back to BLAKE3.
- Import best-effort inserts associations; remove best-effort deletes them.
  Metadata-only writes do not change the index. SQLite WAL transactions have zero
  busy timeout; busy optional updates can be dropped without failing data operations.
- Lookup opens the index read-only. Missing, corrupt, incompatible or busy indexes
  cause a logged performance miss; lookup does not create, rebuild or repair them.
  Import can initialize a missing index; a damaged one requires explicit recovery.
- `CacheWriter::rebuild_index()` scans canonical data names, verifies BLAKE3, samples
  valid data and rebuilds the derived database. Invalid/missing data is skipped.
  Concurrent data changes may still leave missed or stale entries. The returned
  count is the number registered by this rebuild, not a durable object count.
- `.mcpm/index.lock` provides nonblocking shared leases for database connections and
  exclusive recovery access for rebuild publication. It prevents unlinking a live
  WAL database. If unavailable, lookup falls back, optional updates skip, and
  explicit rebuild returns an error. This is not a global object/metadata lock.
- Data publication and index transactions are independent; no cross-file transaction
  or lossless recovery journal is required. Private-file flush and atomic publication
  do not promise power-loss-safe multi-file persistence. Do not automatically delete
  unknown orphaned content.

## Permissions and filesystem boundaries

- Protect contents before publishing new data; keep directories writable so names
  can be deleted or replaced. Unix removes write bits without widening read access.
- Windows denies `FILE_WRITE_DATA` and `FILE_APPEND_DATA` while retaining other ACL
  permissions and delete behavior; do not use the read-only attribute. A null DACL
  is explicitly unsupported. Unsupported protection returns an error.
- Hard links share content, permissions and identity. Consumers update by writing
  a new file and replacing their own filename, never by overwriting, appending or
  truncating linked bytes. Exported copies are independently writable.
- Owners, administrators and existing writable handles can bypass permission changes.
  This is not an adversarial immutability boundary; Windows replacement also depends
  on file sharing and open handles.
- Filesystem identity, protection and hard-link helpers are independent of cache
  models and loaders. Cache code owns naming, content validation and import/export
  policy; `open` owns reuse/fallback; frontends own selection and presentation.

[简体中文](../../docs-zh/conventions/cache.md)
