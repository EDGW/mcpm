# Server Mod snapshots

`mcpm-server stash` saves and switches complete `mods/` file sets. The same commands
are available as `mcpm stash` and `mcpm-tools server stash`. This version supports
local servers only; SSH and Remote workspaces are rejected.

## Usage

```sh
mcpm-server stash create "stable"
# Change mods, then save another version
mcpm-server stash create "experimental"
mcpm-server stash list
mcpm-server stash status
mcpm-server stash switch --name stable
# Explicitly discard unsaved contents
mcpm-server stash switch --name stable --force
```

- `--server PATH` selects a server, defaulting to the current directory; accepted before or after subcommands.
- Snapshots include all regular files recursively, including disabled mods and non-JAR files. Empty directories are omitted.
- Worlds, configuration, launch scripts, Minecraft versions and loader declarations are not switched.
- Keep mods stable during creation and switching. The generated workspace `.gitignore` does not ignore stash.

## Commands

| Command | Behavior |
| --- | --- |
| `create NAME` | Save a modified or untracked workspace and advance HEAD; reject unchanged workspaces |
| `switch --name NAME` / `--id UUID` | Fully verify the target, check the workspace, then replace all mods and advance HEAD |
| `status` | Compare mods against HEAD and show added, removed and modified paths |
| `list` | List snapshots newest first without hashing mods or fully verifying backups |
| `info` | Read selected indices and manifests; `--details` adds file records in Console |
| `rename --name OLD NEW` / `--id UUID NEW` | Change the name without changing identity, time, backup or HEAD |
| `delete` | Delete selected backups and indices, preserving mods and cached content; refuse HEAD |
| `verify` | Verify indices, manifests, complete file inventories, sizes and full BLAKE3 hashes |

Names are unique and case-sensitive. Spaces and Unicode are allowed; whitespace-only
names and control characters are rejected. Names are not trimmed automatically.

## Workspace protection

HEAD identifies the most recently saved or applied stash. Its marker does not imply a clean workspace.

| State | Meaning | Switch policy |
| --- | --- | --- |
| `clean` | Workspace checksum matches HEAD | Allowed; selecting HEAD needs no replacement |
| `modified` | Workspace differs from HEAD | Requires `--force` |
| `untracked` | HEAD does not exist | Requires `--force` |

- All three valid status results exit successfully; inspection errors return nonzero.
- Switch always fully verifies the target first, including with `--force` and when selecting HEAD.
- Force permits discarding contents; it does not bypass corrupt backups, invalid HEAD or workspace read errors.
- Force-switching to HEAD restores the saved version and discards modifications.
- No automatic snapshot is made before switching. Run create first to preserve modifications.
- Application copies writable files and replaces the whole directory, removing files absent from the target.

## Batch selection

Info, verify and delete accept repeated singular options, comma-separated plural options,
and literal nonblank stdin lines:

```sh
mcpm-server stash info --name stable --name experimental
mcpm-server stash verify --ids UUID1,UUID2
printf '%s\n' stable experimental | mcpm-server stash info --input name
mcpm-server stash delete --id UUID1 --id UUID2
```

- Nonblank stdin requires `--input name|id`; types are never inferred.
- Commas are literal in `--name`, separators in `--names`.
- Order is name, names, id, ids, then stdin; each group retains its input order.
- Each resolved UUID is processed once. Item failures do not stop later items; any failure returns nonzero.
- Selecting HEAD for deletion refuses the entire batch before any deletion. Invalid HEAD also blocks deletion.
- Delete has no additional confirmation. Backup deletion precedes index deletion; an absent backup permits index cleanup.

## Output and fields

All commands support Console and Raw TSV. `--raw` or redirected stdout selects Raw;
`--headers` includes column headers. Diagnostics and progress use stderr.

List and info accept `-C`, `--columns`, and repeated `--column`; explicit columns replace defaults:

| Letter | Field | list | info |
| --- | --- | --- | --- |
| N | name | yes | yes |
| I | id | yes | yes |
| T | time | yes | yes |
| H | head | yes | yes |
| B | backup | yes | yes |
| P | path | | yes |
| C | checksum | | yes |
| F | files | | yes |
| S | size | | yes |

- List defaults to name,time,head. `--details` selects all lightweight fields. Console asks before displaying over 80 entries.
- Info defaults to all fields. `--details` adds per-file records in Console only; reading a manifest is not full verification.
- HEAD is a baseline marker, not a clean marker. Unknown HEAD fields are empty and accompanied by diagnostics.
- Size is content bytes, excluding the manifest, not filesystem allocation. Raw booleans use true/false; times are UTC.
- Raw create/switch/rename/delete/verify columns: `operation target id name status`.
  Status is created, switched, unchanged, renamed, deleted, valid or failed.
- Raw status columns: `state id name expected_checksum actual_checksum change path`.
  Modified states emit one row per change (added/removed/modified); clean and untracked emit one row with empty change fields.
- TSV controls and backslashes use the existing escaping rules.
- Create, switch, status and verify accept positive `--parallelism N`, default 10.

## Storage and failures

- Index: `.mcpm/stash/index/<UUID>.json`; baseline: `.mcpm/stash/HEAD`.
- Backups: `.mcpm/data/<backup-UUID>/`; content cache: `.mcpm/stash/blob/`.
- Backups are self-contained and remain usable without the cache. Deleting a stash retains cached content.
- OS file locks serialize stash operations; busy locks fail immediately. Lock files persist and must not be removed manually.
- External programs writing mods do not participate in these locks.
- HEAD advances only after saving the index or successfully applying the backup. A failed HEAD update reports the already completed file operation.
- Application uses the existing backup rollback behavior. Directory replacement and HEAD updates are not one transaction.
- No interruption recovery or power-loss transaction guarantee is provided. Inspect status after interruption; commands never automatically resume a switch.
- Corrupt metadata is not silently rebuilt. Partial deletion may require inspecting the reported paths.

See [Mod file backups](../mod/backup.md) for backup integrity and format details.
