# Remote sessions and dispatch

One `mcpm remote` process serves the entire caller-owned session over non-PTY
SSH stdin/stdout. Requests can run concurrently. Remote handles session routing,
cancellation, concurrency and global errors. Each command owns its contents codec;
remote never parses command-line arguments or business payloads.

A **command ID** is a stable string such as `mod.list`, declared and registered
by its command module. The client automatically generates a UUID v4 for each
invocation session (**request GUID**) to correlate all its replies. Concurrent
invocations of the same command ID have different GUIDs. `mod.list` and `mod.source.identify` are currently registered.

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
{"guid":"00000000-0000-0000-0000-000000000000","status":"ready","contents":{"version":2,"commands":["mod.list","mod.source.identify"],"max_concurrency":8}}
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
command invocation GUIDs. The local console filters by the selected level and
uses italic severity labels for remote events, without a prefix. Raw output,
redirection and NO_COLOR disable styling. Forwarding retains at least debug for
local file logs even with console logging off; trace is forwarded only when
selected. File logs on both machines continue to record through debug.

The last 256 startup records are buffered before subscription; expired records
produce a count warning. After stdin EOF, commands drain, logs drain, the log
GUID receives finished, then the connection receives closed. Console logs wait
until a full-screen TUI closes; file logging continues immediately. Protocol
version 2 requires updating both client and server.

Remote decodes global errors rather than delivering them as command data:

```json
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","status":"error","contents":{"code":"unsupported_command","message":"unsupported remote command ID: ..."}}
```

Global codes: unsupported_command, busy, cancelled, handler_failed, protocol.
The first four end only the target call. Handler failures/panics use handler_failed;
argument and file errors belong to command data. Malformed frames and nil/reused
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

## mod.source.identify contents

Source identify registers `mod.source.identify`, which **only computes file summaries**. Its CLI and mod-list TUI share one local provider scheduler. Generic Remote/SSH code never decodes these contents.

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

The local client queries Modrinth/CurseForge from summaries and verifies size and SHA-1, preserving primary/additional match semantics. Network egress, proxies and official API keys remain local; the remote host never queries providers. CLI output retains input order. Remote file failures use `remote error:`; local network failures use `error:`.

SSH mod-list TUI requests summaries over its existing Session and shares the same local lookup pipeline. Identify parallelism bounds local file/provider requests together and remote file-summary reads separately. Exiting cancels local network tasks and unfinished remote acquisition without waiting for HTTP timeouts. Console and Raw lists do not automatically identify sources.

Logs distinguish execution=local/remote and phase=summary/provider, including reads, requests, validation, failures and durations. Remote logs use the existing subscription and italic terminal severity. This command schema replaces the previous remote provider lookup schema; both client and server must be updated.

## Registration and SSH

Modules submit `inventory::submit! { Initcall(init) }`. Startup invokes all hooks;
each module's hook registers its own Command implementation and string ID.
Duplicate command IDs fail startup. No central list of concrete commands is
needed and remote does not depend on business implementations. Handlers define their business types and send them directly through `Output::send`.
`Reply::new` encapsulates serialization; receivers use `Reply::decode::<BusinessType>()`
to decode payloads. Remote selects only handshake/global-error types and never
interprets business fields. Handlers use checkpoint for cancellation.

SSH lives in shared `src/ssh/`. `--ssh` and `--ssh-mcpm` are top-level global options,
accepted before/after subcommands. Mod list TUI/Console/Raw and mod source identify use
them; other commands explicitly reject them. ssh-mcpm requires ssh and defaults
to mcpm on the remote PATH. There is no legacy mod list --remote entry point.

```sh
mcpm --ssh 'ssh -i key user@host' mod list mods/ --no-ui
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm.exe' --raw mod list 'C:\mods'
```

[Command reference](README.md) · [简体中文](../docs-zh/remote.md)

Remote error payloads preserve `message` and ordered `causes` separately. The local
frontend renders `remote error:` followed by indented `caused by:` entries.
