# Messaging

Multiplexes independent conversations over one byte stream pair. Each message body is a
`serde_json::Value`; the framework manages channel lifecycles from GUIDs and flags carried by
every frame.

## Concepts

- **Carrier**: anything that implements `Carrier`, i.e. splits into an `AsyncRead` half (from
  the peer) and an `AsyncWrite` half (to the peer).
  - `(reader, writer)` tuples, e.g. `(ChildStdout, ChildStdin)` or `(Stdin, Stdout)`.
  - `TcpStream`.
  - `DuplexStream`, for in-process peers.
- **Session**: both ends of one carrier. Opens and accepts channels.
- **Channel**: one conversation, identified by a GUID shared by both sides.
- **Frame**: one message on the wire, one line of compact JSON.

## Usage

```rust
use mcpm_exec::messaging::Session;
use serde_json::json;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (client, server) = tokio::io::duplex(4096);
    let client = Session::new(client);
    let mut server = Session::new(server);

    // The first message opens the channel on the peer.
    let mut call = client.open();
    call.send(json!({"op": "scan"}))?;

    let mut request = server.accept().await?.expect("peer opened a channel");
    assert_eq!(request.recv().await?, Some(json!({"op": "scan"})));
    request.finish(Some(json!({"ok": true})))?;

    // A final message is delivered before the end of the channel.
    assert_eq!(call.recv().await?, Some(json!({"ok": true})));
    assert_eq!(call.recv().await?, None);
    Ok(())
}
```

## API

| Item | Behavior |
| --- | --- |
| `Session::new(carrier)` | Starts the reader and writer tasks. Requires a Tokio runtime. |
| `Session::open()` | Creates a channel with a fresh UUID v4. Nothing is sent until its first message. |
| `Session::accept()` | Next channel opened by the peer. `Ok(None)` after a clean close; `Err` with the cause otherwise. |
| `Channel::id()` | GUID shared by both ends. |
| `Channel::send(body)` | Queues a message; does not wait for the carrier. |
| `Channel::finish(body)` | Finishes the channel for both sides, with an optional final message. |
| `Channel::recv()` | `Ok(Some(body))` per message, `Ok(None)` once finished and drained. |
| `drop(Channel)` | Finishes an unfinished channel without a final message. |

## Behavior

- **Opening**: a frame with an unknown GUID opens a new channel on the receiving side, returned
  by `accept`. A locally opened channel finished before any message is never seen by the peer.
- **Reuse**: every frame with an open channel's GUID goes to that channel.
- **Ordering**: messages of one channel arrive in send order; channels interleave freely.
- **FINISH**: sent or received, it ends the channel for both sides.
  - The receiving side gets the final message (if any), then `Ok(None)`.
  - Later `send`/`finish` fail with `ChannelError::Finished`.
  - GUIDs of finished channels are remembered for the whole session. Frames for them, such as
    a message that crossed a FINISH, are dropped instead of opening a new channel.
- **Session end**: the peer closes its writing side, sends an invalid frame, or the carrier fails.
  - Open channels yield their received messages, then `ChannelError::SessionClosed`.
  - `accept` yields channels already opened, then `Ok(None)` or the `SessionError`.
  - Dropping the `Session` and every `Channel` closes this side's writer after queued frames
    are written. Exception: Tokio's `Stdout` stays open, so the peer sees EOF when the process
    exits.
- **Buffering**: send and inbound queues are unbounded, so a slow channel never blocks the
  others.

## Wire format

UTF-8, one compact JSON object per line, terminated by `\n`.

```json
{"id":"6f1c2c1e-3b9e-4d55-9a57-3f6f0e0b8a10","body":{"op":"scan"}}
{"id":"6f1c2c1e-3b9e-4d55-9a57-3f6f0e0b8a10","flags":["finish"],"body":{"ok":true}}
{"id":"6f1c2c1e-3b9e-4d55-9a57-3f6f0e0b8a10","flags":["finish"]}
```

| Field | Type | Rule |
| --- | --- | --- |
| `id` | UUID | Channel GUID. Chosen by the side that opens the channel; never reused in a session. |
| `flags` | array of strings | Omitted when empty. Unknown flags are a protocol error. |
| `body` | any JSON | Required unless `flags` contains `finish`. `null` is a message. |

| Flag | Meaning |
| --- | --- |
| `finish` | Last frame of the channel, for both sides. |

## Errors

| Type | Variants |
| --- | --- |
| `SessionError` | `Read`, `Write` (carrier I/O), `Encode`, `Protocol` (`FrameError`) |
| `FrameError` | `Json` (invalid JSON or unknown flag), `MissingBody` |
| `ChannelError` | `Finished(id)`, `SessionClosed(cause)`; `cause` is `None` after a clean close |
