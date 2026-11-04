# Server commands

Entry points:

| Entry point | Command form |
| --- | --- |
| `mcpm` | `mcpm <command>` |
| `mcpm-server` | `mcpm-server <command>` |
| `mcpm-tools` | `mcpm-tools server <command>` |

Examples use `mcpm`. General terms, global options, and output modes are defined in [README](../README.md).

| Page | Content |
| --- | --- |
| [Basics](basics.md) | `info` and `init` |
| [Stash](stash.md) | `create`, `switch`, `status`, `list`, `info`, `rename`, `delete`, and `verify` |

## Target selection

| Command | Target argument | Default |
| --- | --- | --- |
| `info` | Positional PATH | `.` |
| `init` | Positional PATH; `--remote` makes PATH the local workspace | `.` |
| `stash` | `--server PATH`, accepted before or after the subcommand | `.` |

- Explicit `--ssh` selects the remote host and interprets PATH there.
- Without `--ssh`, `.mcpm/mcpm.json` decides the target: a Remote workspace redirects execution to its saved remote server path, anything else is a Local Server.
- `init --remote` always requires explicit `--ssh`; its PATH is the local workspace, not the server.
- Execution happens on the Host that owns the server: local execution calls the operation directly, remote execution goes through a reusable `mcpmd` link.
- Connection, registration, or target errors never fall back to local execution.
- Remote execution requires `mcpmd` on the remote machine with the same build identity as the local executables.

The commands documented here are migrated to operations, so they accept Remote workspaces and explicit `--ssh`. `launch` and `mods` remain on the legacy path and are documented under [legacy](../legacy/README.md).

## Output

Console and Raw are available everywhere; each command page defines its fields and Raw column order. Diagnostics and progress use stderr; the large-listing confirmation reads and writes the terminal directly. Redirecting stdout selects Raw.
