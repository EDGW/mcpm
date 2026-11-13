# `debug all-help`

Recursively list all command help for the current executable, for browsing or exporting command usage.

```text
mcpm debug all-help [OPTIONS]
mcpm-tools debug all-help [OPTIONS]
```

## Options

| Option | Description |
| --- | --- |
| `--raw` | Output plain text, suitable for saving to a file. |

## Examples

```sh
mcpm debug all-help
mcpm-tools debug all-help --raw > all-help.txt
mcpm-server debug all-help --raw > server-help.txt
```

## Output scope

- Includes the root command, all parent command groups, and leaf commands. Each parent precedes its descendants.
- Each section identifies its full command path and includes arguments, options, defaults, and supplementary help.
- `mcpm` and `mcpm-server` export the server command tree. `mcpm-tools` exports the complete Tools tree, including server, client, Mod, cache, and debug commands.
- Command aliases are not listed again.

Output goes to stdout. Console and Raw are supported; see [README](../README.md) for global options.
