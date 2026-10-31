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

| 项 | 行为 |
| --- | --- |
| `Session::new(carrier)` | 启动读、写任务。需要 Tokio 运行时。 |
| `Session::open()` | 用新的 UUID v4 创建 Channel。发送第一条消息前不向线上写任何内容。 |
| `Session::accept()` | 对端开启的下一个 Channel。正常关闭后返回 `Ok(None)`，否则返回带原因的 `Err`。 |
| `Channel::id()` | 双方共享的 GUID。 |
| `Channel::send(body)` | 将消息加入发送队列，不等待载体。 |
| `Channel::finish(body)` | 对双方结束 Channel，可附带最后一条消息。 |
| `Channel::recv()` | 每条消息返回 `Ok(Some(body))`；结束且读完后返回 `Ok(None)`。 |
| `drop(Channel)` | 未结束的 Channel 被结束，不附带消息。 |

## 行为

- **开启**：接收方收到未知 GUID 的帧时开启新 Channel，由 `accept` 返回。本地开启后
  未发送任何消息即结束的 Channel 对端不可见。
- **复用**：所有携带已开启 Channel GUID 的帧都投递到该 Channel。
- **顺序**：同一 Channel 内的消息按发送顺序到达；不同 Channel 之间任意交错。
- **FINISH**：无论发出还是收到，Channel 对双方都结束。
  - 接收方先收到最后一条消息（若有），再收到 `Ok(None)`。
  - 之后的 `send`/`finish` 返回 `ChannelError::Finished`。
  - 已结束 Channel 的 GUID 在整个 Session 内保留。发往它们的帧（例如与 FINISH
    交错的消息）被丢弃，不会开启新 Channel。
- **Session 结束**：对端关闭写端、发送非法帧，或载体出错。
  - 已开启的 Channel 先读完已收到的消息，再返回 `ChannelError::SessionClosed`。
  - `accept` 先返回已开启的 Channel，再返回 `Ok(None)` 或 `SessionError`。
  - `Session` 与所有 `Channel` 都被丢弃后，写完已排队的帧，再关闭本端写端。例外：Tokio 的
    `Stdout` 不会被关闭，对端在进程退出时才收到 EOF。
- **缓冲**：发送与接收队列均无上限，慢的 Channel 不会阻塞其他 Channel。

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

## 错误

| 类型 | 变体 |
| --- | --- |
| `SessionError` | `Read`、`Write`（载体 I/O）、`Encode`、`Protocol`（`FrameError`） |
| `FrameError` | `Json`（非法 JSON 或未知标记）、`MissingBody` |
| `ChannelError` | `Finished(id)`、`SessionClosed(cause)`；正常关闭时 `cause` 为 `None` |
