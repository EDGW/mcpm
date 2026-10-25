# Mod cache management

`mcpm-tools cache mods` explicitly manages content and named metadata in `~/.mcpm/cache/mods`.

- Each entry is identified by a complete 64-character lowercase hexadecimal BLAKE3. Identical content is stored once.
- `meta` and `ident` are metadata names, containing Loader declarations and source matches respectively.
- `import` stores content only. It never calculates, refreshes or reads these metadata snapshots.
- `mod info/list` and `mod source identify` query existing caches without writing. Use
  [`mod cache`](mod/cache.md) or [`server mods cache`](server/mods.md) for selected-file caching.
- Management commands run locally and support Console and Raw. `--raw` or non-TTY stdout selects Raw.
- Raw list, info and simple operation receipts use TSV. Refresh succeeds silently with exit code 0.

## Commands

```sh
mcpm-tools cache mods init --directory /path/to/cache
mcpm-tools cache mods import example.jar another.jar
mcpm-tools cache mods import example.jar --mode hard-link
mcpm-tools cache mods list
mcpm-tools cache mods list --columns blake3,size,metadatas
mcpm-tools cache mods list --details
mcpm-tools cache mods info HASH1 HASH2 --columns blake3,status,metadatas
mcpm-tools cache mods info --all
printf '%s\n' HASH1 HASH2 | mcpm-tools cache mods info
mcpm-tools cache mods refresh HASH1 HASH2 meta --loaders fabric,forge
mcpm-tools cache mods refresh HASH1 ident --source modrinth --source curseforge
mcpm-tools cache mods refresh --all meta
mcpm-tools cache mods refresh --all meta --parallelism 4
mcpm-tools cache mods refresh --all ident --details
mcpm-tools cache mods refresh HASH1 meta --short
printf '%s\n' HASH1 HASH2 | mcpm-tools cache mods refresh ident
mcpm-tools cache mods verify HASH1 HASH2
mcpm-tools cache mods verify --all
mcpm-tools cache mods verify --all --parallelism 4
mcpm-tools cache mods verify --all --details
mcpm-tools cache mods verify --all --autoremove
mcpm-tools cache mods export HASH1 restored.jar another-copy.jar
mcpm-tools cache mods remove HASH1
mcpm-tools cache mods remove --all
mcpm-tools cache mods rebuild-index
```

| Command | Behavior |
| --- | --- |
| `init` | Initialize a missing/empty directory, or accept an existing valid cache |
| `import PATH...` | Import/reuse content without calculating metadata |
| `list` | Inventory entries, including invalid content types and orphan metadata; BLAKE3 only by default |
| `info [HASH]...` | Inspect content and selected metadata without identification or refresh |
| `refresh [HASH]... meta` | Refresh selected Loader results |
| `refresh [HASH]... ident` | Refresh selected source results |
| `verify [HASH]...` | Verify content BLAKE3 without reading or validating metadata |
| `export HASH PATH...` | Export to absent destinations with existing parents |
| `remove [HASH]...` | Remove selected content and all owned metadata names; `--all` selects the complete inventory |
| `rebuild-index` | Rebuild the optional imohash index from verified content |

`--directory PATH` can appear before or after a mods subcommand. Explicit roots must already be initialized;
the default root initializes when absent/empty. Directory format is version 3. Nonempty unmarked roots,
malformed manifests and incompatible versions are rejected. No automatic migration.

## Inputs and columns

- info, verify, refresh and remove append redirected stdin after arguments, one complete HASH per nonblank line.
- Duplicates are processed once in first-seen order. At least one combined HASH is required unless `--all` is used.
- info, verify, refresh and remove accept `--all` to select the complete inventory in BLAKE3 order,
  including orphan metadata and invalid content types. It conflicts with HASH arguments and nonblank stdin.
  Empty inventory succeeds without processing entries. Selection does not decode metadata.
  Orphans and invalid types can be shown by info. Refresh stops at the first entry it cannot process;
  verify continues through every selected entry.
- With `--all`, refresh and verify process up to `--parallelism COUNT` entries concurrently (default `10`).
  COUNT must be positive; `1` processes entries sequentially. Explicit HASH inputs remain sequential.
  Results retain selection order (`--all` uses BLAKE3 order), regardless of completion order. info does not accept this option.
  Sources within one refresh entry still run sequentially, so concurrent source writes never target the same entry.
- Place refresh HASH arguments before meta or ident.
- Combine `--columns A,B`, repeated `--column A`, and `-C BFSHTM`. Explicit selections replace defaults.
- list defaults to blake3 only. Metadata is displayed only when metadatas is selected.
  `--details` selects all columns unless columns are explicit.
- Console prints single-field entries as a list; an explicit field label appears once at the top, while default BLAKE3 has no label.
  Multiple fields form a group with inline labels, separated from the next entry by a blank line. More than 80 entries require Y/n confirmation (Enter means Yes). Metadatas shows
  snapshot states and complete details through the shared data renderer. Loader declarations use the same format as `mod info`;
  explicitly unrecognized Loaders are shown, while unchecked Loaders are omitted.
- Raw list and info share the same columns and TSV rendering, one row per entry. Metadatas contains
  sorted names only and does not decode snapshots. Fields escape controls and literal backslashes.
  No header by default; `--headers` prints one header and does not affect Console or refresh.
- info defaults to all columns. Console shows each selected metadata's state and complete details,
  including source matches as labeled field groups. Raw shows metadata names only.

| Column | Letter | Meaning |
| --- | --- | --- |
| blake3 | B | Content ID |
| file | F | Cached content path, not the original import filename |
| size | S | Actual content size in bytes |
| sha256 | H | SHA-256 from valid meta, empty when absent; never calculated on demand |
| status | T | Content structure state; Console also shows the reason when unavailable |
| metadatas | M | Raw: sorted names; Console: states and full details for known names, presence for other names |

Content `available` means a regular content file exists; `not_found` means no content file exists.
Use verify to establish its full content identity. meta and ident report independent states, and missing/corrupt
metadata does not make content verification fail. `--structure-only` checks regular-file presence only and
cannot detect same-size content corruption.

## Verification

- Default output shows only failed entries (`unavailable` or `not_found`). `--details` includes
  `available` entries in both Console and Raw. Results retain selection order.
- Verification continues after individual errors. Every error is reported to stderr with its cause chain.
  Any verification failure produces a nonzero exit code after the complete batch.
- `--autoremove` removes each failed entry's content and associated metadata using the same behavior as
  `remove`. Valid entries, exported copies and external hard links remain. Removal errors are reported
  without stopping verification; unexpected directories are never recursively deleted.
- `--structure-only` also controls the check used with `--autoremove`; it cannot detect content corruption.
- A progress bar on terminal stderr tracks completed entries, including optional removal. `--raw` disables it;
  redirecting stdout alone keeps progress enabled when stderr is a terminal.
- Final statistics go to stderr: entries checked, available and failed. With `--autoremove`, they also
  include entries removed, already absent and failed to remove. Removed entries still count as failed verification.
- `--headers` prints one Raw TSV header before the first displayed result; there is no header if all results are hidden.

## Refresh

- Console uses short output when more than three entries are selected, counting unique HASH inputs
  after merging arguments and stdin, or the inventory selected by `--all`.
- Short output contains only full BLAKE3 and `success` / `failed`, one line per entry.
  Reused matches and successful queries without a match both count as success. Errors still go to stderr.
- With up to three entries, Console defaults to detailed output, starting with the same labeled
  BLAKE3, Status and File fields as info, followed by Loader or source details.
  Status describes cached content, independently of whether a source matched.
- `--details` forces detailed output; `--short` forces short output. They conflict.
  These flags and `--all` work before or after meta/ident; HASH arguments must precede the subcommand.
- Raw, including redirected stdout, emits nothing on success and returns exit code 0.
  `--headers`, `--details` and `--short` have no effect in Raw. Use info or list to query the resulting cache.
- Errors go to stderr through the shared error renderer, including each `caused by:` context,
  and return a nonzero exit code.
- meta defaults to forge,fabric,neoforge. Combine `--loaders A,B` and repeated `--loader A`;
  both accept commas. Explicit selection replaces defaults.
- Current recognition and successful non-recognition are reused; `--force` reidentifies selected Loaders.
  Selected results commit together, preserving unselected current results; failures preserve the old snapshot.
- ident defaults to modrinth,curseforge in that order. Select with `--sources A,B` or repeated `--source A`.
  `curseforge@KEY` uses the official API. Each source may be selected once.
- Only verified successful matches persist. Misses are not negatively cached. `--force` bypasses cached matches;
  errors and unmatched requests preserve previous positive records.
  Ambiguous verified candidates count as unmatched.
- Detailed Console ident output reports each selected source as `reused`, `matched` or `unmatched`.
  `reused` skips a provider request; `matched` verifies and stores a provider match;
  `unmatched` is a successful query without a match and is never stored.
- Detailed Console ident output groups fields by source, showing the current result and saved match details.
  When every source is unmatched and no earlier matches exist, no ident snapshot is created;
  Console shows `Stored matches: none`.
- An unmatched source may still show a retained earlier match in Console; a miss does not remove saved matches.
- Sources for one HASH commit in selection order; a later source failure retains earlier writes.
- A failed entry stops new work beyond it. In-flight operations finish and may complete writes;
  output stops at the first failed entry in selection order. Completed writes remain; there is no batch rollback.

## Transfers and Raw receipts

- Import/export process inputs in order and emit one record per successful item. The first failure stops processing.
- `--mode copy` is the default; hard-link requires links; prefer-hard-link falls back only for filesystem capability failures.
- Valid existing content is reused without replacement or linking/protecting another independent source copy.
- Hard links share content and write protection. Update files by replacing names with new files.
- Export never overwrites or creates parents. Removing cache names leaves exported copies and hard links intact.
- TSV escapes controls and literal backslashes. An empty method means reuse without a transfer.

| Operation | TSV fields in order |
| --- | --- |
| init | directory, version |
| import | blake3, file, published, method |
| export | blake3, destination, method |
| verify | blake3, status, reason |
| remove | blake3, removed |
| rebuild-index | indexed |

Verification receipts contain only status; error details use the shared stderr cause-chain renderer.
The `reason` TSV field is reserved and empty.

## Concurrency

- Published content is immutable; metadata publishes complete snapshots atomically.
- Concurrent updates may lose some results, and a publisher may recreate deleted names. No batch transaction.
- Reusable metadata requires valid associated content; orphan metadata can be inventoried/removed but not reused.
- imohash only selects candidates. Lookup validates identity and falls back to full BLAKE3 when needed. The index is rebuildable.

[Command reference](README.md) · [中文](../docs-zh/cache.md)
