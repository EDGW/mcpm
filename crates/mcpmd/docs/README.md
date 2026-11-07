# mcpmd

mcpmd serves remote operations and maintains reusable SSH links.

| Command | Content |
| --- | --- |
| [`mcpmd serve`](#mcpmd-serve) | Operation registry on stdin/stdout on the remote Host |
| [`mcpmd link`](#mcpmd-link) | Per-user link that reuses one SSH connection per target |

## `mcpmd serve`

```text
mcpmd serve
```

Reads operation requests from stdin and writes protocol frames to stdout. Stdout is reserved for protocol frames; diagnostics use stderr and log streams. mcpmd link starts this process through SSH; it is not normally invoked directly.

## `mcpmd link`

### Foreground link

```text
mcpmd link [--mcpmd <PATH>] [--idle-timeout <SECONDS>] SSH
```

| Argument or option | Description |
| --- | --- |
| `SSH` | SSH connection command, in the same form as `--ssh`. |
| `--mcpmd <PATH>` | Explicit remote mcpmd executable; overrides installer discovery. |
| `--idle-timeout <SECONDS>` | Exit after this many seconds without clients or calls; `0` disables idle exit. Default `600`. |

Existing links keep their startup value; stop them to apply a changed setting.

### Remote executable discovery

- With no explicit path, probe the remote command interpreter. The local OS does not select the remote lookup rules.
- POSIX hosts read `~/.mcpm/mcpmd.path`; Windows hosts read `%LOCALAPPDATA%\mcpm\mcpmd.path`. Windows SSH must use cmd as its default shell and have Windows PowerShell available.
- The record is UTF-8 text containing one absolute executable path, optionally followed by LF or CRLF. Its contents are data, never shell code.
- A missing record emits a warning and tries `mcpmd` on the remote PATH. Only a failed fallback makes the missing-record case an error. Warnings are forwarded to the launching mcpm process, independently of `--remote-logs`.
- Unreadable or malformed records, missing registered executables, and daemon startup failures are errors; they do not select a different executable.
- Missing remote executables produce an English Tip in mcpm's diagnostic output, with non-interactive SSH/PATH guidance. Missing mcpmd additionally explains the installer record and suggests reinstalling if that record is not taking effect. Ordinary missing data files and authentication failures do not receive this Tip.

### `mcpmd link list`

```text
mcpmd link list [--raw]
```

| Option | Description |
| --- | --- |
| `--raw` | Force TSV output. |

Unresponsive records are marked stale and removed.

### `mcpmd link stop`

```text
mcpmd link stop (SSH | --all) [--force] [--mcpmd <PATH>]
```

| Argument or option | Description |
| --- | --- |
| `SSH` | SSH connection command, in the same form as `--ssh`. Omit only with `--all`. |
| `--all` | Stop all links. |
| `--force` | Abort active calls immediately instead of waiting. |
| `--mcpmd <PATH>` | Remote mcpmd path; when omitted, match a registered link by SSH command. |

Rebuilding does not replace a running link. On an incompatible version, stop the matching link and retry with matching builds.

[mcpm commands](../../mcpm/docs/README.md) · [Backend modules](../../mcpm-backend/docs/README.md) · [简体中文](../docs-zh/README.md)
