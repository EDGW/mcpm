# Operation

Runs typed operations on the peer of a messaging `Session`. Each call uses one channel: the
caller sends a request, the executing side sends one reply, then the output type decides what
else happens on that channel.

## Concepts

- **Operation**: a type implementing `Operation`, with a wire `ID`, an `Input`, an `Output` and
  an async `run`.
- **Output**: any `OperationOutput`.
  - Every serde type: the reply carries the value and finishes the channel.
  - `StreamingOutput<T>`: the reply is a handshake; items follow on the same channel.
- **Registry**: the operations the executing side runs, by `ID`.
- **Reply**: the executing side's handle for sending the result.

`execute::<O>(input)` is the shared typed execution entry. It records one INFO start event with the operation ID and `SHALLOW` classification, then calls `O::run` without serde. Local Executor calls, Shallow workspace resolution, and registered peer handlers use this entry. A remote Executor also records INFO when dispatching the request; peer execution logs remain remote diagnostics.

## Usage

```rust,ignore
use std::sync::Arc;

use anyhow::{Context, Result};
use mcpm_exec::messaging::Session;
use mcpm_exec::operation::{Operation, Registry, StreamingOutput, call, serve};

struct Add;
impl Operation for Add {
    const ID: &'static str = "add";
    type Input = (i64, i64);
    type Output = i64;
    async fn run((a, b): (i64, i64)) -> Result<i64> {
        Ok(a + b)
    }
}

struct Count;
impl Operation for Count {
    const ID: &'static str = "count";
    type Input = u32;
    type Output = StreamingOutput<u32>;
    async fn run(to: u32) -> Result<StreamingOutput<u32>> {
        Ok(StreamingOutput::new(move |items| async move {
            for n in 0..to {
                items.send(n).await?;
            }
            Ok(())
        }))
    }
}

struct Load;
impl Operation for Load {
    const ID: &'static str = "load";
    type Input = String;
    type Output = String;
    async fn run(path: String) -> Result<String> {
        std::fs::read_to_string(&path).with_context(|| format!("failed to read {path}"))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let (client, server) = tokio::io::duplex(4096);
    let client = Session::new(client);
    let mut server = Session::new(server);

    let mut registry = Registry::new();
    registry.register::<Add>()?;
    registry.register::<Count>()?;
    registry.register::<Load>()?;
    tokio::spawn(async move { serve(&mut server, Arc::new(registry)).await });

    assert_eq!(call::<Add>(&client, &(2, 3)).await?, 5);

    let mut numbers = call::<Count>(&client, &3).await?;
    let mut received = Vec::new();
    while let Some(n) = numbers.next().await {
        received.push(n?);
    }
    assert_eq!(received, [0, 1, 2]);

    // The remote cause chain survives: operation, context, I/O error.
    let error = call::<Load>(&client, &"/missing".to_owned()).await.unwrap_err();
    assert_eq!(error.chain().count(), 3);
    assert!(format!("{error:#}").starts_with("operation `load` failed: failed to read /missing: "));
    Ok(())
}
```

## API

### Core traits

```rust,ignore
pub trait Operation {
    const ID: &'static str;
    type Input: Serialize + DeserializeOwned;
    type Output: OperationOutput;
    async fn run(input: Self::Input) -> Result<Self::Output>;
}

pub trait OperationOutput: Sized {
    async fn deliver(self, reply: Reply) -> Result<()>;
    fn receive(value: Value, channel: Channel) -> Result<Self>;
}
```

#### `Operation`

A typed operation that can run on the peer of a session.

- **`ID`**: wire identifier. Must be unique within a registry. Used in the request and in error
  messages.
- **`Input`** and **`Output`**: cross the session boundary as JSON. `Input` is deserialized on
  the executing side; `Output` implements `OperationOutput` to control how the result is sent
  and whether the channel is reused.
- **`run(input)`**: the operation body. Runs on the executing side. `anyhow::Error` from this
  function reaches the caller with its full cause chain.

#### `OperationOutput`

Decides how an operation result is delivered and whether the channel continues after the first
reply.

- **`deliver(self, reply)`**: executing side. Sends the result using `reply`, then optionally
  takes over the channel for more messages. Returns `Ok(())` when delivery completes, `Err` when
  it fails (channel ended, session closed, encoding error).
- **`receive(value, channel)`**: calling side. Builds `Self` from the first reply's value and
  takes over the channel. Returns `Err` when `value` is malformed or when receiving setup fails.

Every serde type automatically implements `OperationOutput`: `deliver` sends the value and finishes
the channel; `receive` decodes the value and drops the channel. Use `StreamingOutput<T>` to send
multiple items on the same channel.

### Functions

```rust,ignore
pub async fn call<O: Operation>(session: &Session, input: &O::Input) -> Result<O::Output>;
pub async fn serve(session: &mut Session, registry: Arc<Registry>) -> Result<()>;
```

#### `call::<O>(session, input) -> Result<O::Output>`

Runs operation `O` on the peer and returns its output.

Opens a channel, sends `{"op":"<O::ID>","input":<encoded input>}`, waits for the first reply:
- `{"ok":<value>}`: calls `O::Output::receive(value, channel)` and returns the result.
- `{"error":{...}}`: reconstructs the remote `anyhow::Error` with its cause chain and returns it
  under the context ``operation `<O::ID>` failed``.

The channel is passed to `O::Output` after the first reply; the operation layer no longer touches
it. For plain outputs the channel is already finished by the peer; for streaming outputs the
channel carries items until the stream ends.

Dropping the returned future before it completes aborts the channel, which stops the executing
side's `run`.

Returns:
- `Ok(output)`: the operation succeeded. For streaming outputs, errors during item delivery are
  inside the stream, not here.
- `Err`: the operation failed, the input was invalid, the operation is unknown, the channel ended
  before the reply, or the session closed.

#### `serve(session, registry) -> Result<()>`

Accepts channels from `session` and dispatches them to operations in `registry`. Each request runs
in its own task.

For each channel:
1. Decodes the request `{"op","input"}`.
2. Looks up `op` in the registry. Unknown operations are reported as errors.
3. Deserializes `input` and spawns `O::run(input)`.
4. Waits for `run` to complete, then calls `O::Output::deliver` to send the result or error.

Returns `Ok(())` when the session ends cleanly, `Err(SessionError)` when it ends with a protocol
error or carrier failure.

### `Registry`

```rust,ignore
impl Registry {
    pub fn new() -> Self;
    pub fn register<O: Operation>(&mut self) -> Result<(), DuplicateOperation>;
}
```

The table of operations the executing side can run.

#### `Registry::register::<O>() -> Result<(), DuplicateOperation>`

Adds operation `O` to the registry. Returns `Err` if `O::ID` is already taken by another operation.

The registry does not store `O` itself, only a function that decodes its input, runs it, and
delivers its output.

### `Reply`

```rust,ignore
impl Reply {
    pub fn finish<T: Serialize>(self, value: &T) -> Result<()>;
    pub fn send<T: Serialize>(self, value: &T) -> Result<Channel>;
}
```

The executing side's handle for sending the first reply of an operation. Held by `deliver`.

#### `Reply::finish(value) -> Result<()>`

Encodes `value` as `{"ok":<value>}`, sends it with the `finish` flag, and consumes the reply. The
channel ends for both sides. Use this for plain outputs.

Returns `Err` when `value` fails to serialize, the channel already ended, or the session closed.

#### `Reply::send(value) -> Result<Channel>`

Encodes `value` as `{"ok":<value>}`, sends it without `finish`, and returns the channel. The
channel stays open; the output can send more messages. Use this for streaming outputs.

Returns the channel on success, `Err` with the same cases as `finish`.

### `StreamingOutput<T>`

```rust,ignore
impl<T> StreamingOutput<T> {
    pub fn new<F, Fut>(producer: F) -> Self
    where
        F: FnOnce(StreamSender<T>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send;

    pub async fn next(&mut self) -> Option<Result<T>>;
}

impl<T> StreamSender<T> {
    pub async fn send(&self, item: T) -> Result<(), StreamClosed>;
}
```

A stream of `T` produced by a function. One operation output carries at most one stream.

#### `StreamingOutput::new(producer) -> Self`

Creates a stream whose items are sent by `producer`. The producer is a function that receives a
`StreamSender<T>` and sends items through it.

The producer starts on the first `next()` call, including when `deliver` runs on the executing
side. It runs in its own task; `send` on the `StreamSender` waits while 64 items are buffered
between the producer and the consumer.

If the producer returns `Err`, the stream ends with that error as the last item. If the producer
panics, the stream ends with a wrapped panic message.

#### `StreamingOutput::next() -> Option<Result<T>>`

Returns the next item from the stream.

- `Some(Ok(item))`: an item produced by the producer.
- `Some(Err(error))`: the producer returned `Err` or panicked. This is the last item; subsequent
  `next()` calls return `None`.
- `None`: the producer returned `Ok(())`. The stream has ended.

On the calling side, `error` is a `RemoteError` when the items come from the peer. On the
executing side (local stream), `error` is the producer's `anyhow::Error`.

Dropping the stream before it ends aborts the producer task (local) or aborts the channel (remote).

#### `StreamSender::send(item) -> Result<(), StreamClosed>`

Sends `item` to the consumer. Waits while 64 items are buffered.

Returns `Err(StreamClosed)` when the consumer is gone: the stream was dropped or the channel was
aborted.

## Behavior details

### Error propagation

Every `anyhow::Error` from the executing side reaches the calling side with its full cause chain:

- `run` returns `Err(e)`: the chain of `e` is serialized into nested `{"message","source"}` and
  sent as `{"error":{...}}`. The caller reconstructs it as a `RemoteError` and wraps it with
  ``operation `ID` failed``.
- Stream producer returns `Err(e)` or panics: same serialization, sent as a stream error
  `{"error":{...}}` with `finish`. The caller's `next()` returns `Some(Err(e))` wrapped with
  `remote stream failed`.

Example chain: ``operation `load` failed: failed to read /missing: No such file or directory``.
The outer context is added by `call`, the middle context by `run`, the inner error by the I/O
operation.

### Cancellation

- **Dropping `call`'s future**: the channel is aborted. The executing side's `run` is dropped if
  it has not returned yet.
- **Dropping a remote `StreamingOutput`**: the channel is aborted. The executing side's `deliver`
  loop exits, the producer task is aborted.
- **Dropping a local `StreamingOutput`**: the producer task is aborted. The channel (if any) is
  unaffected.

### Streaming buffering and backpressure

- **Local stream** (producer and consumer in the same process): the producer pauses while 64 items
  wait in the buffer. The consumer's `next()` pulls items from this buffer.
- **Remote stream**:
  - Executing side: `deliver` pulls from the producer and sends `{"item":<item>}` on the channel.
    The producer pauses while 64 items wait between it and `deliver`. Items sent on the channel
    enter the session's unbounded send queue: a slow consumer does not pause the producer beyond
    the 64-item buffer. **Memory grows on the executing side when the consumer is slower than the
    producer.**
  - Calling side: `next()` receives `{"item"}` from the channel. The channel's inbound queue is
    unbounded, so a slow consumer accumulates items there.

### Channel ownership after the first reply

The operation layer manages the channel only until the first reply:
- **Executing side**: `deliver` receives a `Reply` wrapping the channel. `reply.finish()` sends
  the result and consumes the channel (it is finished). `reply.send()` sends the result and
  returns the channel to the output. A plain output uses `finish`; a streaming output uses `send`
  and keeps the channel to send items.
- **Calling side**: `receive` gets the first reply's value and the channel. A plain output decodes
  the value and drops the channel (already finished by the peer). A streaming output keeps the
  channel and receives items from it.
