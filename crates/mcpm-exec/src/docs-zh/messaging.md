# Messaging

在一对字节流上复用多路相互独立的会话。每条消息的内容是一个 `serde_json::Value`；
框架根据每个帧携带的 GUID 和标记自动管理 Channel 的生命周期。

## 概念

- **Carrier（载体）**：任何实现 `Carrier` 的类型，即可拆分为 `AsyncRead`（来自对端）
  与 `AsyncWrite`（发往对端）两半。
  - `(reader, writer)` 元组，例如 `(ChildStdout, ChildStdin)` 或 `(Stdin, Stdout)`。
  - `TcpStream`。
  - `DuplexStream`，用于进程内的两端。
- **Session**：一个载体的两端，负责开启和接受 Channel。
- **Channel**：一段会话，由双方共享的 GUID 标识。
- **Frame（帧）**：线上的一条消息，即一行紧凑 JSON。

Session、Channel 句柄创建和销毁时均产生 debug 日志。日志包含进程内 `session` 标识，Channel 还包含 GUID。句柄生命周期日志与载体读写任务的关闭日志分开；仍有 Channel 持有载体时，销毁 Session 不会立即关闭传输。

## 用法

```rust
use mcpm_exec::messaging::Session;
use serde_json::json;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (client, server) = tokio::io::duplex(4096);
    let client = Session::new(client);
    let mut server = Session::new(server);

    // 第一条消息在对端开启 Channel。
    let mut call = client.open();
    call.send(json!({"op": "scan"}))?;

    let mut request = server.accept().await?.expect("peer opened a channel");
    assert_eq!(request.recv().await?, Some(json!({"op": "scan"})));
    request.finish(Some(json!({"ok": true})))?;

    // 最后一条消息先于 Channel 结束送达。
    assert_eq!(call.recv().await?, Some(json!({"ok": true})));
    assert_eq!(call.recv().await?, None);
    Ok(())
}
```

## API

### `Session`

```rust
impl Session {
    pub fn new(carrier: impl Carrier) -> Self;
    pub fn open(&self) -> Channel;
    pub async fn accept(&mut self) -> Result<Option<Channel>, std::sync::Arc<SessionError>>;
    pub async fn closed(&self) -> Result<(), std::sync::Arc<SessionError>>;
}
```

#### `Session::new(carrier) -> Session`

启动两个独立任务：读任务从载体解码帧并路由到 Channel；写任务将排队的帧编码并写入载体。
需要 Tokio 运行时。

Session 运行至对端关闭写端、收到非法帧、载体出错，或所有句柄（`Session` 本身和所有
`Channel`）都被丢弃。

#### `Session::open() -> Channel`

用新的 UUID v4 创建 Channel 并立即返回。对端在此 Channel 发送第一条消息前不会收到通知。

如果 Session 已经结束，返回的 Channel 生来就是关闭的：`send`/`finish` 返回
`ChannelError::SessionClosed`，`recv` 返回 `Err(SessionClosed)`。

#### `Session::accept() -> Result<Option<Channel>, SessionError>`

等待对端开启的下一个 Channel。当收到未见过的 GUID 的帧时，开启一个新 Channel。

返回值：
- `Ok(Some(channel))`：对端的一个新 Channel。第一条消息可能已在 Channel 的队列里。
- `Ok(None)`：对端干净地关闭了写端（载体上的 EOF）。
- `Err(SessionError)`：Session 因协议错误或载体故障结束。错误中包含原因。已 `accept`
  的 Channel 仍可继续读取，直到读完。

### `Channel`

```rust
impl Channel {
    pub fn id(&self) -> Uuid;
    pub fn send(&self, body: Value) -> Result<(), ChannelError>;
    pub fn finish(self, body: Option<Value>) -> Result<(), ChannelError>;
    pub async fn recv(&mut self) -> Result<Option<Value>, ChannelError>;
}

impl Drop for Channel;
```

Session 内一段会话的句柄。每个 Channel 由双方共享的 GUID 标识。

#### `Channel::id() -> Uuid`

返回 Channel 的 GUID。双方看到的是同一个 UUID。

#### `Channel::send(body) -> Result<(), ChannelError>`

将 `body` 加入发往对端的队列。消息被放进 Session 的发送队列后函数立即返回；不等载体
写出该帧，也不等对端收到。

本地开启的 Channel 的第一条消息会在对端开启这个 Channel：对端的 `accept()` 会返回
这个 Channel。

返回值：
- `Ok(())`：消息已排队。除非 Session 在写任务到达它之前结束，否则会被写出。
- `Err(ChannelError::Finished(id))`：Channel 已被任意一方结束。在远端 FINISH 帧到达前
  发送的消息已排队，可能还在传输中；对端会丢弃发往已结束 Channel 的帧。
- `Err(ChannelError::Aborted(id))`：对端中止了 Channel（未结束就丢弃）。同样存在竞争：
  得知中止之前排队的帧还在传输中。
- `Err(ChannelError::SessionClosed)`：Session 已结束。写任务不再运行。

#### `Channel::finish(body) -> Result<(), ChannelError>`

对双方结束 Channel，可附带最后一条消息。此调用之后：
- 本端：`send` 和 `finish` 返回 `Finished`。`recv` 读完已收到的消息后返回 `Ok(None)`。
- 对端：`recv` 收到所有消息（含最后一条），然后返回 `Ok(None)`。`send` 和 `finish` 在
  对端读任务处理完 FINISH 帧后返回 `Finished`。

消耗 Channel。如果 `body` 为 `None` 且 Channel 是本地开启但从未发过消息，则不写任何内容：
对端永远不知道这个 Channel。

返回值与 `send` 相同，竞争情况也相同。

#### `Channel::recv() -> Result<Option<Value>, ChannelError>`

接收对端的下一条消息。如果暂无消息则等待。

返回值：
- `Ok(Some(body))`：对端的一条消息。`null` 是合法消息。
- `Ok(None)`：对端结束了 Channel，且所有已收到的消息都已读完。对端可能发送了最后一条
  消息，它会是最后一个 `Ok(Some(_))`。
- `Err(ChannelError::Aborted(id))`：对端中止了 Channel，且所有已收到的消息都已读完。
  用于区分"对端放弃了"和"对端完成了"。
- `Err(ChannelError::SessionClosed(cause))`：Session 结束，且所有已收到的消息都已读完。
  正常关闭（对端发送 EOF）时 `cause` 为 `None`，协议错误或载体故障时为 `Some(error)`。

同一 Channel 内的消息按发送顺序送达。不同 Channel 的消息任意交错。

#### `drop(Channel)`

如果 Channel 仍处于开启状态（未被任意一方结束），丢弃它会发送带 `abort` 标记的 FINISH。
对端的 `recv` 在读完已收到的消息后返回 `ChannelError::Aborted`，对端的 `send`/`finish`
会返回 `Aborted`。

如果 Channel 已结束或 Session 已结束，drop 无操作。

## 线上格式

UTF-8，每行一个紧凑 JSON 对象，以 `\n` 结尾。

```json
{"id":"6f1c2c1e-3b9e-4d55-9a57-3f6f0e0b8a10","body":{"op":"scan"}}
{"id":"6f1c2c1e-3b9e-4d55-9a57-3f6f0e0b8a10","flags":["finish"],"body":{"ok":true}}
{"id":"6f1c2c1e-3b9e-4d55-9a57-3f6f0e0b8a10","flags":["finish"]}
```

| 字段 | 类型 | 规则 |
| --- | --- | --- |
| `id` | UUID | Channel GUID。由开启 Channel 的一方生成；同一 Session 内不得复用。 |
| `flags` | 字符串数组 | 为空时省略。未知标记视为协议错误。 |
| `body` | 任意 JSON | 除非 `flags` 含 `finish`，否则必填。`null` 也是一条消息。 |

| 标记 | 含义 |
| --- | --- |
| `finish` | Channel 的最后一帧，对双方生效。 |
| `abort` | 发送方未结束就丢弃了 Channel。只能与 `finish` 一起出现。 |

## 错误

| 类型 | 变体 |
| --- | --- |
| `SessionError` | `Read`、`Write`（载体 I/O）、`Encode`、`Protocol`（`FrameError`） |
| `FrameError` | `Json`（非法 JSON 或未知标记）、`MissingBody`、`AbortWithoutFinish` |
| `ChannelError` | `Finished(id)`、`Aborted(id)`、`SessionClosed(cause)`；正常关闭时 `cause` 为 `None` |
