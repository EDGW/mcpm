# Stash commands

Save and restore snapshots of a server's `mods/` directory. Target selection follows [README](README.md).

A snapshot has a name, UUID, and creation time, and records the content of every file under `mods/`. After saving, you can compare the current `mods/` with a snapshot, check whether a snapshot is intact, or restore one at any time. Snapshots belong to the server itself and support Local Servers, explicit SSH targets, and Remote workspaces.

## Family options

```text
mcpm stash [OPTIONS] <COMMAND>
```

| Option | Description |
| --- | --- |
| `--server <PATH>` | Server directory, or a registered Remote workspace. Defaults to `.`; accepted before or after the subcommand. |
| `--headers` | Include Raw TSV column headers; ignored in Console. |

## Common rules

### What is saved

- A snapshot includes every regular file under `mods/`, including disabled mods and non-JAR files; empty directories are not saved.
- Worlds, configuration, launch scripts, Minecraft versions and loader declarations are not part of a snapshot and do not change when switching.
- Do not modify `mods/` during creation or switching.
- Cache management commands are refused by default when they would modify the server's stash cache; `--no-protect-stash` bypasses the guard. See [stash cache protection](../legacy/cache.md).

### HEAD and workspace state

**HEAD** is the snapshot the workspace was most recently saved to or switched to; `status` and `switch` use it as the baseline. HEAD only says which snapshot the workspace started from; it does not mean the workspace is unmodified.

`status` compares `mods/` with HEAD and reports one of three states:

| State | Meaning | `switch` requires `--force` |
| --- | --- | --- |
| `clean` | Identical to HEAD | No; switching to HEAD itself needs no replacement |
| `modified` | Differs from HEAD | Yes |
| `untracked` | No HEAD yet (never saved) | Yes |

All three states are successful results and the command exits 0; only a failure in the check itself returns nonzero.

### Selecting snapshots

`switch` and `rename` require exactly one snapshot; `info`, `delete`, and `verify` may take several:

| Option | Meaning |
| --- | --- |
| `--name <NAME>` | Exact name; repeatable. Commas are part of the name |
| `--names <NAME,NAME>` | Comma-separated names |
| `--id <UUID>` | Complete UUID; repeatable |
| `--ids <UUID,UUID>` | Comma-separated UUIDs |
| `--input name\|id` | Interpret every nonblank stdin line as a name or UUID. Required when stdin has nonblank lines; the type is never inferred |

`switch` and `rename` accept only one of `--name` or `--id`.

```sh
mcpm stash info --name stable --name experimental
mcpm stash verify --ids UUID1,UUID2
printf '%s\n' stable experimental | mcpm stash info --input name
mcpm stash delete --id UUID1 --id UUID2
```

When selecting several snapshots:

- Processing order is `--name`, `--names`, `--id`, `--ids`, then stdin; each group keeps its input order.
- A snapshot selected multiple times (for example by name and then by UUID) is processed once.
- One item failing does not prevent later items, but the command returns nonzero if any item failed.

Names are unique and case-sensitive. Spaces and Unicode are allowed; whitespace-only names and control characters are rejected.

### Output modes

All commands support Console and Raw. `--raw`, or a redirected stdout, selects Raw (TSV); `--headers` includes headers. Diagnostics and progress use stderr.

### Fields

`list` and `info` select fields by letter:

| Letter | Field | Meaning | list | info |
| --- | --- | --- | --- | --- |
| N | name | Name | ✓ | ✓ |
| I | id | UUID | ✓ | ✓ |
| T | time | Creation time (UTC) | ✓ | ✓ |
| H | head | Whether this is HEAD (true/false) | ✓ | ✓ |
| B | backup | Backup UUID | ✓ | ✓ |
| P | path | Backup location | | ✓ |
| C | checksum | Whole-snapshot checksum | | ✓ |
| F | files | File count | | ✓ |
| S | size | Total content bytes, excluding the manifest; not disk usage | | ✓ |

- `list` accepts `NITHB`; `info` accepts `NITHBPCFS`. `--columns` takes comma-separated field names; `--column` takes one field at a time.
- `list` defaults to name, time, and head; `info` defaults to all fields. Explicit selection replaces the defaults.
- When HEAD cannot be read, the `head` field is empty and an error is reported.

### Result rows

In Raw, `create`, `switch`, `rename`, `delete`, and `verify` write one row per result:

```text
operation target id name status
```

| Command | `target` | `status` |
| --- | --- | --- |
| `create` | Snapshot UUID | `created` |
| `switch` | Snapshot UUID | `switched` or `unchanged` |
| `rename` | Snapshot UUID | `renamed` |
| `delete` | The name or UUID you provided | `deleted` or `failed` |
| `verify` | The name or UUID you provided | `valid` or `failed` |

Failed rows leave `id` and `name` empty. Console output is described in each command's `### Output`.

### Storage and safety

- Snapshot data lives under the server directory's `.mcpm/`: `.mcpm/stash/` stores snapshot records, HEAD, and the content cache; `.mcpm/data/<backup UUID>/` stores the backup manifest for that snapshot.
- When moving or backing up a server, take `.mcpm/stash/` and `.mcpm/data/` together.
- On one server, commands that modify snapshots (`create`, `switch`, `rename`, `delete`) cannot run alongside other stash commands; a conflict fails immediately instead of waiting. Lock files under `.mcpm/stash/` must be kept long-term and never removed manually.
- Other programs modifying `mods/` are not subject to this restriction; avoid changing `mods/` during creation and switching.
- Verification and switching use only the server's own stash cache, never the global cache under `~/.mcpm/cache/mods`, and never create a missing cache.

## `mcpm stash create`

```text
mcpm stash create [OPTIONS] <NAME>
```

Save the current `mods/` and make it HEAD.

### Arguments

| Argument | Description |
| --- | --- |
| `<NAME>` | Unique, exact, case-sensitive snapshot name. |

### Options

| Option | Description |
| --- | --- |
| `--parallelism <N>` | Maximum concurrently processed files; positive integer, default 10. |

### Output

Console shows the operation, snapshot name, and result, with the snapshot UUID on the next line. Raw writes one result row with `status` set to `created`.

A workspace identical to HEAD is refused, because another identical snapshot adds nothing; a workspace with no HEAD can be saved directly.

### Examples

```sh
mcpm stash create "stable"
mcpm stash create "experimental" --parallelism 4
```

## `mcpm stash switch`

```text
mcpm stash switch [OPTIONS] <--name <NAME>|--id <UUID>>
```

Restore `mods/` from the selected snapshot and make it HEAD.

### Options

| Option | Description |
| --- | --- |
| `--name <NAME>`, `--id <UUID>` | Select one snapshot; use exactly one. |
| `--force` | Permit discarding modifications in `modified` or `untracked` states. |
| `--parallelism <N>` | Maximum concurrently processed files; positive integer, default 10. |

### Output

Console shows the operation, snapshot name, and result, with the snapshot UUID on the next line. Raw writes one result row with `status` set to `switched`. When the workspace matches HEAD and the selected snapshot is HEAD, no files change and `status` is `unchanged`.

### Behavior

- Switching always fully verifies the snapshot first, including with `--force` and when the target is HEAD. A snapshot with missing or corrupt content is refused and the existing `mods/` is left intact.
- Switching replaces the whole tree: files absent from the snapshot are deleted.
- `--force` only permits discarding modifications in `modified` or `untracked` states; it never skips verification, and never ignores a corrupt snapshot, an invalid HEAD, or errors while reading the workspace.
- Using `--force` with HEAD discards modifications and restores the saved version.
- Overwritten modifications are not saved automatically. Run `create` first to keep them.
- HEAD is updated only after switching succeeds. When switching succeeds but the HEAD update fails, the error states that the files were already switched.

### Examples

```sh
mcpm stash switch --name stable
mcpm stash switch --id UUID1
# Explicitly discard unsaved modifications
mcpm stash switch --name stable --force
```

## `mcpm stash status`

```text
mcpm stash status [OPTIONS]
```

Show how `mods/` differs from HEAD: added, removed, and modified files.

### Options

| Option | Description |
| --- | --- |
| `--parallelism <N>` | Maximum concurrently processed files; positive integer, default 10. |

### Output

Console first shows `Status:` and the workspace state; when HEAD exists, it then shows the HEAD name and UUID, followed by each `Missing`, `Unexpected`, or `Changed` file. For changed files, it shows only the size or BLAKE3 values that actually differ.

Raw columns:

```text
state id name expected_checksum actual_checksum change path
```

In `modified`, each changed file gets one row with `change` set to `added`, `removed`, or `modified`; `clean` and `untracked` each emit one row with empty `change` and `path`.

### Examples

```sh
mcpm stash status
```

## `mcpm stash list`

```text
mcpm stash list [OPTIONS]
```

List all snapshots from newest to oldest. It does not inspect `mods/` or open any backup, so it is fast; HEAD appears only as a marker.

### Options

| Option | Description |
| --- | --- |
| `-C <LETTERS>`, `--columns <LIST>`, `--column <FIELD>` | Select output fields; explicit selection replaces the defaults. |
| `--details` | Show every lightweight field that `list` supports. |

### Output

Fields are written in the order of the [fields](#fields) table. Console shows one line per snapshot by default, with the name and creation time and a `HEAD` marker for the HEAD snapshot; explicit fields or `--details` switch to labeled fields. More than 80 entries prompts Y/n first; redirected stdout never prompts.

### Examples

```sh
mcpm stash list
mcpm stash list --columns name,time,head
mcpm stash list --details
```

## `mcpm stash info`

```text
mcpm stash info [OPTIONS]
```

Show details for the selected snapshots. Some fields (checksum, file count, size) need to read the snapshot's file manifest, which is not the same as verifying file contents.

### Options

| Option | Description |
| --- | --- |
| `--name`, `--names`, `--id`, `--ids`, `--input` | Select one or more snapshots; see [selecting snapshots](#selecting-snapshots). |
| `-C <LETTERS>`, `--columns <LIST>`, `--column <FIELD>` | Select output fields; explicit selection replaces the defaults. |
| `--details` | In Console, append each file's name, BLAKE3, and size. |

### Output

Fields are written in the order of the [fields](#fields) table. Console shows labeled fields; `--details` applies only to Console, even when fields are also selected.

### Examples

```sh
mcpm stash info --name stable
mcpm stash info --name stable --details
```

## `mcpm stash rename`

```text
mcpm stash rename [OPTIONS] <--name <NAME>|--id <UUID>> <NEW_NAME>
```

Change one snapshot's name. Only the name changes; the UUID, creation time, content, and HEAD status stay the same.

### Arguments

| Argument | Description |
| --- | --- |
| `<NEW_NAME>` | New unique name, subject to the same rules as snapshot names. |

### Options

| Option | Description |
| --- | --- |
| `--name <NAME>`, `--id <UUID>` | Select one snapshot; use exactly one. |

### Output

Console shows the operation, snapshot name, and result, with the snapshot UUID on the next line. Raw writes one result row with `status` set to `renamed`. Renaming to the snapshot's current name succeeds immediately without writing.

### Examples

```sh
mcpm stash rename --name stable "long-term stable"
```

## `mcpm stash delete`

```text
mcpm stash delete [OPTIONS]
```

Delete the selected snapshots. `mods/` is unaffected, and content shared with other snapshots is kept.

### Options

| Option | Description |
| --- | --- |
| `--name`, `--names`, `--id`, `--ids`, `--input` | Select one or more snapshots; see [selecting snapshots](#selecting-snapshots). |

### Output

Console shows the operation, snapshot name, and result for each snapshot, with the snapshot UUID on the next line; a failed item shows the name or UUID you provided and has no UUID line. Raw writes one row per snapshot with `status` set to `deleted` or `failed`.

### Behavior

- There is no extra confirmation and no `--force`.
- If the selection contains HEAD, or HEAD cannot be read, nothing is deleted.
- Otherwise each snapshot is deleted independently; one failure does not affect the rest, and failed items can be retried.

### Examples

```sh
mcpm stash delete --name experimental
mcpm stash delete --ids UUID1,UUID2
```

## `mcpm stash verify`

```text
mcpm stash verify [OPTIONS]
```

Check whether the selected snapshots are intact: backup format, file manifest, each file's size, and complete BLAKE3 checksums. It checks only saved content, never compares the current `mods/`, and modifies no files.

### Options

| Option | Description |
| --- | --- |
| `--name`, `--names`, `--id`, `--ids`, `--input` | Select one or more snapshots; see [selecting snapshots](#selecting-snapshots). |
| `--parallelism <N>` | Maximum concurrently processed files; positive integer, default 10. |

### Output

Console shows the operation, snapshot name, and result for each snapshot, with the snapshot UUID on the next line; a failed item shows the name or UUID you provided and has no UUID line. Progress goes to stderr. Raw writes one row per snapshot with `status` set to `valid` or `failed`.

One item failing does not stop later checks; an unreadable HEAD is reported and the selected snapshots are still checked.

### Examples

```sh
mcpm stash verify --name stable
mcpm stash verify --ids UUID1,UUID2 --parallelism 4
```

## Failures and interruption

- No interruption recovery is provided, and consistency across a power loss is not guaranteed. After an interruption, run `status` first; a command never resumes a previous switch automatically.
- When `create` fails to publish and cleanup also fails, the error reports both and points to the leftover unreferenced backup directory.
- Committed files and HEAD changes are not rolled back when a connection drops.
- Corrupt snapshot records, HEAD, or manifests are reported as errors; after a partial deletion, follow the error to inspect the current state.

See [Mod file backups](../legacy/mod/backup.md) for backup format and integrity details.
