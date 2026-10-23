# `mcpm server init`

```sh
mcpm server init [PATH] [OPTIONS]
mcpm server init /path/to/server
mcpm server init --start-script ./start.sh
mcpm server init --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
mcpm server init --minecraft-version 1.21.1 --loader vanilla
```

Defaults to `run.bat` on Windows and `run.sh` on Unix in the server directory.
`--start-script FILE` accepts a path relative to that directory or an absolute
path, and conflicts with manual `--minecraft-version` (alias `--mc-version`),
`--loader`, and `--loader-version`. Loader choices are `forge`, `neoforge`,
`fabric`, and `vanilla`. Complete manual metadata registers directly. Incomplete
metadata is filled by inspecting the default script and filtering verified
candidates against every supplied field; no match fails. Vanilla cannot have a
loader version.

An existing `.mcpm/server.json`, even invalid JSON, blocks initialization.
`--force` allows atomic replacement after successful verification and selection;
failure or cancellation preserves the previous file. `--force`, `--full-command`,
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
in raw/non-TTY mode; use a more specific script or manual version filters. Any
unrecognized Java launch fails the entire scan without partial results. Missing
loader evidence never proves vanilla. Failure explicitly reports an unrecognized
server and asks for manual initialization with complete version and loader options;
use `--loader vanilla` explicitly for a known vanilla server.

The selected script, JVM directory, executable and flattened argv are saved as a
snapshot. Successful output matches [`server info`](info.md). Non-raw command
display fits one terminal line with `(+N)` omitted-character count. Use
`--full-command` to display it all. Commands are gray; path values use plain color.
