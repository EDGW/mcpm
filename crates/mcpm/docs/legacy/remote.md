> Not yet migrated to the new documentation structure. This is retained legacy documentation; migrated commands are documented outside legacy/.

# Remote execution and legacy sessions
PUT <3:
## mcpmd execution for server info/init

- Explicit `--ssh` selects the host and makes the supplied path remote. Only when `--ssh` is absent does workspace registration select Local or Remote execution. Connection failures never fall back to Local.
- `SshArgs::create_executor(Some(path))` resolves the path and opens one Executor for the flow. `None` skips workspace discovery, using explicit SSH or Local, for pathless commands.
- Local calls invoke the operation directly. Remote calls connect to a per-user `mcpmd link`, which owns one reusable SSH process running remote `mcpmd serve`. Concurrent clients have independent IPC streams and share the SSH session without interleaving JSON frames.
- The remote host needs `mcpmd` for migrated operations. Use `--ssh-mcpmd PATH` when it is not on the remote PATH. The local daemon is found beside mcpm, then through PATH.
- Remote stages and results are rendered locally. Candidate selection remains local. `mcpmd.logs` supplies structured remote diagnostics; `--remote-logs` controls their console display while the process log retains debug records.

### Authentication and startup

OpenSSH uses `RequestTTY=no`, `-T`, and `BatchMode=yes`. Configure keys, ssh-agent, IdentityFile or ProxyJump before invoking mcpm. Password and keyboard-interactive prompts are unsupported; unknown host keys fail until accepted through a manual SSH connection. Unrecognized connection tools retain their original argv. SSH stderr, exit status, and handshake errors retain their complete cause chain.

The link reports readiness over its inherited startup pipe; mcpm does not poll the endpoint. Unix uses private directories and UID-checked sockets; Windows uses a current-user-only named pipe that rejects remote clients. Protocol frames alone may appear on `mcpmd serve` stdout.

### Link lifetime

```sh
mcpmd link list
mcpmd link stop 'ssh user@host'
mcpmd link stop 'ssh user@host' --force
mcpmd link stop --all
```

Graceful stop rejects new work and drains active calls. Force stop aborts them. Client disconnection cancels only that client's calls. SSH closure terminates the link; the next invocation starts a new one. Unix SIGTERM/SIGINT and Windows CTRL_CLOSE/CTRL_BREAK request graceful stop. List queries live status and removes stale state records.

Configure new links through the existing `~/.mcpm/settings.toml` codec:

```toml
[link]
idle_timeout = 600
```

The timer runs only while there are no clients and no active calls. Zero disables idle exit. Existing links keep their startup value; stop them to apply a changed setting. `mcpmd link --idle-timeout SECS SSH` sets the timeout directly; mcpmd does not read mcpm settings.

## Legacy transport for unmigrated commands

The sections below describe mods/cache and server mods, not migrated server info/init. Both transports coexist during migration; unmigrated commands still need remote `mcpm-tools`.


One `mcpm-tools remote` process serves the entire caller-owned session over non-PTY
SSH stdin/stdout. Requests can run concurrently. Remote handles session routing,
cancellation, concurrency and global errors. Each command owns its contents codec;
remote never parses command-line arguments or business payloads.

A **command ID** is a stable string such as `mod.list`, declared and registered
by its command module. The client automatically generates a UUID v4 for each
invocation session (**request GUID**) to correlate all its replies. Concurrent
invocations of the same command ID have different GUIDs. `mod.list`, `mod.info`, `mod.deps`, `mod.source.identify`, `server.mods.cache`, `server.mods.list`, `server.mods.info`, `server.mods.deps` and `server.mods.source.search/files/identify` are currently registered (the last notation represents three IDs).

```json
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","command":"mod.list","contents":{"paths":["mods/"],"loaders":["fabric"]}}
```

Every reply has exactly **guid + status + contents**. Reply guid is the request
GUID. Nil UUID is reserved for connection control messages and cannot identify a request.

| Status | Contents owner | Meaning |
| --- | --- | --- |
| `ready` | Remote | Version, registered command IDs, concurrency limit. |
| `data` | Command | Opaque progress, results or business errors. |
| `finished` | Remote | Invocation ended normally; contents null. Business success is command-defined. |
| `error` | Remote | Global code/message error; terminal for this invocation, no finished follows. |
| `closed` | Remote | Session ended normally; contents null. |

Both directions use UTF-8 NDJSON, one JSON object plus newline per frame, flushed
immediately. Stdout is protocol-only; normal logs use a dedicated subscription. Stderr carries the unofficial release build startup warning and startup/connection diagnostics, with colors/UI disabled.
Wait for ready before submitting:

```json
{"guid":"00000000-0000-0000-0000-000000000000","status":"ready","contents":{"version":5,"commands":["mod.deps","mod.info","mod.list","mod.source.identify","server.info","server.init","server.init.recognize","server.mods.cache","server.mods.deps","server.mods.info","server.mods.list","server.mods.source.files","server.mods.source.identify","server.mods.source.search"],"max_concurrency":8}}
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","status":"data","contents":{"event":"count","count":0}}
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","status":"data","contents":{"event":"done","success":true}}
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","status":"finished","contents":null}
```

Submit without waiting for earlier calls. Replies may interleave across request
GUIDs; order within a request is preserved. One writer emits complete frames, and
one client reader routes opaque contents to request handles. Request GUIDs must
be non-nil and never reused within the session. `--max-concurrency` defaults to 8;
excess requests receive a terminal busy error. Retry with a new GUID.

After Ready, Session generates a fresh UUID v4 and subscribes to logs, forwarding
its local `--log-level` through a control message:

```json
{"logs":{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","level":"debug"}}
{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","status":"ready","contents":null}
{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","status":"data","contents":{"level":"debug","target":"mcpm::remote::server","fields":{"message":"Received remote command"},"spans":[]}}
```

This built-in subscription consumes no command slot and allows one unique non-nil
GUID per connection. Records retain severity, target, fields and spans, including
command invocation GUIDs. Remote logs are hidden on stderr unless the global
`--remote-logs` option is present. Remote operation errors always appear as
`remote error:`, independently of `--remote-logs` and `--log-level`. These options
do not affect file logs or failure exit statuses. The local console also filters logs by the selected level and
uses italic severity labels for remote events, without a prefix. Raw output,
redirection and NO_COLOR disable styling. Forwarding retains at least debug for
local file logs even with console logging off; trace is forwarded only when
selected. File logs on both machines continue to record through debug.

The last 256 startup records are buffered before subscription; expired records
produce a count warning. After stdin EOF, commands drain, logs drain, the log
GUID receives finished, then the connection receives closed. Console logs wait
until a full-screen TUI closes; file logging continues immediately. Protocol
version 5 requires updating both client and server.

Remote decodes global errors rather than delivering them as command data:

```json
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","status":"error","contents":{"code":"unsupported_command","message":"unsupported remote command ID: ..."}}
```

Global codes: unsupported_command, busy, cancelled, handler_failed, protocol.
The first four end only the target call. Handler failures/panics use handler_failed;
Recoverable business errors use command data; invalid arguments or unreadable server metadata may terminate a handler with handler_failed. Malformed frames and nil/reused
request GUIDs emit a nil-guid protocol error and terminate the session unsuccessfully.
Unexpected EOF, unrelated GUIDs, missing terminal replies or abnormal process exit
are transport failures.

Cancel with `{"cancel":"<request-guid>"}`. Cancellation is cooperative; list checks
between completions and stops scheduling new files; already-running synchronous
file operations finish before the command returns. Current I/O is not forcibly interrupted. Unknown/completed calls
are ignored; completion can win the race. Dropping a pending handle attempts cancel
while draining further replies so other calls remain usable. Close stdin to drain
accepted work, emit a nil-guid closed reply and exit successfully, even if calls
failed. Dropping Session closes/reaps the child process. Closing Session logs `stdin_bytes`
(sent), `stdout_bytes` (received) and `total_bytes` at info level, once even on
failure. These count actual protocol bytes, including handshakes, logs, control
frames and newlines, excluding SSH framing and stderr.

## Mod list contents

Input is an object with optional `paths` and `loaders` arrays, defaulting to empty,
and a positive `parallelism` integer (default 10). Parallelism bounds outer-JAR
reads and loader parsing on the remote host, independently of identify concurrency
and the number of SSH connections.
Empty paths means remote current directory; empty loaders tries all. Loader values
are forge, neoforge, fabric. Stdin is reserved for protocol. Display filters, columns,
expansion, colors, width and path styling remain local frontend settings.

The command itself serializes count, item and done events. Exactly count items
arrive immediately after each file loads, potentially out of order. Indices refer
to original input positions and must cover `0..count` exactly once. The client
checks index bounds, duplicates and omissions before presenting ordered results. Successes, failures
and unrecognized inputs each have exactly one record. Duplicate normalized paths share inspection results but retain separate records;
empty directories contribute no items; unresolved paths contribute one failure.
File failures do not stop later items and make business done.success false.
Cancellation/transport failure can leave incomplete results.

Records contain path, name, enabled, status, metadata and errors. Input failures
have null name/enabled. Status is success, unrecognized or failed. Metadata contains
loader, mods and recursive nested children; errors contain loader (null for input
failure) and message. All declarations/loaders/nested JARs for a file stay together.
Invalid arguments emit business error then done false, followed by remote finished.
Remote knows none of these business event names.

The CLI `mods info/list` sends a default-cache selection unless `--no-cache` is used; `--cache-dir PATH` sends an explicit remote directory. At the structured protocol boundary, omitted/null `cache` still means disabled, preserving existing callers and deps behavior.

Optional `cache` selects existing metadata reuse on the remote file host:

```json
{"kind":"default"}
{"kind":"directory","path":"/srv/mod-cache"}
```

Omitted or null disables caching. The default root initializes only when absent
or empty; explicit roots must already have a supported cache manifest. Broken
roots fail the request. Valid recognition and non-recognition are reused after
imohash/identity candidate matching or full BLAKE3 lookup. Missing, stale or invalid
results are identified normally. Inspection does not import files, establish hard
links or write metadata.
This option is also accepted by `mod.info` and the shared `mod.deps` request schema.
Command-local duplicate-input caching is independent of this persistent cache.

## Mod info and deps contents

`mod.info` and `mod.deps` share inspection acquisition and the same structured schema:

```json
{"directory":"/srv/mods","loaders":["fabric"],"selection":{"queries":[{"kind":"file","value":"example.jar"},{"kind":"modid","value":"nested_mod"}]}}
```

Optional positive `parallelism` defaults to 10. It bounds file reads for `--all`
and lazy name/ID directory indexing. Independent file queries remain sequential.

All fields are optional. Directory defaults to `.`, loaders to all builtins,
and selection to no inputs (successful empty result). Query kinds are `auto`,
`file`, `modid`, `name`; their order and duplicates are preserved. Values must
be UTF-8. `auto` tries filename, exact ID, then exact name. `selection.all: true`
scans the directory and cannot be combined with queries. Relative files are
resolved against directory on the remote host; directories are not file inputs.
A name/ID match selects its owning archive and descendants, retaining sibling
declarations in that archive. Stdin never enters the remote command parser.

Each file read not yet inspected in this command, or directory scan, emits `scan` progress. A scan starts
with completed=0 and its total, followed by one increment per completion; failed,
unrecognized and cached directory entries all count. Empty scans emit 0/0 once.
Command-local hits for individual files and already-built selection indexes emit no scan events. Persistent cache reuse still counts as a file inspection completion.
The next scan can start only after the previous one reaches its total. Progress
arrives during parsing, before selected file results; clients validate the sequence
and display progress based on local stderr TTY detection.

Acquisition emits `scan`, `file` and `error` events, then exactly one `done`.
Diagnostics may arrive during a scan; selected files follow scan completion:

```json
{"event":"scan","completed":0,"total":1}
{"event":"scan","completed":1,"total":1}
{"event":"file","index":0,"file":{"path":"/srv/mods/example.jar","filename":"example.jar","enabled":true,"metadata":[{"loader":"fabric","loader_name":"Fabric","records":[{"info":{"modid":"example","name":"Example","version":"1.0","dependencies":[]},"source":[]}]}],"failed_loaders":[]}}
{"event":"error","error":{"message":"inspection failed","causes":["underlying cause"]}}
{"event":"done","files":1,"failed":1,"ignored":0}
```

File indices start at zero and increase by one. Done.files must equal the number
of file events. Records contain typed mod declarations and archive-member source
chains; they already reflect query subtree selection. Paths and filenames belong
to the remote platform and are never read by the client. Styling and path display
options remain local. Partial loader failures retain successful metadata and
failed loader IDs; error chains are separate events. Info omits files without
metadata; deps can present their incomplete identification state.

Done.failed counts unique scanned files with failures plus failed selections;
repeated failed file queries share scan failures. Name/ID indexing can discover
failures in other directory entries. Done.ignored counts unrecognized files skipped
by loader filtering. Without filtering these are errors. Input and identification
errors do not stop later queries. Argument errors emit error then done with
files=0, failed=1, ignored=0. Business failures are followed by finished.
Execution failures (including worker or consumer failures) terminate acquisition
and propagate to the caller; remote handlers use a global error without business
done or finished. Cancellation uses the global cancelled status and can leave
partial results. Cancellation is checked between queries,
selected files and directory scan completions. The client validates indices,
completion counts, diagnostics/failure agreement and lifecycle before final success.
Protocol version 5 requires updating both endpoints.

## mod.source.identify contents

Source identify registers `mod.source.identify`, which **only reads file summaries and optional cached matches**. Its CLI and mod-list TUI share one local provider scheduler. Generic Remote/SSH code never decodes these contents.

```json
{"paths":["/srv/mods/a.jar","/srv/mods/b.jar"],"requirements":{"murmur2":true},"parallelism":10}
```

Paths must be nonempty. SHA-1 and size are always computed; requirements.murmur2 optionally requests the CurseForge fingerprint (default false). Positive parallelism defaults to 10 and bounds remote file reads. Unknown fields are rejected. Provider configuration, API keys and proxy settings never cross SSH.

Events use zero-based indices into the original paths. Duplicate physical paths share one read and appear together:

```json
{"event":"loading","indices":[0]}
{"event":"item","indices":[0],"outcome":{"status":"summary","summary":{"size":3,"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d","murmur2":1621425345}}}
{"event":"loading","indices":[1]}
{"event":"item","indices":[1],"outcome":{"status":"failed","error":{"message":"failed to summarize file ...","causes":["read failed","file not found"]}}}
{"event":"done","failed":1}
```

MurmurHash2 uses seed 1 and excludes TAB, LF, CR and SPACE; its field is null when not requested. Each path position has exactly one loading and one item, including failures. Items may arrive out of order. Done counts failures by original path position, followed by Remote's finished status. The client validates ranges, duplicates, completeness, failure totals and digest syntax. Invalid arguments, cancellation and handler failures use the common terminal error status; partial results may precede it.

The local client queries Modrinth/CurseForge from summaries and verifies size and SHA-1, preserving primary/additional match semantics. Network egress, proxies and official API keys remain local; the remote host never queries providers. CLI output retains input order. Remote file failures always use `remote error:`; local network failures use `error:`. Neither depends on `--remote-logs` or `--log-level`.

SSH mod-list TUI requests summaries over its existing Session and shares the same local lookup pipeline. Identify parallelism bounds local file/provider requests together and remote file-summary reads separately. Exiting cancels local network tasks and unfinished remote acquisition without waiting for HTTP timeouts. Console and Raw lists do not automatically identify sources.

Logs distinguish execution=local/remote and phase=summary/provider, including reads, requests, validation, failures and durations. Remote logs use the existing subscription and italic terminal severity. This command schema replaces the previous remote provider lookup schema; both client and server must be updated.

## Server command contents

Protocol version 5 requires both endpoints to be upgraded together. Clients validate the handshake version and required command IDs. Server cache targets come from the remote Server runtime; requests cannot override the directory.

- [`server.mods.info/deps`](server/mods.md): server-owned scope with the shared inspection event stream.
- [`server.mods.list`](server/mods.md#list): server-owned selections, streamed progress and records, local TUI/Console/Raw.
- [`server.mods.cache`](server/mods.md#cache): server-scoped explicit archive imports and metadata refresh, executed on the file host.
- [`server.mods.source.search/files/identify`](server/source.md#ssh): remote compatibility or file summaries, local provider requests and credentials.

Malformed requests and server metadata loading failures use terminal `handler_failed`. Selection and scan errors may be business events while valid results continue. Cancellation or execution/transport failure does not emit a business completion event. Errors are always displayed independently of remote-log visibility and produce a nonzero exit status.

### Migrated server operations

`server.info`, `server.init.recognize`, `server.init`, and `server.init.prepare_remote` use the backend operation registry through `mcpmd`; they are not registered in the legacy protocol. Workspace inspection and stub registration are Shallow operations, executed locally even when the Executor targets a remote host. Git and all actual server preparation belong to Host operations.

Recognition streams stage events followed by candidates containing their configuration drafts. Initialization streams stages followed by its committed result. Local execution uses the same types without serde; remote execution uses the existing operation framing. See [initialization behavior](server/init.md#remote-behavior) and the [execution layer](../../../mcpm-exec/README.md).

## Registration and SSH

Modules submit `inventory::submit! { Initcall(init) }`. Startup invokes all hooks;
each module's hook registers its own Command implementation and string ID.
Duplicate command IDs fail startup. No central list of concrete commands is
needed and remote does not depend on business implementations. Handlers define their business types and send them directly through `Output::send`.
`Reply::new` encapsulates serialization; receivers use `Reply::decode::<BusinessType>()`
to decode payloads. Remote selects only handshake/global-error types and never
interprets business fields. Handlers use checkpoint for cancellation.

Unmigrated commands retain their SSH adapter in `src/ssh/`. `--ssh` and `--ssh-mcpmd` remain top-level global options,
accepted before or after subcommands. This legacy transport serves mods list/info/deps/source identify and supported server mods commands, not migrated server info/init. TUI and rendering remain local. Server mods check and dep-graph reject SSH.
For these unmigrated commands, the remote executable defaults to mcpm-tools on PATH; an explicit override must identify the legacy tools executable. Migrated server info/init instead use mcpmd as described above.

The SSH server's default shell directly starts `mcpm-tools remote`. Windows connections
require cmd as the default shell; executable paths are double-quoted and must not
contain double quotes, line breaks, NUL, `%` or `!`. POSIX executables use shell
quoting. Business arguments are sent as JSON, independently of shell syntax.

```sh
mcpm-tools --ssh 'ssh -i key user@host' mods list mods/ --no-ui
mcpm-tools --ssh 'ssh user@windows' --ssh-mcpmd 'C:\mcpm-tools.exe' --raw mods list 'C:\mods'
```

[Command reference](README.md) · [简体中文](../../docs-zh/legacy/remote.md)

Remote error payloads preserve `message` and ordered `causes` separately. The local
frontend always renders `remote error:` followed by indented `caused by:` entries,
independently of `--remote-logs` and `--log-level`.

## Identification cache statistics

Inspection and list `done` events include optional `cache` totals:

```json
{"fully_cached":8,"partially_cached":3,"total":15}
```

- Counts describe unique normalized file inspections on the owning host, including failures and non-recognition, before presentation filtering. Duplicate inputs and nested declarations do not increase totals.
- Full means all requested loader results were reused; partial means at least one was reused and at least one was identified normally. Cached non-recognition counts as reuse. Disabled caching yields zero full/partial counts.
- These fields are independent of emitted file/item counts, which can contain repeated queries or selected declarations. Clients reject cached counts exceeding total; absent statistics from older endpoints are omitted from presentation.
- Local Raw text remains unchanged. Console and list TUI consume the structured totals to render a summary.

Legacy `added` and `updated` fields from older endpoints are ignored. Inspection no longer writes cache data.

## Source-identification cache transport

Server commands use the Server runtime cache. Server list scopes and source-selection completions carry `environment:{cache}`; explicit server caching begins with a `{cache}` data payload. The host resolves the effective target once, and clients reuse it for subsequent reads and publication. See [Server mod protocol](server/mods.md#remote-protocol).

- `mod.source.identify` accepts optional `cache`: null/omitted disables lookup; `{"kind":"default"}` or `{"kind":"directory","path":"..."}` selects the file-host cache. The CLI enables it by default.
- Successful summary outcomes may include `cached`, a source-keyed map of FileMatch values. Empty maps are omitted for compatibility. Cached matches require cached content file and validate against the summary. Clients validate key ownership, digest syntax, size and SHA-1 before skipping provider requests.
- `server.mods.cache.writing.metas` selects `meta`/`ident` (both by default). The remote handler imports cached content file and optionally refreshes Loader metadata. The client enriches source results with its own providers and credentials after storage completes; source specifications never cross SSH.
- Explicit `cache.mods.identification.write` accepts `{cache,id,file}`, where `id` is BLAKE3, `file` is a trusted canonical-source FileMatch, and cache uses the selection above. The host checks cached content file BLAKE3, size and SHA-1, merges through the library codec, and acknowledges with one null data value. Invalid/missing data produces the common command error. Identify never calls this write operation.
- FileMatch contains `reference {project:{source,id},id}`, `filename`, `size`, `hashes` (`{algorithm,value}`), and `role` (`primary`/`additional`). All CurseForge configurations persist `source:"curseforge"`.
- Cache records add `metadata_requested`, source-keyed `identification` outcomes (`matched`, `unmatched`, or `failed` with `error` and `remote`), and `error_remote` for the primary error. File failures are counted once even if several sources fail. Console reports each source error with its actual origin.

### Branched diagnostics (protocol 5)

- Errors preserve `message` and ordered `causes`. An optional `branches` array contains further error reports for independent failures, recursively.
- An aggregate root may have an empty message and contain only branches. The first branch uses the current error/cause label; subsequent branches use `and:`. Causes within each branch indent one additional level.
- Both endpoints must support protocol 5 so that nested failures are never silently discarded.
