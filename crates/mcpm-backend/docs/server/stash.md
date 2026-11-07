# Stash operations

A stash is a saved snapshot of a server's `mods/` directory. Each snapshot has a name, UUID, and
creation time, and records every file's path, size, and content checksum. With this information you
can compare the workspace with a snapshot, check whether a snapshot is intact, or restore it.

**HEAD** is the snapshot the workspace was most recently saved to or switched to; `status` uses it
as the comparison baseline. HEAD only says where the workspace started; it does not mean the
workspace is unmodified.

## Operations

| Operation | Purpose |
| --- | --- |
| `server.stash.create` | Save the current `mods/` as a new snapshot and make it HEAD |
| `server.stash.switch` | Replace `mods/` with a snapshot's content and make it HEAD |
| `server.stash.status` | Show how `mods/` differs from HEAD |
| `server.stash.list` | List all snapshots |
| `server.stash.info` | Show details for the selected snapshots |
| `server.stash.rename` | Change one snapshot's name |
| `server.stash.delete` | Delete the selected snapshots |
| `server.stash.verify` | Check whether the selected snapshots are still intact |

## Selecting snapshots

`switch` and `rename` act on one snapshot; `info`, `delete`, and `verify` act on a group. Each
snapshot is selected by exact name or UUID; a name is never interpreted as a UUID.

When selecting a group:

- Items are processed in the given order.
- A snapshot selected multiple times (for example by name and then by UUID) is processed once.
- An unresolvable snapshot fails only its own item; the rest are still processed.

## Progress

`create`, `switch`, `status`, and `verify` report progress throughout the work and emit one final
result; the number of files processed concurrently can be bounded. When the caller stops reading,
the operation stops afterward and files already written are kept.

## Concurrency

`create`, `switch`, `rename`, and `delete` modify snapshots, so they cannot run alongside other
stash operations on the same server; read-only operations may run together. A busy server fails
immediately instead of waiting in a queue.

## Failures

A failure either aborts the whole operation or affects only one item.

| Operation | Item failures |
| --- | --- |
| `list`, `info` | Unreadable snapshots and an unreadable HEAD are reported in place; the remaining records are still returned |
| `verify` | An unreadable HEAD is reported once; each snapshot has its own result |
| `delete` | Each snapshot has its own result |

Item failures retain the complete original error chain; see the
[mcpm-exec error model](../../../mcpm-exec/src/docs/error.md). All other failures make the whole
operation fail. A dropped connection does not undo files already written or HEAD changes already
made.

## `create`

Checksums the current workspace, saves it as a newly named snapshot, and makes it HEAD. A workspace
identical to HEAD is refused, because another identical snapshot carries no new information.

If saving fails and cleanup of partially written data also fails, the error reports both and points
to the leftover backup directory.

## `switch`

Restores the snapshot to `mods/` and makes it HEAD.

- The snapshot is fully verified first, even with `force` and even when it is already HEAD, so a
  snapshot with missing or corrupt content never overwrites the workspace.
- Replacement is whole-tree: files absent from the snapshot are deleted.
- When the workspace differs from HEAD, or no HEAD exists, replacement happens only with `force`.
  `force` only permits discarding modifications; it does not skip verification.
- The current state is not saved automatically. To keep current modifications, run `create` first.
- The result states whether replacement actually happened.

## `status`

Compares `mods/` with HEAD file by file. A file present in HEAD but absent from the workspace is
missing; one present in the workspace but absent from HEAD is unexpected; differing content is
changed, with both records reported. Without HEAD there is nothing to compare with, and the result
says so. `status` only reads the workspace and saved checksums; it writes nothing.

## `list`

Returns each snapshot's name, UUID, creation time, and backup location without hashing the
workspace or opening any backup. Ordering is the caller's concern.

## `info`

Returns the same information as `list` for the selected snapshots. When `include_manifest` is set,
it also returns each snapshot's checksum, file list, and size. Reading this information is not
verification.

## `rename`

Changes a snapshot's name; the UUID, creation time, content, and HEAD status stay the same.
Renaming to the existing name succeeds immediately. Unlike the list operations, a failure fails the
whole operation.

## `delete`

Deletes the selected snapshots; `mods/` and content shared with other snapshots are unaffected.

- HEAD cannot be deleted. If the selection contains HEAD, or HEAD cannot be read, no snapshot is
  deleted and the whole operation fails.
- Otherwise each snapshot has its own result, and partial failures can be retried.
- A snapshot whose backup no longer exists can still be deleted.

## `verify`

Checks whether the selected snapshots are still intact: the backup format is valid, every file in
the manifest exists with the same size, and the complete checksum matches. It reads only content
saved on this server, never uses the global cache, and does not look at the current `mods/`.
Snapshots are checked one by one. HEAD is read only to report that it is unreadable, which does not
prevent checking the selected snapshots.
