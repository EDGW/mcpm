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
| `--mcpmd <PATH>` | Remote mcpmd executable; defaults to `mcpmd` on the remote environment PATH. |
| `--idle-timeout <SECONDS>` | Exit after this many seconds without clients or calls; `0` disables idle exit. Default `600`. |

Existing links keep their startup value; stop them to apply a changed setting.

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
