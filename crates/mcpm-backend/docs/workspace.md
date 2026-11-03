# Workspace operations

A workspace is the caller-local entry used to select a target. A Local workspace uses a local directory; a Remote workspace (stub) stores an SSH connection and remote path and uses a remote target.

Workspace operations are Shallow and run on the caller's machine.

## Operations

| Operation | Classification | Input | Output |
| --- | --- | --- | --- |
| `workspace.inspect` | Shallow | `InspectInput { path }` | `WorkspaceInfo { kind, connection, remote_path }` |
| `workspace.register_remote` | Shallow | `RegisterRemoteInput { path, connection, remote_path, force }` | `()` |

## `workspace.inspect`

Reads local workspace registration and returns its type. A Remote result contains the connection and remote path; a Local result has `connection` and `remote_path` set to `None`.

| Situation | Behavior |
| --- | --- |
| `.mcpm/mcpm.json` is missing | Treated as the legacy Local layout |
| Type is Local | Returns Local |
| Type is Remote | Reads version-3 `.mcpm/server.json` and returns the connection and remote path |
| Marker is damaged, unreadable, or unsupported | Returns an error |

It reads only registration; it does not start SSH or read remote server metadata.

## Target selection

| Situation | Target |
| --- | --- |
| Explicit `--ssh` | The specified host; the supplied path is interpreted there |
| No explicit `--ssh`; path is a Remote workspace | Remote connection and path from registration |
| No explicit `--ssh`; path is Local or unregistered | Local machine |
| No path | Explicit `--ssh` or local; workspace is not inspected |

Explicit `--ssh` takes precedence over saved registration. Connection or registration failure does not select another target.

## `workspace.register_remote`

It canonicalizes an existing local directory, acquires the registration lease, and writes the connection stub and Remote type marker:

| File | Content |
| --- | --- |
| `.mcpm/server.json` | Version 3: `ssh`, `ssh_mcpmd`, `path` |
| `.mcpm/mcpm.json` | Version 1, `type: remote` |
| `.mcpm/.gitignore` | Metadata ignore rules |

`force` allows replacing an existing Remote connection. It cannot convert an existing Local workspace to Remote or replace a configuration path that is a directory. Concurrent registration changes fail immediately as busy.

The two JSON files are individually atomic, not a cross-file transaction. If the first type-marker publication fails, the new connection registration is removed; abrupt termination may leave registration that cannot be loaded.

The registration lease covers the write sequence and is released when the Writer ends.

## Remote stub creation flow

1. Host `server.init.prepare_remote` validates the remote Local Server and performs Git preparation.
2. Shallow `workspace.register_remote` writes the local connection registration.

The stub performs no Git operations and republishes no remote server configuration. Failure of step 2 does not roll back Git preparation completed in step 1.
