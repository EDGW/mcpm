> Not yet migrated to the new documentation structure. This is retained legacy documentation; migrated commands are documented outside legacy/.

# `mcpm launch` / `mcpm attach`

Prepare an immutable Java command from the server's `launch.toml` and `conf.toml`, then let an independent `mcpmd shim` own Java. Launch attaches by default; leaving the client does not stop Java.

```sh
mcpm init ./server --start-script run.sh
mcpm launch --server ./server
mcpm-server launch --server ./server
mcpm-tools server launch --server ./server
```

- `--server <PATH>` selects the server root; default `.`. There is no positional path argument.
- TUI is the terminal default; `--no-ui` selects Console, and `--raw` or redirected stdout selects Raw. TUI requires drawable stdout and stderr. Local, explicit SSH and Remote workspaces share Host operations. Automatic restarts are not provided.
- `launch.toml` owns fixed JVM arguments, the class/JAR/module entry, fixed application arguments, working directory and explicit environment overrides.
- `conf.toml` owns the Java executable, memory/GC tuning, authlib-injector, server overrides and user arguments. Import enables source settings and leaves unspecified settings commented out. Every supported field has English documentation in the generated file.
- Complete manual registration creates a default `conf.toml` without inventing a launch entry.

## Attach and process control

```sh
mcpm launch --server ./server --detach
mcpm attach --server ./server --history-lines 100 --input-height 5
mcpm --ssh 'ssh user@host' launch --server /srv/minecraft
mcpm launch --server ./server --communication rcon
```

- `--detach` prints the run ID once the shim owns Java, not when Minecraft becomes ready. A second launch fails; attach never starts Java.
- `--communication auto` prefers RCON. `rcon` requires configuration before launch; both fall back to Java stdin on connection/authentication failure. `stdin` never attempts RCON. Read properties in Java's actual working directory, honoring application `--config` / `-c`; never enable or rewrite RCON automatically.
- RCON returns complete command output in the same operation. Stdin only confirms sending; its output is Java stdout. A connection loss after sending is unknown and never triggers replay.
- Decode only properties consumed by RCON; invalid unrelated values or escapes do not block extraction. A configured port does not enable disabled RCON.
- Logs are Java stdout, not RCON. Stdout and stderr each use one continuous Channel, without polling; source EOF closes that Channel independently of Java exit. Stderr and RCON replies are separately labeled. Raw preserves stdout bytes; other sources and diagnostics go to stderr. Bounded retention reports gaps.
- Source read blocks and log-batch payloads are capped at 1 MiB and pushed as data arrives, without waiting to fill the block. Blocks are raw bytes, not logical lines. Each stdout/stderr tail retains at most 1 MiB; gaps explicitly report lost bytes. After Java exits, retained tails are stored as raw binary, with only status/cursor metadata in JSON, not byte arrays. Command deduplication records remain separate.
- The default is a bottom Inline TUI, without an alternate screen. `--input-height` sets its initial height (default 4, minimum 3). Use Option+Up/Down on macOS, Alt+Up/Down elsewhere. Grow/shrink restores the visible output position without duplicating scrollback. Card titles and input contents have one space of horizontal padding. Left/Right/Home/End edit; Enter sends.
- Builtins occupy a complete line: `@detach` (or EOF/Ctrl+D) leaves without stopping Java; `@stop` sends ordinary stop and never escalates; `@kill` force-kills Java independently of the normal command queue. `@@detach`, `@@stop`, and `@@kill` send the corresponding literal command to the server. Already-submitted commands report their outcome before the client leaves.
- Ctrl+C sends the ordinary `stop` command. Another Ctrl+C while the shim reports Stopping kills Java's process group/Job. There is no automatic kill timeout.
- Multiple Listeners may attach and submit commands concurrently; the shim serializes normal commands without an exclusive control lease. A separate result Channel broadcasts user-command outcomes to all connected Listeners, including the sender, which displays each result only once. Own commands use a Primary frame; other Listeners' commands use a green frame labeled `Executed by another Listener · <id>`. Console uses the same frames; Raw omits frames/colors but preserves origin labels.
- Result subscriptions are live-only: re-attaching does not replay earlier commands. Slow Listeners receive an explicit missed-result count without blocking Java. Stdin acknowledgements mean only sent; game output still arrives through shared Java logs. `@kill` bypasses normal commands and its request is also broadcast.
- Status uses one persistent Channel: an initial snapshot, immediate updates on state/channel changes, and a current snapshot every second by default. Five seconds without a snapshot, or closure before a final exit state, is reported as a Channel failure, not as Java exit. A status failure does not stop Java.
- The bottom bar separates Java lifecycle, server communication status, and the actual command channel. Explicit `--communication rcon` and `auto` with usable RCON configuration share discovery, connection reuse, and status classification. Successful authentication shows `Server RCON Connected`, not Minecraft readiness. `auto` with missing, disabled, or unreadable RCON configuration, and explicit `stdin`, show `Server Unknown`.
- When using RCON (explicit `rcon` or configured `auto`), while Java is alive, stop has not been attempted, and authentication has never succeeded, the shim retries connection/authentication with a one-second pause after each attempt. Attempts never overlap and send no game commands. The first authenticated connection is retained and reused for commands. An observed disconnect changes status to Unknown without restarting initial discovery. A possibly sent stop or Java exit also means Unknown. Later user commands retain the normal channel-selection/fallback behavior; failed commands are never replayed.
- Each attach issues one real `help` after first observing RCON Connected. Its result stays private in that frontend session's memory, without broadcasting or printing the automatic response card; detach discards it and re-attach fetches it again. User-entered `help` is broadcast normally. Failed execution or an unrecognized catalog shows `Failed to fetch help`, without automatic retry. Builtin hints for `@detach`, `@stop`, and `@kill` remain available regardless of help.
- Inline hints appear only for nonempty, non-whitespace input and hide when the draft is cleared or sent. Gray top/side borders share Command Input's top border as their bottom; blank cells never expose logs. Only command names participate in prefix matching; arguments remain opaque syntax hints. At most five candidates are visible, long syntax wraps to multiple lines, Up/Down browses all matches, and the title counts candidates as selected index/total. Tab fills the selected name (first match if none selected), preserving typed arguments; Enter always sends the current draft without accepting a hint. Matched name text is bold Primary, its suffix regular Primary, game-command arguments use regular output color, and builtin descriptions are gray. Aliases such as `/wp -> waypoint` retain the arrow and append the target syntax when available; completion still inserts the alias. Console/Raw stay line-based without interactive hints.
- Hint ordering puts exact command-name matches first; other prefix matches retain their original help-list order.
- Java exit drains output and ends the shim. A bounded terminal receipt preserves final output and exit status without an empty resident service. A failed connection does not prove Java exited; discovery PIDs are never used for killing.
- Discovery and exit receipts in `.mcpm/run/` contain no absolute server path. Use the new path after moving the folder. A copied active folder does not carry the original OS lock and is refused as an active instance; renaming a live directory remains subject to OS restrictions.
- Relative command deduplication records are removed after the run is reaped. Endpoint collisions reject launch/control rather than selecting another instance.
- Inline TUI uses standard ratatui viewport/widgets and the shared theme. Terminal diagnostics selected by `--log-level` arrive through a bounded Channel and are inserted above the input; file logging remains independent. `--no-ui` selects Console.
- Install the matching `mcpmd` next to mcpm or on the executing Host's PATH.

### Attach history and prefix colors

- Standalone attach replays one terminal screen of history lines by default, or 24 lines when terminal size is unavailable. `--history-lines N` selects a count; zero follows live output directly.
- A History divider precedes replay. After reaching the connection snapshot, the same Channels continue with live output, without duplicate replay. Availability is limited by the shim's retained tail.
- Match only line-leading `[time/datetime] [thread/LEVEL]`; `thread/` is optional. Supported timestamps include time with fractional seconds, ISO datetime, and Forge's `ddMMMyyyy HH:mm:ss.SSS`. See the [Minecraft/Forge Log4j example](https://docs.spongepowered.org/stable/en/server/spongineer/logs.html).
- Timestamps are gray; only the level block is colored. INFO uses green, WARN/WARNING yellow, ERROR/FATAL red, DEBUG Secondary, TRACE muted. Bodies through WARN/WARNING are gray for that line only; ERROR/FATAL bodies and unmatched lines receive no added style. `NO_COLOR` disables colors.
- Own RCON replies use a Primary frame; replies to another Listener use a green frame with `Executed by another Listener · <id>`, also in Console mode. The padded title keeps a regular-weight label and bold command name (the first submitted word, without arguments). Game logs remain separate.
- Raw adds no prefix colors, history divider or RCON frame; Java stdout bytes remain unchanged. History line selection still applies.



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
- Dynamic progress is cleared before Java starts, then launch attaches or returns a run ID. Preparation completion or process creation does not imply Minecraft readiness; mcpm does not parse game logs to determine it.
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
- Unix shims detach from the original terminal and Java gets a separate process group. Windows uses a process group and Job, without forwarding client console Ctrl+C to Java.
- Stop travels through the command channel; force termination requires an explicit second Ctrl+C / kill operation. No automatic kill timeout is applied.
- Nonzero Java exits remain process results. Operation failures use the normal error-chain renderer. Unix signal exits map to `128 + signal`.
- Long commands use a temporary Java argument file, requiring Java 9+. The file is removed after execution.
- Legacy `server.json.start` snapshots are historical information only. Reimport explicitly; their original working directory cannot be reconstructed reliably.
