# `mcpm server init`

```sh
mcpm server init [PATH] [OPTIONS]
mcpm server init /path/to/server
mcpm server init --start-script ./start.sh
mcpm server init --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
mcpm server init --minecraft-version 1.21.1 --loader vanilla
```

Defaults to `run.bat` on Windows and `run.sh` on Unix in the server directory on the data-source host (the SSH host with `--ssh`).
`--start-script FILE` accepts a path relative to that directory or an absolute
path, and conflicts with manual `--minecraft-version` (alias `--mc-version`),
`--loader`, and `--loader-version`. Loader choices are `forge`, `neoforge`,
`fabric`, and `vanilla`. Complete manual metadata registers directly. Incomplete
metadata is filled by inspecting the default script and filtering verified
candidates against every supplied field; no match fails. Vanilla cannot have a
loader version.

An existing `.mcpm/server.json`, even invalid JSON, blocks writing unless `--force` is supplied. Recognition itself does not inspect existing registration metadata.
`--force` allows atomic replacement after successful verification and selection;
failure before writing or cancellation of candidate selection preserves the previous file. `--force`, `--full-command`,
the directory argument, and global options can accompany `--start-script`.

Inspection never runs scripts, Java, installers, or downloads. Static syntax
includes literal assignments, quoting, continuations, directory changes,
shell source/child-script references and batch `call`, `%~dp0`, and basic
`setlocal`. `$(dirname "$0")` is the only supported command substitution;
using `$0` after changing directories is rejected. Script invocation arguments
are empty: conventional trailing `"$@"` is omitted, and batch `%*` expands to
empty wherever it occurs. The inspector does not import environment variables.
Dynamic conditions, loops, redirection, pipes, arbitrary substitutions, delayed
batch expansion, unresolved variables, and recursive references fail.

Java argument files are flattened. Recognized classpath/module-path options,
entry classes, `-jar`, and relevant properties drive identification; unknown JVM
options are retained and ignored. Their validity is not checked. Unknown options
with separate operands may still prevent accurate entry parsing, since their
arity cannot be guessed. Loader detection cross-checks referenced Maven artifacts,
identity markers inside JARs, and Minecraft version metadata. It does not pick
the newest cached library. Supported layouts include common Forge/NeoForge
argument files, Forge shims, both Fabric installer layouts, and vanilla JARs
with version metadata. Older JARs without metadata, third-party wrappers, and
unsupported layouts fail. Recognition verifies consistent declared identity,
not authenticity, JVM argument validity, or launchability.

A unique verified candidate initializes immediately. Multiple candidates open a
TUI: Up/Down selects, Enter confirms, Esc/Ctrl-C cancels. Multiple candidates fail
with `--no-ui`, raw output, or non-TTY stdout/stderr; use a more specific script or manual version filters. Unavailable launches do not discard other recognized candidates; when no candidate survives, recognition fails. Missing
loader evidence never proves vanilla. Failure explicitly reports an unrecognized
server and asks for manual initialization with complete version and loader options;
use `--loader vanilla` explicitly for a known vanilla server.

The selected script, JVM directory, executable and flattened argv are saved as a
snapshot. Successful output matches [`server info`](info.md). Non-raw command
display fits one terminal line with `(+N)` omitted-character count. Use
`--full-command` to display it all. Commands are gray; path values use plain color.

## SSH and initialization protocol

```sh
mcpm --ssh 'ssh user@host' server init /srv/minecraft --minecraft-version 1.21.1 --loader vanilla
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm.exe' server init 'D:\server' --start-script run.bat
```

Complete manual arguments invoke `server.init` directly. Otherwise the client invokes `server.init.recognize`, selects a candidate locally, then invokes `server.init` on the same SSH session. Console and Raw require a unique candidate. TUI selection uses local stdout/stderr terminal policy and does not inspect stdin to choose a mode. `--no-ui` disables selection. Output uses the same local formatter as `server info`; phase spinners use stderr.

`server.init.recognize` is independent and read-only: it reads scripts and identity evidence, ignores existing registration metadata and never writes configuration or creates a mods directory. It accepts `path` (default `.`), optional `start_script`, and optional `minecraft_version`, `loader`, `loader_version` filters. The default script is chosen by the file-owning host. It emits:

```json
{"event":"stage","phase":"recognizing"}
{"event":"candidates","candidates":[{"metadata":{"version":1,"minecraft_version":"1.21.1","loader":null,"start":{"script":"/srv/minecraft/run.sh","directory":"/srv/minecraft","executable":"java","arguments":["-jar","server.jar"],"windows":false}},"line":1,"main_class":"-jar"}]}
```

`server.init` accepts complete data with no recognition token or cached server-side state:

```json
{"path":"/srv/minecraft","metadata":{"version":1,"minecraft_version":"1.21.1","loader":null},"force":false}
```

`metadata` uses the persisted server metadata schema. Its `version` and nonempty `minecraft_version` are required. `loader` must be explicit: `null` for vanilla, or `{ "type": "fabric", "version": "0.16.10" }` (also Forge/NeoForge). Optional `start` contains the complete script, directory, executable, argument array and Windows quoting flag shown above. Script/directory/executable must be nonempty; executable and arguments cannot contain NUL. It is a snapshot: initialization neither rereads the script nor checks whether its referenced files still exist or are launchable. Paths and argv retain the originating host's representation.

The command validates the data and current destination, then emits `stage` events with `validating` and `writing`, followed by `initialized {server}` using the `server.info` snapshot schema. Success is followed by the session's `finished` status. Failed validation, recognition or persistence uses a terminal `error`; no result follows. Clients validate phase order and exactly one matching result. Progress describes phases, with no artificial percentage.

Writing reuses atomic metadata persistence. Without `force`, even a registration created after validation cannot be overwritten. With `force`, a failed replacement preserves the prior file. Recognition and initialization are separate operations: a script changed after recognition does not invalidate a deliberately selected snapshot.

Cancellation is checked at phase boundaries and immediately before writing. Static recognition and filesystem persistence are synchronous and cannot be interrupted mid-operation. Once an atomic write succeeds, cancellation, lost connectivity or failure to receive the reply does not roll it back. Check `server info` before retrying if the outcome is uncertain. Protocol version remains 3; the remote executable must advertise both command IDs for the recognition flow.
