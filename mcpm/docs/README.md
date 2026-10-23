# mcpm command reference

`mcpm` inspects and launches local Minecraft clients, and inspects mod archives. The available commands are
`client list`, `client check`, `client launch`, `server init`, `server info`, `mod list`, `mod info`, `mod cache`, `mod deps`, and `mod dep-graph build`. Mod archives
are read without modifying them by inspection commands; `mod cache` explicitly stores files and metadata, with optional hard-link protection. Graph snapshots can be written to a separate file.

[Client list](client/list.md) · [client check](client/check.md) · [client launch](client/launch.md)

[Server init](server/init.md) · [server info](server/info.md) · [server mods](server/mods.md) · [server mod sources](server/source.md)

[Mod commands](mod/README.md): [list](mod/list.md) · [info](mod/info.md) · [cache](mod/cache.md) · [deps](mod/deps.md) · [dep-graph](mod/dep-graph.md)

[Customize presentation](customize.md) · [Debug palette](debug/palette.md)

[Persistent remote sessions](remote.md)

[Mod cache management](cache.md)

[简体中文](../docs-zh/README.md)

## Quick start

```sh
mcpm client list "$HOME/Downloads/.minecraft"
mcpm client launch 1.18.1 --directory "$HOME/Downloads/.minecraft" --java /path/to/java
mcpm mod list
mcpm mod list mods/ example.jar other-mods/
mcpm mod info example.jar another.jar.disabled
mcpm mod deps example.jar --loader fabric
mcpm --raw mod list mods/ -C MF > mods.tsv
```

To run from the repository without installing the executable, replace `mcpm`
with `cargo run --manifest-path mcpm/Cargo.toml --`.

## Global options

`--ssh` and `--ssh-mcpm` are global; supported by `mod list`, `mod info`, `mod deps`, `mod source identify`, `server init`, `server info`, and `server mods list/info/cache/deps/source search/files/identify`. See [Remote sessions](remote.md).

```text
mcpm [OPTIONS] <COMMAND>
```

| Option | Description |
| --- | --- |
| `--raw` | Force raw output: disable all TUI interaction, colors, terminal layout, progress indicators, and terminal-only hints. |
| `--log-level LEVEL` | Diagnostic logging on stderr: off, error, warn (default), info, debug, trace. Available before or after subcommands. |
| `--remote-logs` | Show remote logs on stderr, filtered by `--log-level`; hidden by default. Remote errors are always shown. |
| `-h`, `--help` | Show help for the current command. |
| `-V`, `--version` | Show the executable version at the top level. |

`--raw` is global and can appear before or after subcommands:

```sh
mcpm --raw mod info example.jar
mcpm mod list mods/ --raw
```

Use command-specific help to see the accepted arguments:

```sh
mcpm --help
mcpm client list --help
mcpm client check --help
mcpm client launch --help
mcpm mod list --help
mcpm mod info --help
mcpm mod cache --help
mcpm mod deps --help
mcpm mod dep-graph build --help
mcpm mod dep-graph test --help
mcpm mod dep-graph view --help
```

## Terminal output

For terminal stdout, `list` aligns and wraps columns to the terminal width.
Key browsing content uses bold primary color; secondary content uses the secondary color.
Paths and descriptions use dim white. Warning, success and error use bright yellow,
green and red. Loaders use configured colors. See [`mcpm customize`](customize.md).
Dependency labels use bold white for `required`, secondary color for `recommended`,
white for `suggested`, dim white for `optional`, error color for `breaks`, and warning color for `conflicts`.

`list` and `info` print an ignored-file hint after a blank line when loader
filtering skips files. `list --expanded` also lists the skipped filenames.
These hints appear only on terminal stdout and are hidden with `--raw`.

A shared terminal policy disables styling and interaction with `--raw`.
Redirected stdout automatically uses Raw result formatting. Stderr independently
controls diagnostic colors and progress; stdin does not determine the mode.
Raw lists use unaligned, unwrapped, tab-separated columns. Client commands require
explicit version IDs (or `check --all`) when selection is disabled.

Setting `NO_COLOR` disables colors while retaining terminal layout and hints:

```sh
NO_COLOR=1 mcpm mod list mods/
```

## Exit status

| Status | Meaning |
| --- | --- |
| `0` | Completed successfully, including empty results or files skipped by loader filtering. Help and version requests also succeed. |
| `1` | An operational error occurred, such as an invalid path, unreadable archive, failed identification, or output failure. Partial results may have been printed. |
| `2` | Command-line parsing failed, such as an unknown option, missing required argument, or invalid option value. |

## Diagnostic logging

`--log-level` configures stderr logging through `tracing-subscriber`. Default `warn` keeps
ordinary successful operations quiet. Use `info` for current steps and important
events such as game process startup and exit. Routine completion, session cleanup,
transfer statistics, selected libraries, paths and the final redacted Java argument list use `debug`,
and `trace` for individual native entries and required-file inspections.

```sh
mcpm --log-level info client launch 1.18.1 --no-logging-config
mcpm client launch 1.18.1 --no-logging-config --log-level debug
```

Logs use stderr and cooperate with progress bars. Colors follow terminal policy,
`NO_COLOR`, and `--raw`. Log filtering does not hide normal command results or
operational error diagnostics and does not filter the game's stdout/stderr.
Each run also creates `~/.mcpm/logs/YYYY-MM-DD/HH-MM-SS.nnnnnnnnn.log`, using
the local process start date and time with Windows-compatible filenames. The
same file remains open across midnight. A UUID suffix resolves simultaneous-start
filename collisions without overwriting existing logs. File logging records through `debug`
(error, warn, info, debug), independently of `--log-level`, including when it is
`off`. Files include timestamps, module targets and error chains, without colors.
Remote logs are hidden on stderr by default. Enable the global `--remote-logs`
option to display them; remote logs also follow `--log-level`. Operational errors
from remote commands always appear as `remote error:`, independently of both
options. File logging and failure exit statuses are unaffected.
Remote processes write logs under the remote user's home directory as well.
Identity tokens and known credential arguments are redacted from command logs;
environment variable values are not logged.
