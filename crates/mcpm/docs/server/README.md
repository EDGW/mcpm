# Server commands

Entry points:

| Entry point | Command form |
| --- | --- |
| `mcpm` | `mcpm <command>` |
| `mcpm-server` | `mcpm-server <command>` |
| `mcpm-tools` | `mcpm-tools server <command>` |

Examples use `mcpm`. See [README](../README.md) for general terminology, global options, and output modes.

| Page | Content |
| --- | --- |
| [Create a server](new.md) | `new`; server creation, downloads, and optional mod loader setup. |
| [Add a mod loader](add-loader.md) | `add-loader`; in-place Forge, Fabric, or NeoForge installation on a Vanilla server. |
| [Basics](basics.md) | `info`, `info --upgrade`, `init`; explicit server identity and registration formats |
| [Launch and attach](launch.md) | `launch`, `attach`; server startup and monitoring. |
| [Stash](stash.md) | `create`, `switch`, `status`, `list`, `info`, `rename`, `delete`, `verify` |

## Target selection

| How to specify the target | Commands |
| --- | --- |
| Supply the path directly as the positional argument | `new`, `info`, `init` |
| Specify `--server PATH` | `add-loader`, `launch`, `attach`, `stash` |

When the target path is omitted, the current directory `.` is used.

See [legacy documentation](../legacy/README.md) for `mods` and other commands not yet included in this directory.

## Output

All commands support Console and Raw. Each command page defines its fields and Raw column order. Diagnostics and progress use stderr; confirmation before displaying large inventories reads and writes the terminal directly. Redirecting stdout selects Raw.

`info`, `init`, `new`, and `add-loader` share the same server summary, including identity, RCON status, and launch-configuration file status. RCON passwords are masked in the summary; `new` separately displays the full password after creating RCON settings.
