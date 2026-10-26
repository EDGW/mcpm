# `mcpm launch`

Launch a registered local server from its root `launch.toml` and `conf.toml`, inherit Java's standard streams, wait in the foreground, and return its exit status.

```sh
mcpm init ./server --start-script run.sh
mcpm launch --server ./server
mcpm-server launch --server ./server
mcpm-tools server launch --server ./server
```

- `--server <PATH>` selects the server root; default `.`. There is no positional path argument.
- Console and Raw both preserve Minecraft's console streams. SSH, Remote workspaces, background operation and automatic restarts are unsupported.
- `launch.toml` owns fixed JVM arguments, the class/JAR/module entry, fixed application arguments, working directory and explicit environment overrides.
- `conf.toml` owns the Java executable, memory/GC tuning, server overrides and user arguments. Import enables source settings and leaves unspecified settings commented out. Every supported field has English documentation in the generated file.
- Complete manual registration creates a default `conf.toml` without inventing a launch entry.

```toml
# launch.toml
[entry]
type = "jar"
path = "server.jar"
```

```toml
# conf.toml
[java]
executable = "java"

[jvm.heap]
initial = "1G"
maximum = "4G"

[server]
nogui = true
```

## Per-launch overrides

Overrides affect only this launch and never rewrite `conf.toml`.

```sh
mcpm launch --java /opt/jdk/bin/java -m 1G:4G -G g1 --port 25566 --world another-world
mcpm launch -m 4G -Ojvm.threadStack=2M -E '-Dfile.encoding=UTF-8'
mcpm launch '-Ojvm.extraArguments:=["-Dfirst=1"]' '-Ojvm.extraArguments=-Dsecond=2'
```

| Option | Configuration and behavior |
| --- | --- |
| `-O<key>=<value>` | Replace a scalar; append an item or array to a list |
| `-O<key>+<value>` or `-O<key>+=<value>` | Append an item or array to a list |
| `-O<key>:=<value>` | Clear a list, then set its new item or array; omit the value to clear |
| `--java <EXECUTABLE>` | `java.executable` |
| `-m/--memory <INITIAL>:<MAXIMUM>` | `jvm.heap.initial` and `jvm.heap.maximum` |
| `-m/--memory <MAXIMUM>` | Only `jvm.heap.maximum`; preserve the current initial heap |
| `-G/--gc <COLLECTOR>` | `jvm.gc.collector`: serial, parallel, g1, zgc, shenandoah, epsilon |
| `--port <PORT>` | `server.port` |
| `--world <DIRECTORY>` | `server.world` |
| `-E/--extra <ARGUMENT>` | Append one `jvm.extraArguments` item after all `-O` operations |

- Keys use the full, case-sensitive `conf.toml` field path. Launch structure cannot be overridden.
- Values accept TOML literals and unquoted strings. Quote the whole argument when shell spaces or array quotes need preserving.
- List operations may repeat and run in `-O` order. Use `-Ojvm.extraArguments:=` to clear existing general JVM additions.
- Each convenience option may appear once. Repeating a scalar key, including through both `-O` and a convenience option, fails.
- Configuration structure and argument-region boundaries are checked after all changes. JVM values, duplicates and compatibility are interpreted by the selected Java. `-E` accepts general JVM options; use `-Ojvm.memory.extraArguments=...` for memory/GC extensions.

## Behavior

- Both files are read from the selected server root; missing files are errors. Launch never executes a source script or rereads its original argument files.
- `process.workingDirectory` is relative to the server root. JAR/class/module paths and application arguments use the actual Java working directory. Relative `java.executable` paths use the server root; bare names use the effective `PATH`.
- Each array string is one argv item. No shell expansion occurs. Raw JVM arrays cannot introduce another entry or a live `@file` reference.
- Source entries, exit commands, unknown option boundaries, foreign platforms and explicit `auto` settings are rejected. Omitted tuning leaves JVM defaults unchanged.
- Raw argument arrays retain their order and duplicates for Java or the server to interpret. CLI scalar overrides must still be unique.
- Environment variables are passed to Java. Ordinary launches do not run a tuning validation probe; long commands probe only argument-file transport support.
- Argument order is fixed JVM arguments, structured JVM settings, memory extension arguments, general JVM additions, entry, fixed application arguments, server overrides, then user application additions. Order-sensitive memory groups remain together.
- Server settings are extracted only when application operand boundaries are known; unknown grammars retain the entire fixed argv region. Imports reject unlock/fixed diagnostic option combinations whose order cannot be preserved safely.
- `server.port` and `server.world` generate launch arguments without reading or writing `server.properties`. The target server decides whether it accepts these arguments and values; `world` selects a directory.
- There are no dependency downloads, EULA acceptance or persistent configuration rewrites during launch.

## Reimport and process lifetime

- Reimport changed scripts with `mcpm init ./server --start-script run.sh --force`.
- `--force` replaces registration and launch structure while preserving existing `conf.toml` bytes. Add `--foverride-conf` to regenerate it, including existing settings and comments.
- Interrupted publication is recovered on the next explicit initialization. Launch refuses a pending recovery journal. A workspace-marker failure reports files already committed.
- Unix transfers the controlling terminal to Java's process group and restores it afterward. Windows shares the console so Ctrl+C reaches Java without duplicate forwarding.
- Explicit stop requests allow ten seconds for shutdown; a second request forces termination. Unix terminal Ctrl+C goes directly to Java.
- Nonzero Java exits remain process results. Operation failures use the normal error-chain renderer. Unix signal exits map to `128 + signal`.
- Long commands use a temporary Java argument file, requiring Java 9+. The file is removed after execution.
- Legacy `server.json.start` snapshots are historical information only. Reimport explicitly; their original working directory cannot be reconstructed reliably.
