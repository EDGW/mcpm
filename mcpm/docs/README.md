# mcpm command reference

`mcpm` inspects local Minecraft clients and mod archives. The available commands are
`client list`, `client check`, `mod list`, `mod info`, `mod deps`, and `mod dep-graph build`. Mod archives
are read without modifying them; graph snapshots can be written to a separate file.

[Client list](client/list.md) · [client check](client/check.md)

[Mod commands](mod/README.md): [list](mod/list.md) · [info](mod/info.md) · [deps](mod/deps.md) · [dep-graph](mod/dep-graph.md)

[简体中文](../docs-zh/README.md)

## Quick start

```sh
mcpm client list "$HOME/Downloads/.minecraft"
mcpm mod list
mcpm mod list mods/ example.jar other-mods/
mcpm mod info example.jar another.jar.disabled
mcpm mod deps example.jar --loader fabric
mcpm --raw mod list mods/ -C MF > mods.tsv
```

To run from the repository without installing the executable, replace `mcpm`
with `cargo run --manifest-path mcpm/Cargo.toml --`.

## Global options

```text
mcpm [OPTIONS] <COMMAND>
```

| Option | Description |
| --- | --- |
| `--raw` | Treat output streams as non-terminal streams: disable colors, terminal layout, progress indicators, and terminal-only hints. |
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
mcpm mod list --help
mcpm mod info --help
mcpm mod deps --help
mcpm mod dep-graph build --help
mcpm mod dep-graph test --help
mcpm mod dep-graph view --help
```

## Terminal output

For terminal stdout, `list` aligns and wraps columns to the terminal width.
`info` and `deps` highlight mod identity and dependency targets; paths and
supplementary information use gray. Dependency labels use green for `required`,
blue for `recommended`, bright white for `suggested`, gray for `optional`,
red for `breaks`, and yellow for `conflicts`.

`list` and `info` print an ignored-file hint after a blank line when loader
filtering skips files. `list --expanded` also lists the skipped filenames.
These hints appear only on terminal stdout and are hidden with `--raw`.

Redirected or piped stdout has no colors or terminal-only hints. `list` then
uses unaligned, unwrapped, tab-separated columns. `--raw` applies this behavior
even in a terminal and also disables colored diagnostics and progress
indicators. Normal diagnostics use stderr; `list` progress indicators require
both stdout and stderr to be terminals.

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
