# mcpm command reference

The executables share one implementation and configuration:

| Executable | Commands |
| --- | --- |
| `mcpm`, `mcpm-server` | `init`, `info`, `mods` |
| `mcpmc`, `mcpm-client` | `list`, `check`, `launch` |
| `mcpm-tools` | `cache`, `remote`, `customize`, `debug`, `mod`, `server`, `client` |

Each long name has the same behavior as its short name; help and command hints use the selected name. Tools also exposes `mcpm-tools server info` and `mcpm-tools client list`. The server and client executables accept only their own command families; remote sessions use `mcpm-tools remote`.

Inspection commands read mod archives without modifying them. `mcpm-tools mod cache` explicitly stores files and metadata, with optional hard-link protection. Graph snapshots can be written to a separate file.

[Client list](client/list.md) · [client check](client/check.md) · [client launch](client/launch.md)

[Server init](server/init.md) · [server info](server/info.md) · [server mods](server/mods.md) · [server mod sources](server/source.md)

[Mod commands](mod/README.md): [list](mod/list.md) · [info](mod/info.md) · [cache](mod/cache.md) · [deps](mod/deps.md) · [dep-graph](mod/dep-graph.md)

[Customize presentation](customize.md) · [Debug palette](debug/palette.md)

[Persistent remote sessions](remote.md)

[Mod cache management](cache.md) · [Mod file backups](mod/backup.md)

[简体中文](../docs-zh/README.md)

## Quick start

```sh
mcpmc list "$HOME/Downloads/.minecraft"
mcpmc launch 1.18.1 --directory "$HOME/Downloads/.minecraft" --java /path/to/java
mcpm-tools mod list
mcpm-tools mod list mods/ example.jar other-mods/
mcpm-tools mod info example.jar another.jar.disabled
mcpm-tools mod deps example.jar --loader fabric
mcpm-tools --raw mod list mods/ -C MF > mods.tsv
```

Build all five executables with `cargo build --manifest-path mcpm/Cargo.toml --release --bins`.
`cargo run --manifest-path mcpm/Cargo.toml -- info` starts `mcpm` by default.
Select another executable with `--bin`, for example:

```sh
cargo run --manifest-path mcpm/Cargo.toml --bin mcpmc -- list
cargo run --manifest-path mcpm/Cargo.toml --bin mcpm-tools -- mod list
```

## Global options

`--ssh`, `--ssh-mcpm`, and `--remote-logs` are available only in the server and tools executables. SSH defaults to the remote `mcpm-tools` executable; explicit `--ssh-mcpm` paths are used as provided. `--ssh` and `--ssh-mcpm` are global; supported by `mod list`, `mod info`, `mod deps`, `mod source identify`, `server init`, `server info`, and `server mods list/info/cache/deps/source search/files/identify`. See [Remote sessions](remote.md).

```text
mcpm-tools [OPTIONS] <COMMAND>
```

| Option | Description |
| --- | --- |
| `--raw` | Force raw output: disable all TUI interaction, colors, terminal layout, progress indicators, and terminal-only hints. |
| `--log-level LEVEL` | Diagnostic logging on stderr: off, error, warn (default), info, debug, trace. Available before or after subcommands. |
| `--remote-logs` | Show remote logs on stderr, filtered by `--log-level`; hidden by default. Remote errors are always shown. |
| `-h`, `--help` | Show help for the current command. |
| `-V` | Show the executable version at the top level. |
| `--version` | Show the version, build identity (`official` / `unofficial`), build profile, Git commit ID, and build-time contributors (commit counts, names, and email addresses); mark the relevant information as unavailable if Git metadata could not be read. |

The detailed output shows the full commit ID. Staged or unstaged changes to tracked files append ` + modified`; untracked files do not count. This information is captured at build time and does not depend on the runtime directory.

Unofficial versions append `.<first six commit characters>`, or `.<first six commit characters>.modified` when tracked files have changes. If Git metadata is unavailable, they append `.unknown`. Both `-V` and `--version` use this version; official builds retain the Cargo package version.

Unofficial release-profile builds print a startup warning on stderr when executing commands: the program may be a third-party build or contain changes that have not been officially released. The warning does not change command results or exit status and remains visible with `--raw` or `--log-level off`. Debug builds omit the startup warning; `-V` prints only the executable name and version.

`--raw` is global and can appear before or after subcommands:

```sh
mcpm-tools --raw mod info example.jar
mcpm-tools mod list mods/ --raw
```

Use command-specific help to see the accepted arguments:

```sh
mcpm --help
mcpmc list --help
mcpmc check --help
mcpmc launch --help
mcpm-tools mod list --help
mcpm-tools mod info --help
mcpm-tools mod cache --help
mcpm-tools mod deps --help
mcpm-tools mod dep-graph build --help
mcpm-tools mod dep-graph test --help
mcpm-tools mod dep-graph view --help
```

## Terminal output

For terminal stdout, `list` aligns and wraps columns to the terminal width.
Key browsing content uses bold primary color; secondary content uses the secondary color.
Paths and descriptions use dim white. Warning, success and error use bright yellow,
green and red. Loaders use configured colors. See [`mcpm-tools customize`](customize.md).
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
NO_COLOR=1 mcpm-tools mod list mods/
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
mcpmc --log-level info launch 1.18.1 --no-logging-config
mcpmc launch 1.18.1 --no-logging-config --log-level debug
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
