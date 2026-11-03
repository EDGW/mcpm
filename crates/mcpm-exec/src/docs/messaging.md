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

### `Session`

```rust,ignore
impl Session {
    pub fn new(carrier: impl Carrier) -> Self;
    pub fn open(&self) -> Channel;
    pub async fn accept(&mut self) -> Result<Option<Channel>, SessionError>;
}
```

#### `Session::new(carrier) -> Session`

Starts two independent tasks: a reader that decodes frames from the carrier and routes them to
channels, and a writer that encodes queued frames and writes them to the carrier. Requires a
Tokio runtime.

The session runs until the peer closes its writing side, an invalid frame is received, the carrier
fails, or every handle (`Session` itself and all `Channel`s) is dropped.

#### `Session::open() -> Channel`

Creates a channel with a fresh UUID v4 and returns it immediately. The peer is not notified until
the first message is sent on this channel.

If the session has already ended, the returned channel is born closed: `send`/`finish` fail with
`ChannelError::SessionClosed`, and `recv` returns `Err(SessionClosed)`.

#### `Session::accept() -> Result<Option<Channel>, SessionError>`

Waits for the next channel opened by the peer. A channel opens when a frame arrives with a GUID
not seen before.

Returns:
- `Ok(Some(channel))`: a new channel from the peer. The first message may already be in the
  channel's queue.
- `Ok(None)`: the peer closed its writing side cleanly (EOF on the carrier).
- `Err(SessionError)`: the session ended due to a protocol error or carrier failure. The cause
  is in the error. Channels already `accept`ed can still be read until drained.

### `Channel`

```rust,ignore
impl Channel {
    pub fn id(&self) -> Uuid;
    pub fn send(&self, body: Value) -> Result<(), ChannelError>;
    pub fn finish(self, body: Option<Value>) -> Result<(), ChannelError>;
    pub async fn recv(&mut self) -> Result<Option<Value>, ChannelError>;
}

impl Drop for Channel;
```

A handle for one conversation inside a session. Each channel is identified by a GUID shared by
both sides.

#### `Channel::id() -> Uuid`

Returns the channel's GUID. Both sides see the same UUID.

#### `Channel::send(body) -> Result<(), ChannelError>`

Queues `body` for the peer. The message is added to the session's send queue and the function
returns immediately; it does not wait for the carrier to write the frame or for the peer to
receive it.

The first message of a locally opened channel opens it on the peer: the peer's `accept()` will
return this channel.

Returns:
- `Ok(())`: the message was queued. It will be written unless the session ends before the writer
  reaches it.
- `Err(ChannelError::Finished(id))`: the channel was already finished by either side. Messages
  sent before the remote FINISH arrived were queued and may still be in flight; the peer drops
  frames for finished channels.
- `Err(ChannelError::Aborted(id))`: the peer aborted the channel (dropped it without finishing).
  The same race applies: frames queued before learning about the abort are in flight.
- `Err(ChannelError::SessionClosed)`: the session ended. The writer task is no longer running.

#### `Channel::finish(body) -> Result<(), ChannelError>`

Finishes the channel for both sides, with an optional final message. After this call:
- This side: `send` and `finish` fail with `Finished`. `recv` drains already-received messages,
  then returns `Ok(None)`.
- Peer: `recv` receives all messages including the final one, then returns `Ok(None)`. `send` and
  `finish` fail with `Finished` once the peer's reader processes the FINISH frame.

Consumes the channel. If `body` is `None` and the channel was locally opened but never sent a
message, nothing is written: the peer never learns about this channel.

Returns the same errors as `send`, with the same races.

#### `Channel::recv() -> Result<Option<Value>, ChannelError>`

Receives the next message from the peer. Waits if no message is available yet.

Returns:
- `Ok(Some(body))`: a message from the peer. `null` is a valid message.
- `Ok(None)`: the peer finished the channel and every received message was consumed. The peer may
  have sent a final message, which would have been the last `Ok(Some(_))`.
- `Err(ChannelError::Aborted(id))`: the peer aborted the channel and every received message was
  consumed. Use this to distinguish "the peer gave up" from "the peer completed".
- `Err(ChannelError::SessionClosed(cause))`: the session ended and every received message was
  consumed. `cause` is `None` for a clean close (peer sent EOF), `Some(error)` for a protocol
  error or carrier failure.

Messages are delivered in send order within one channel. Messages of different channels interleave
freely.

#### `drop(Channel)`

If the channel is still open (not finished by either side), dropping it sends FINISH with the
`abort` flag. The peer's `recv` will return `ChannelError::Aborted` after draining received
messages, and the peer's `send`/`finish` will fail with `Aborted`.

If the channel was already finished or the session ended, drop is silent.

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
| `abort` | The sender dropped the channel without finishing it. Only valid with `finish`. |

## Errors

| Type | Variants |
| --- | --- |
| `SessionError` | `Read`, `Write` (carrier I/O), `Encode`, `Protocol` (`FrameError`) |
| `FrameError` | `Json` (invalid JSON or unknown flag), `MissingBody`, `AbortWithoutFinish` |
| `ChannelError` | `Finished(id)`, `Aborted(id)`, `SessionClosed(cause)`; `cause` is `None` after a clean close |
