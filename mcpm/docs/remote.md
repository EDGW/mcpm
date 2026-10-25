# Remote sessions and dispatch

One `mcpm-tools remote` process serves the entire caller-owned session over non-PTY
SSH stdin/stdout. Requests can run concurrently. Remote handles session routing,
cancellation, concurrency and global errors. Each command owns its contents codec;
remote never parses command-line arguments or business payloads.

A **command ID** is a stable string such as `mod.list`, declared and registered
by its command module. The client automatically generates a UUID v4 for each
invocation session (**request GUID**) to correlate all its replies. Concurrent
invocations of the same command ID have different GUIDs. `mod.list`, `mod.info`, `mod.deps`, `mod.source.identify`, `server.info`, `server.init`, `server.init.recognize`, `server.mods.cache`, `server.mods.list`, `server.mods.info`, `server.mods.deps` and `server.mods.source.search/files/identify` are currently registered (the last notation represents three IDs).

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
immediately. Stdout is protocol-only; normal logs use a dedicated subscription. Stderr is reserved for startup/connection diagnostics, with colors/UI disabled.
Wait for ready before submitting:

```json
{"guid":"00000000-0000-0000-0000-000000000000","status":"ready","contents":{"version":4,"commands":["mod.deps","mod.info","mod.list","mod.source.identify","server.info","server.init","server.init.recognize","server.mods.cache","server.mods.deps","server.mods.info","server.mods.list","server.mods.source.files","server.mods.source.identify","server.mods.source.search"],"max_concurrency":8}}
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
version 4 requires updating both client and server.

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

The CLI `mod info/list` sends a default-cache selection unless `--no-cache` is used; `--cache-dir PATH` sends an explicit remote directory. At the structured protocol boundary, omitted/null `cache` still means disabled, preserving existing callers and deps behavior.

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
Protocol version 4 requires updating both endpoints.

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

Protocol version 4 requires both endpoints to be upgraded together. Clients validate the handshake version and required command IDs. Server cache targets come from the remote Server runtime; requests cannot override the directory.

- [`server.init` and `server.init.recognize`](server/init.md#remote-behavior): independent complete-data initialization and read-only recognition; candidate selection stays local.
- [`server.info`](server/info.md#ssh): one source-host metadata snapshot, rendered locally.
- [`server.mods.info/deps`](server/mods.md): server-owned scope with the shared inspection event stream.
- [`server.mods.list`](server/mods.md#list): server-owned selections, streamed progress and records, local TUI/Console/Raw.
- [`server.mods.cache`](server/mods.md#cache): server-scoped explicit archive imports and metadata refresh, executed on the file host.
- [`server.mods.source.search/files/identify`](server/source.md#ssh): remote compatibility or file summaries, local provider requests and credentials.

Malformed requests and server metadata loading failures use the terminal global `handler_failed` status. Selection and scan errors can be reported in business events while valid results continue. No business completion event follows cancellation or an execution/transport failure. Initialization checks cancellation before its atomic write; interruption after that boundary can leave the configuration written even without a successful reply. Remote errors are always displayed, independently of `--remote-logs` and `--log-level`; failures produce a nonzero exit status.

### Server initialization protocol

`server.init.recognize` accepts `path` (default `.`), optional `start_script`, and optional `minecraft_version`, `loader`, and `loader_version` candidate filters. After requiring a Local workspace type, it only reads scripts and identity evidence, independently of existing server.json metadata. The file-owning host selects the default script. It emits:

```json
{"event":"stage","phase":"recognizing"}
{"event":"candidates","candidates":[{"metadata":{"version":1,"minecraft_version":"1.21.1","loader":null,"start":{"script":"/srv/server/run.sh","directory":"/srv/server","executable":"java","arguments":["-jar","server.jar"],"windows":false}},"line":1,"main_class":"-jar"}]}
```

`server.init` accepts complete `metadata`, `path` (default `.`), and `force` (default false). No recognition token or cached candidate is required:

```json
{"path":"/srv/server","metadata":{"version":1,"minecraft_version":"1.21.1","loader":null},"force":false}
```

Metadata requires nonempty Minecraft/loader versions and explicit `loader` (`null` for vanilla). Optional `start` is the complete startup snapshot shown above; script/directory/executable must be nonempty, and executable/arguments cannot contain NUL. Initialization never rereads the script or requires its referenced files to exist. Wire v4 continues to emit metadata version 1 for compatibility; Local persistence writes disk version 2. Initialization emits `stage` with `validating`, then `writing`, then `initialized {server}` using the `server.info` schema. Successful operations finish with the common `finished` status; failures end with `error`. Clients validate phase order and exactly one result. Recognition and initialization share one SSH session when candidate selection is needed.

All server endpoints, including recognition, require a Local workspace type. Remote references are resolved only by the frontend, never by `mcpm-tools remote`. Initialization can create a new Local registration but cannot replace a Remote workspace, even with `force`. Recognition remains an independent script-only operation and does not follow workspace references.

Metadata files are individually atomic, not a two-file transaction. mcpm registration writers hold a workspace lock and recheck its type before writing. A new Remote registration publishes `server.json` before `mcpm.json`; if publishing the type fails, it removes its new server file. An abrupt process termination may still leave an incomplete registration, which fails to load. Local initialization can persist `server.json` before publishing the Local marker; a marker failure reports that partial success, and a missing marker still defaults to Local. A completed atomic write is not rolled back after cancellation or connection loss; inspect `server info` before retrying an uncertain result.

## Registration and SSH

Modules submit `inventory::submit! { Initcall(init) }`. Startup invokes all hooks;
each module's hook registers its own Command implementation and string ID.
Duplicate command IDs fail startup. No central list of concrete commands is
needed and remote does not depend on business implementations. Handlers define their business types and send them directly through `Output::send`.
`Reply::new` encapsulates serialization; receivers use `Reply::decode::<BusinessType>()`
to decode payloads. Remote selects only handshake/global-error types and never
interprets business fields. Handlers use checkpoint for cancellation.

SSH lives in shared `src/ssh/`. `--ssh` and `--ssh-mcpm` are top-level global options,
accepted before/after subcommands. Supported commands are mod list/info/deps/source identify, server init/info, and server mods list/info/cache/deps/source search/files/identify. List and source browsing keep their TUI local; inspection supports Console/Raw. Server mods check and dep-graph reject SSH. ssh-mcpm requires ssh and defaults
to mcpm-tools on the remote PATH. There is no legacy mod list --remote entry point.

The SSH server's default shell directly starts `mcpm-tools remote`. Windows connections
require cmd as the default shell; executable paths are double-quoted and must not
contain double quotes, line breaks, NUL, `%` or `!`. POSIX executables use shell
quoting. Business arguments are sent as JSON, independently of shell syntax.

```sh
mcpm-tools --ssh 'ssh -i key user@host' mod list mods/ --no-ui
mcpm-tools --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm-tools.exe' --raw mod list 'C:\mods'
```

[Command reference](README.md) · [简体中文](../docs-zh/remote.md)

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
