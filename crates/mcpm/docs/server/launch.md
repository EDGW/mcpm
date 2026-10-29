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
- `conf.toml` owns the Java executable, memory/GC tuning, authlib-injector, server overrides and user arguments. Import enables source settings and leaves unspecified settings commented out. Every supported field has English documentation in the generated file.
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
| `-A/--auth <API_URL>` | `authlib-injector.apiUrl`; enable minimal managed authentication for this launch if absent |
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
- Source entries, exit commands, unknown option boundaries, foreign platforms and explicit `auto` memory/GC settings are rejected. Omitted tuning leaves JVM defaults unchanged.
- Raw argument arrays retain their order and duplicates for Java or the server to interpret. CLI scalar overrides must still be unique.
- Environment variables are passed to Java. Ordinary launches do not run a tuning validation probe; long commands probe only argument-file transport support.
- Argument order is fixed JVM arguments, structured JVM settings, memory extension arguments, general JVM additions, authentication agent and properties, entry, fixed application arguments, server overrides, then user application additions. Order-sensitive memory groups remain together.
- Server settings are extracted only when application operand boundaries are known; unknown grammars retain the entire fixed argv region. Imports reject unlock/fixed diagnostic option combinations whose order cannot be preserved safely.
- `server.port` and `server.world` generate launch arguments without reading or writing `server.properties`. The target server decides whether it accepts these arguments and values; `world` selects a directory.
- Launch does not install server dependencies, accept the EULA or rewrite configuration. Managed authentication may download authlib-injector.

## Launch progress

- Preparation stages go to stderr: configuration, Java/argument checks, authentication and Java startup. Argument-file probing, release lookup, download, verification and installation appear only when required.
- With capable terminal stderr and without `--raw`, stages update in one dynamic progress area without printing status lines. Downloads show the source, transferred bytes and speed; known totals add a percentage and ETA, while unknown totals remain indeterminate.
- A failed source is identified before fallback. Each attempt resets byte counters and speed estimates; failed transfers are never accumulated. Finishing the transfer still requires verification and installation.
- `--raw`, redirected stderr or terminals without dynamic drawing disable progress, preserving game output and error diagnostics. Progress is independent of `--log-level` and never goes to stdout.
- Dynamic progress is cleared before Java starts, after which game streams are inherited. Preparation completion or process creation does not imply Minecraft readiness; mcpm does not parse game logs to determine it.
- Failure or cancellation clears the display. Errors continue through the shared cause-chain renderer.

## authlib-injector

Enable authentication with this minimal addition to `conf.toml`:

```toml
[authlib-injector]
apiUrl = "https://example.com/api/yggdrasil/"
```

- An omitted or empty `jar` uses `.mcpm/authlib-injector/authlib-injector.jar`. Its `meta.json` records the build number, version, upstream SHA-256 and optional release time.
- A nonempty `jar`, such as `libraries/custom-agent.jar`, selects a manual file relative to the server root, independently of Java's working directory. Only file existence and readability are checked: no digest validation, metadata access or automatic replacement.
- `--auth` / `-A` overrides only the API URL for this launch and preserves other authentication settings. Combining it with `-Oauthlib-injector.apiUrl=...` is a duplicate scalar override error. Managed paths are never written into TOML.
- The generated template documents all supported fields: logging, Mojang namespace and proxy, legacy skins, debugging, ignored packages, HTTP service, server name, anti-features, profile keys and username checks. Feature choices accept `default`, `enabled` or `disabled`; false or omitted booleans emit no property.
- Configuring the same agent or a configured authentication property in raw JVM arguments and structured fields fails. Raw arguments cannot override structured settings.

`--auth-jar-policy <auto|local|refresh>` controls the managed JAR for this launch:

| Policy | Behavior |
| --- | --- |
| `auto` (default) | Verify local metadata and JAR digest; reuse a valid pair without network access. Download the latest release when files are missing, metadata is corrupt or the digest differs |
| `local` | Verify the local pair and fail if repair is needed; never download |
| `refresh` | Download the latest release again, even if its version is unchanged |

- An explicit policy requires enabled authentication with a managed JAR. It fails for manual JARs or disabled authentication.
- Unknown metadata format versions, permission/I/O errors and directory targets fail without automatic replacement. Download or verification failures prevent launch without falling back to old files.
- Downloads use the existing default BMCLAPI/upstream source order. Valid local files trigger no update check. `local` restricts only mcpm's agent download, not Java or authentication traffic.
- An independent preparation lock covers validation, download and publication; contention fails immediately. The lock is released before Minecraft starts. Stopping preparation cancels downloads and cleans temporary files.
- Managed files are server state and are outside cache command management.

```sh
mcpm launch -A https://example.com/api/yggdrasil/
mcpm launch --auth-jar-policy local
mcpm launch --auth-jar-policy refresh
mcpm launch -Oauthlib-injector.jar=libraries/custom-agent.jar
```

## Reimport and process lifetime

- Reimport changed scripts with `mcpm init ./server --start-script run.sh --force`.
- `--force` replaces registration and launch structure while preserving existing `conf.toml` bytes. Add `--foverride-conf` to regenerate it, including existing settings and comments.
- Interrupted publication is recovered on the next explicit initialization. Launch refuses a pending recovery journal. A workspace-marker failure reports files already committed.
- Unix transfers the controlling terminal to Java's process group and restores it afterward. Windows shares the console so Ctrl+C reaches Java without duplicate forwarding.
- Explicit stop requests allow ten seconds for shutdown; a second request forces termination. Unix terminal Ctrl+C goes directly to Java.
- Nonzero Java exits remain process results. Operation failures use the normal error-chain renderer. Unix signal exits map to `128 + signal`.
- Long commands use a temporary Java argument file, requiring Java 9+. The file is removed after execution.
- Legacy `server.json.start` snapshots are historical information only. Reimport explicitly; their original working directory cannot be reconstructed reliably.
