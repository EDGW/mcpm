# Mod file backups

`mcpm-tools mod backup` creates, verifies, applies and deletes a complete directory's file backup,
and computes directory checksums for comparison with backups. It recursively
includes every regular file, including disabled mods and extensions other than `.jar`.
Archives are not unpacked and empty directories are not recorded. Operations are local only.

## Usage

```sh
mcpm-tools mod backup create ./mods ./backups/before-update
mcpm-tools mod backup create ./mods ./backups/before-update --mode copy
mcpm-tools mod backup create ./mods ./backups/before-update --mode prefer-hard-link --parallelism 4
mcpm-tools mod backup apply ./backups/before-update ./mods
mcpm-tools mod backup apply ./backups/before-update ./mods --force
mcpm-tools mod backup apply ./backups/before-update ./mods --mode hard-link
mcpm-tools mod backup verify ./backups/before-update ./backups/another
printf '%s\n' './backups/before-update' './backups/another' | mcpm-tools mod backup verify
mcpm-tools mod backup checksum ./mods ./another-mods
printf '%s\n' './mods' './another-mods' | mcpm-tools mod backup checksum
mcpm-tools mod backup verify-with ./mods --backup ./backups/before-update
mcpm-tools mod backup verify-with ./mods --checksum <CHECKSUM>
mcpm-tools mod backup delete ./backups/before-update ./backups/another
printf '%s\n' './backups/before-update' | mcpm-tools mod backup delete
```

The backup destination's parent must exist; the destination must be absent or empty.
The apply target's parent must exist; the target must be absent or empty unless `--force` is set.

## Options

| Option | create | apply |
| --- | --- | --- |
| Positional arguments | `<SOURCE> <BACKUP>` | `<BACKUP> <TARGET>` |
| `--mode MODE` | Cache to backup; defaults to `hard-link` | Backup to target; defaults to `copy` |
| `--cache-dir PATH` | An initialized content cache; defaults to `~/.mcpm/cache/mods` | No cache is used |
| `--parallelism COUNT` | Concurrent files per stage; defaults to `10` | Same |
| `--force` | Unavailable | Replace the entire nonempty target, removing files absent from the backup |
| `--details` | Show file paths, BLAKE3, sizes and actual transfer methods in Console | Same |
| `--headers` | Include Raw TSV headers | Same |
| `--raw` | Disable terminal formatting, progress and colors | Same |

`--parallelism` must be positive; use `1` for sequential work.
`--mode` reuses the cache commands' transfer strategies:

- `copy`: copy without sharing file contents with the source.
- `hard-link`: require a hard link; errors such as crossing filesystems fail the operation.
- `prefer-hard-link`: prefer hard links and copy only when the filesystem does not support them;
  permission and path errors do not trigger fallback.

## Creation

1. Recursively enumerate regular files. Symbolic links, special files and invalid paths fail the operation.
2. Copy content into the cache or reuse verified content through Cache APIs. Verify source and cached bytes.
   No `meta` or `ident` is computed, refreshed or backed up.
3. After every source file passes verification, generate `data/` files from the cache using `--mode`, preserving relative paths.
4. Verify generated files, protect file contents and the `data/` directory tree, then write and protect the manifest contents before publishing the backup.

Both `manifest.mcbackup` and data files are protected against in-place writes. After successful
creation, Console prompts you to delete the backup with `mcpm-tools mod backup delete <BACKUP>`;
default directory permissions prevent ordinary recursive deletion on Unix.

Source-to-cache imports always copy; `--mode` controls cache-to-backup transfer only.
Creating a backup therefore does not make independent source files read-only. Sources already linked
to protected content retain their existing protection. Identical contents share one cache object,
while each file path has its own manifest record. Empty sources are valid, but empty subdirectories are not restored.

Failures do not publish an incomplete backup. Completed cache imports remain.
Source, backup and cache directories must not overlap.

## Applying

1. Validate the manifest version, paths and semantic checksum.
2. Match the complete regular-file set in `data/` and verify each file's size and BLAKE3.
3. Generate and verify files in a sibling staging directory, then replace the target after all files succeed.

Default copies are writable. Hard links share the backup's contents and write protection;
replace linked files when updating them rather than editing their bytes in place.
Backups are self-contained and remain usable after the original cache objects are removed.

`--force` never skips verification or merges directories. The previous target is retained until
publication succeeds, with rollback attempted on failure. If rollback or cleanup of the previous tree
fails, the error identifies the retained recovery directory. Directory switching does not provide
transaction recovery after power loss. Backup and target must not overlap. Unsupported target-platform
names or conflicting paths fail the operation.

## Standalone verification

```text
mcpm-tools mod backup verify [OPTIONS] [BACKUP]...
```

- Accepts multiple directory arguments and one backup directory per stdin line; arguments are processed before stdin paths.
- Blank lines are ignored and spaces are preserved. Quotes in stdin are literal filename characters, not shell quoting.
- Identical paths are checked once in first-seen order. At least one path is required after combining inputs.
- Checks manifest format and checksum, the complete `data/` file set, file sizes and full BLAKE3 hashes.
- Does not unpack archives, apply backups, create target directories or open/populate a cache.
- Processes backups sequentially. `--parallelism COUNT` limits concurrent files within each backup; defaults to `10` and must be positive.
- Continues after individual backup failures. Returns `0` only if all selected backups pass.
- `--details` adds file paths, BLAKE3 and sizes in Console; `--headers` adds a Raw header.

Console shows `Backup`, `Status`, `Files`, `Size` and `Checksum` for each backup and reuses detailed integrity diagnostics.
`Size` includes the original encoded manifest bytes. Verification performs no transfer and does not estimate copied, linked or cached space.
Raw emits one row per backup in input order, without headers by default:

```text
backup  status  files  size  checksum
```

`status` is `valid` or `invalid`; the last three fields are empty on failure.
Successful `backup` paths are resolved absolute directories; failed paths retain their input spelling.
`size` is an integer byte count.

## Directory checksums

```text
mcpm-tools mod backup checksum [OPTIONS] [DIRECTORY]...
```

- Compute an independent checksum for each directory using the v1 backup checksum protocol; directories are not merged.
- Multiple directory arguments and newline-delimited stdin follow `verify`'s ordering, blank-line, literal-path and deduplication rules.
  At least one directory is required.
- Recursively include every regular file, including disabled mods and other extensions. Do not unpack or identify mods.
  Empty directories do not contribute to the checksum.
- Split relative paths into components and join them with `/` on every OS. Matching names and contents use the same encoding.
  Case and Unicode representations are preserved; literal backslashes in filenames are rejected.
- Read complete file contents for BLAKE3 without trusting cached hashes. Do not write files, create backups, open caches or change permissions.
- Reject symbolic-link roots or entries, special files and invalid paths. Fail on file changes detected during reading
  or file-set changes detected at completion. This is not an atomic directory snapshot; keep directories stable during calculation.
- Process directories sequentially and continue after individual failures. `--parallelism COUNT` limits concurrent files per directory; default `10`.
- Return `0` only if all calculations succeed. Report errors through chained stderr diagnostics.
- Support Console and Raw; `--details` adds file records in Console and `--headers` adds Raw headers.

Raw emits only the checksum of each successful directory, one per line, without paths, statuses, file counts or sizes:

```text
<checksum>
```

Failed directories emit no stdout line; diagnostics go to stderr and subsequent directories are still processed.
Any failure returns a nonzero exit code. There is no default header; `--headers` adds a single `checksum` header line.
Console additionally shows the directory, status, file count and size. Size counts file-content bytes only, without a manifest.

## Comparing a directory

```text
mcpm-tools mod backup verify-with [OPTIONS] <MODS_DIR>
```

```sh
mcpm-tools mod backup verify-with ./mods --backup ./backups/before-update
mcpm-tools mod backup verify-with ./mods --checksum <CHECKSUM>
mcpm-tools mod backup verify-with ./mods --backups ./backups/a,./backups/b --checksum <CHECKSUM>
printf '%s\n' './backups/a' './backups/b' | mcpm-tools mod backup verify-with ./mods --input backup
mcpm-tools mod backup checksum ./mods | mcpm-tools mod backup verify-with ./another-mods --input checksum
```

- Calculate `<MODS_DIR>` once using the complete `checksum` rules and reuse its records for every target.
- Repeat `--backup BACKUP` or supply comma-separated `--backups BACKUPS`. Commas in a singular `--backup` are literal path characters.
- Repeat `--checksum CHECKSUM` or supply comma-separated `--checksums CHECKSUMS`.
  Checksums must be 64 lowercase hexadecimal characters and use the current v1 protocol.
- Both target types can be combined. Nonblank stdin lines append targets and require `--input backup|checksum`; types are not inferred.
  Spaces and quotes are preserved; blank lines are ignored.
- Process `--backup`, `--backups`, `--checksum`, `--checksums`, then stdin, preserving order within each group.
  Identical targets of the same type are compared once. At least one target is required.
- Backup targets read and validate only the manifest, without inspecting `data/`. A backup directory containing only a manifest is sufficient.
  Use `backup verify` to check the backup's own integrity.
- Console shows the result and expected/actual checksums. Manifest mismatches list all missing, unexpected and size/content-changed files.
  Bare checksums cannot describe file differences. `--details` additionally lists the computed directory's files.
- No directory, backup or cache is written. Parallelism, change detection and stable-directory requirements match `checksum`.
- Continue after individual target read or parse failures. Return `0` only if every target matches; mismatches and operational failures return nonzero.

Console and Raw are supported. Raw emits one TSV row per target; `--headers` includes the header:

```text
target_type  target  directory  status  expected_checksum  actual_checksum
```

`target_type` is `backup` or `checksum`; `target` retains its input value. `status` is `match`, `mismatch` or `failed`.
Unavailable checksums are empty. The directory is absolute after a successful calculation and otherwise retains its input path.
Mismatches are comparison results; read, format and calculation errors use the existing chained stderr diagnostics.

## Deleting backups

```text
mcpm-tools mod backup delete [OPTIONS] [BACKUP]...
```

- Accepts multiple directory arguments and one directory per stdin line, with the same ordering, blank-line, whitespace and deduplication rules as `verify`. At least one path is required.
- Deletes the entire backup directory, including `data/` and the manifest. No cache is opened; cache objects and other hard links are retained.
- Validates the manifest format and checksum before deletion. The backup root must be a regular directory containing no entries other than `data` and `manifest.mcbackup`.
- Does not require full data verification, so backups with missing or damaged files can be cleaned up. Every entry inside `data/` is removed, including files absent from the manifest.
- Allows an already absent `data/` for cleanup after partial deletion. An existing `data/` must be a regular directory. Symbolic-link backup roots or top-level `data/` directories are rejected.
- Symbolic links within `data/` are unlinked without following their targets.
- Temporarily relaxes directory deletion protection without changing any file's permissions or ACL, preserving write protection on cached content and other hard links.
- Processes backups sequentially and continues after failures. Returns `0` only if every selected backup is deleted. There is no additional `--force` option or confirmation step.
- I/O errors may leave a partially deleted backup. The manifest is removed after the data directory, followed by the backup root.

On Unix, the write protection on `data/` prevents ordinary file edits and individual deletion, which also prevents ordinary recursive deletion.
Windows protection prevents in-place content writes; directory protection cannot prevent deletion of a file that has its own `DELETE` permission.
This command provides a cross-platform backup cleanup procedure without broadening deny ACLs on shared files.
Console displays each backup path and a complete result sentence. Raw retains input paths and emits one TSV row per backup:

```text
backup  status
```

`status` is `deleted` or `failed`. Headers are omitted unless `--headers` is set. Errors go to stderr with their cause chains.

## Backup format

```text
backup/
├── manifest.mcbackup
└── data/
    ├── example.jar
    └── disabled/
        └── another.jar.disabled
```

`manifest.mcbackup` is JSON. Version `1` has these fields:

```json
{
  "version": 1,
  "files": [
    {
      "path": "example.jar",
      "blake3": "<64 lowercase hexadecimal characters>",
      "size": 123456
    }
  ],
  "checksum": "<64 lowercase hexadecimal characters>"
}
```

- `path` is a UTF-8 relative file path separated with `/`. Empty components, `.`, `..`, backslashes,
  NUL and colons in the first component are forbidden.
- `blake3` is both the content hash and cache key. `size` is in bytes.
- Directories, absolute cache paths, transfer methods, permissions, modification times and named metadata are omitted.
- Unknown fields, duplicate JSON fields, duplicate file paths and files also used as parent directories are rejected.
- Missing or extra files, symbolic links and special files in `data/` fail verification. Extra empty directories do not affect the file set.

The checksum uses parsed records, independently of JSON formatting, object field order, record order
and equivalent string escaping. Sort files by ascending UTF-8 path bytes and calculate BLAKE3 over:

1. Domain identifier: UTF-8 bytes `mcpm.mcbackup` followed by one NUL byte.
2. `version`: big-endian `u32`.
3. File count: big-endian `u64`.
4. For each file: UTF-8 path byte length (big-endian `u64`), path bytes, the raw 32-byte BLAKE3 digest,
   and file size (big-endian `u64`).

File names are not Unicode-normalized; different names retain different meanings.
The checksum verifies the file mapping's integrity; it does not authenticate the backup's origin.

## Output

Console shows stage progress, completion sentences and a final summary. `--details` adds file field groups.
Summary sizes count content bytes, without querying allocated filesystem space:

- `Size`: all file sizes plus the manifest's actual encoded byte count, including JSON formatting.
- `Actual Size`: copied file sizes plus the manifest size; shared hard-linked contents are excluded.
- Creation labels shared contents `(… cached)`; application links directly to the backup and labels them `(… shared)`.
- Application summaries also include the input manifest size; the manifest is not copied into the target Mod directory.
- Sizes use forms such as `1.50MiB (1572864 bytes)`, with parentheses and exact bytes shown in gray.
- Hard links trigger an English tip explaining that copying, transferring or moving to a location that cannot preserve the links may consume that space again, with `--mode copy` for independent copies.

Redirected stdout automatically uses Raw. Stderr independently controls progress; explicit `--raw` disables it.

Creation and application emit one Raw TSV row only after success, without headers by default. Columns are:

```text
operation  destination  files  size  actual_size  shared_size  checksum  copied  hard_linked
```

`operation` is `create` or `apply`; `destination` is the resolved absolute target path.
`size`, `actual_size` and `shared_size` are integer byte counts; `shared_size` counts hard-linked file contents and excludes the manifest.
Backslashes and controls in paths use the existing TSV escaping convention. `--details` has no effect on Raw.
Errors go to stderr with their cause chains and a nonzero exit status. Integrity failures also show missing and unexpected files, or the affected file path with expected and actual values.
