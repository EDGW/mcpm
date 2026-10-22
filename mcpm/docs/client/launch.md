# mcpm client launch

Prepare and launch one or more installed client versions. There is no `--all`.

```sh
mcpm client launch [VERSION ...] [--directory PATH] [OPTIONS]
mcpm client launch 1.18.1 --directory "$HOME/Downloads/.minecraft" --java /path/to/java
mcpm client launch "Pack A" "Pack B" --directory "$HOME/Downloads/.minecraft" --username Player
mcpm client launch --directory "$HOME/Downloads/.minecraft"
mcpm client launch 1.21.1 --max-memory 4096 --width 1280 --height 720
mcpm client launch 1.21.1 --jvm-arg=-XX:+UseG1GC --game-arg=--fullscreen
```

IDs are the version directory names shown by `client list`. Duplicate IDs are
removed, preserving argument order. Every selected version gets a preparation
and launch attempt before waiting for any child to exit. A failed version does
not prevent the others from launching. The command stays running until every
started child has been waited for; an unsuccessful child exit also fails the command.

Without IDs, a terminal selector opens: Up/Down selects one version, Enter
launches it, and Esc/Ctrl-C cancels successfully. Cancellation happens before
preparation or process creation. If any of stdin, stdout, or stderr is not a terminal, or `--raw` is set,
selection and styling are disabled. These callers must supply
IDs; launch never accepts `--all`.

`--directory` / `-d` selects the Minecraft root. The default is the last directory
named exactly `.minecraft` in pwd, or pwd itself. An inferred-root hint suggests
`mcpm client launch --directory .`; raw mode hides the hint.

Each version uses its own version directory as the default game and working
directory. Libraries/assets remain shared under the Minecraft root.
`--game-directory` overrides this; `--shared-game-directory` instead uses the root.
These two flags conflict. Relative directory overrides resolve against the root.

## Java and identity

| Option | Behavior |
| --- | --- |
| `--java PATH` | Configured Java executable; defaults to `java` on PATH. No automatic runtime discovery/download. |
| `--java-version-check minimum\|exact\|skip` | Default minimum; checks manifest majorVersion during preparation. |
| `--java-arch ARCH`, `--java-bits 32\|64` | Override host platform defaults for the selected JVM. Known architectures imply bitness unless explicitly overridden. |
| `--username NAME` | Defaults to offline `Player`. |
| `--uuid UUID` | Override offline UUID or supply the online account UUID. |
| `--access-token TOKEN` | Switch to supplied online identity; requires UUID. Also reads `MCPM_ACCESS_TOKEN`. |
| `--user-type TYPE` | Override default `legacy` offline / `msa` online. |
| `--xuid ID`, `--client-id ID` | Optional supplied account fields; require token. |

Tokens are not printed in launch status or help environment values. Login and
refresh are not performed. Use an existing token from the environment with:

```sh
mcpm client launch 1.21.1 --username Player --uuid ACCOUNT_UUID
# MCPM_ACCESS_TOKEN must already be set for online identity.
```

## Game and runtime configuration

| Options | Behavior |
| --- | --- |
| `--min-memory MIB`, `--max-memory MIB`, `--stack-size KIB` | Positive JVM sizes; no implicit memory override. Minimum cannot exceed maximum. |
| `--jvm-arg VALUE`, `--game-arg VALUE` | Repeatable argument appends. Quote spaces; use `--jvm-arg=-X...` for flag values. |
| `--main-class CLASS` | Override the resolved main class. |
| `--working-directory PATH` | Override only process cwd; defaults to game directory. |
| `--width N`, `--height N` | Positive window dimensions, required together. |
| `--fullscreen`, `--demo` | Enable corresponding game options. |
| `--server ADDRESS`, `--port PORT` | Legacy server connection; port requires server. |
| `--quick-play-singleplayer WORLD`, `--quick-play-multiplayer ADDRESS`, `--quick-play-realms ID` | Mutually exclusive; conflict with legacy server; require support in profile arguments. |
| `--libraries-directory PATH`, `--assets-directory PATH`, `--client-jar PATH` | Override installed resource locations. |
| `--natives-directory PATH` | Native directory; defaults to `versions/<version>/<version>-natives` and remains after exit. |
| `--use-existing-natives` | Requires native directory; disables extraction. |
| `--no-logging-config` | Omit manifest logging configuration. |
| `--verify none\|default\|strict\|shallow` | Optional pre-launch integrity checks. Default none; strict hashes all four resource categories, shallow checks sizes. |

The underlying check currently targets standard cache/JAR locations. Combining
`--verify` other than `none` with a custom libraries/assets/JAR location is rejected.
Required local files are checked for existence even without full verification.
There is no downloading, loader installation, or automatic dependency repair.

## Process output and lifecycle

| Options | Behavior |
| --- | --- |
| `--game-output inherit\|null` | Default inherit; null suppresses child stdout and stderr. Child stdin is null. |
| `--clear-env` | Clear inherited child environment before overrides. |
| `--env KEY=VALUE` | Repeatable child environment assignments. |
| `--unset-env KEY` | Repeatable removals; take precedence over assignments. |

Preparation uses the configured environment for the Java version probe as well.
If the environment is cleared, supply a usable PATH or an absolute Java path.

Terminal status uses cyan for preparation, green for successful start/exit, red
for unsuccessful exit, and white for version IDs/PIDs. `NO_COLOR` keeps plain
status without styling. Redirected and raw status has three TSV columns:
version, event (`preparing`, `started`, `exited`), detail. Inherited game output
shares the command's streams; use `--game-output null` when collecting status rows.
Diagnostics are written to stderr. Extracted natives remain in the version’s
`<version>-natives` directory after exit. Later launches extract into the same
directory and replace matching files.

Exit status is `0` when all selected processes exit successfully or selection is
cancelled, `1` for preparation/start/wait/output errors or unsuccessful child
exits, and `2` for invalid arguments. Check `mcpm client launch --help` for all flags.

## Diagnostic launch logs

Use the global `--log-level info` for planning, resource verification, Java
version checks, native extraction stages, process creation, PID and exit status.
`debug` adds routine completion messages, each native archive extraction,
platform/path/library decisions and the final redacted executable/argument list.
`trace` adds individual native entry
paths, skipped exclusions, sizes and required-file inspections. Default is `warn`.
Logs are written to stderr; game stdout/stderr retain `--game-output` behavior.
Tokens and common credential arguments are redacted; environment values are omitted.

```sh
mcpm client launch 1.18.1 --no-logging-config --log-level debug
```
