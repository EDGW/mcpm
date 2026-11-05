# Operation

在 messaging `Session` 的对端执行带类型的操作。每次调用占用一个 Channel：调用方发送请求，
执行方回复一次，之后这个 Channel 上发生什么由输出类型决定。

## 概念

- **Operation**：实现 `Operation` 的类型，包含线上 `ID`、`Input`、`Output` 和异步 `run`。
- **Output**：任意 `OperationOutput`。 
  - 所有 serde 类型：回复携带值并结束 Channel。
  - `StreamingOutput<T>`：回复是握手，之后的 item 在同一个 Channel 上发送。
- **Registry**：执行方可运行的操作，按 `ID` 索引。
- **Reply**：执行方发送结果的句柄。

`execute::<O>(input)` 是统一的类型化执行入口：开始时记录包含 operation ID、`SHALLOW` 分类的 INFO 日志，再直接调用 `O::run`，不经过 serde。本地 Executor、Shallow 工作区解析、远端已注册处理器共用此入口。远端 Executor 也在派发请求时记录 INFO；主机实际执行日志仍属于远端诊断。

## 用法

```rust
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

    // 远端错误的原因链完整保留：操作、上下文、I/O 错误。
    let error = call::<Load>(&client, &"/missing".to_owned()).await.unwrap_err();
    assert_eq!(error.chain().count(), 3);
    assert!(format!("{error:#}").starts_with("operation `load` failed: failed to read /missing: "));
    Ok(())
}
```

## API

### 核心 trait

```rust
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

可在 Session 对端运行的带类型操作。

- **`ID`**：线上标识符。在 Registry 中必须唯一。用于请求和错误消息。
- **`Input`** 和 **`Output`**：以 JSON 跨越 Session 边界。`Input` 在执行方反序列化；
  `Output` 实现 `OperationOutput` 以控制结果如何发送以及 Channel 是否复用。
- **`run(input)`**：操作体。在执行方运行。此函数返回的 `anyhow::Error` 会连同完整原因链
  传给调用方。

#### `OperationOutput`

决定操作结果如何投递，以及第一条回复之后 Channel 是否继续使用。

- **`deliver(self, reply)`**：执行方。用 `reply` 发送结果，之后可选地接管 Channel 发送
  更多消息。投递完成时返回 `Ok(())`，失败时返回 `Err`（Channel 结束、Session 关闭、编码错误）。
- **`receive(value, channel)`**：调用方。从第一条回复的 value 构造 `Self` 并接管 Channel。
  `value` 格式错误或接收设置失败时返回 `Err`。

所有 serde 类型自动实现 `OperationOutput`：`deliver` 发送值并结束 Channel；`receive` 解码
值并丢弃 Channel。使用 `StreamingOutput<T>` 在同一 Channel 上发送多个 item。

### 函数

```rust
pub async fn call<O: Operation>(session: &Session, input: &O::Input) -> Result<O::Output>;
pub async fn serve(session: &mut Session, registry: Arc<Registry>) -> Result<()>;
```

#### `call::<O>(session, input) -> Result<O::Output>`

在对端运行操作 `O` 并返回其输出。

开启 Channel，发送 `{"op":"<O::ID>","input":<编码的输入>}`，等待第一条回复：
- `{"ok":<value>}`：调用 `O::Output::receive(value, channel)` 并返回结果。
- `{"error":{...}}`：重建远端 `anyhow::Error` 及其原因链，在 ``operation `<O::ID>` failed``
  上下文下返回。

第一条回复后 Channel 交给 `O::Output`；操作层不再触碰它。普通输出的 Channel 已被对端结束；
流式输出的 Channel 传输 item 直到流结束。

在 future 完成前丢弃它会中止 Channel，这会停止执行方的 `run`。

返回值：
- `Ok(output)`：操作成功。流式输出的 item 投递错误在流内部，不在这里。
- `Err`：操作失败、输入非法、操作未知、回复前 Channel 结束，或 Session 关闭。

#### `serve(session, registry) -> Result<()>`

从 `session` 接受 Channel 并分派到 `registry` 中的操作。每个请求在独立 task 中运行。

对每个 Channel：
1. 解码请求 `{"op","input"}`。
2. 在 Registry 中查找 `op`。未知操作作为错误报告。
3. 反序列化 `input` 并启动 `O::run(input)`。
4. 等待 `run` 完成，然后调用 `O::Output::deliver` 发送结果或错误。

Session 干净结束时返回 `Ok(())`，协议错误或载体故障时返回 `Err(SessionError)`。

### `Registry`

```rust
impl Registry {
    pub fn new() -> Self;
    pub fn register<O: Operation>(&mut self) -> Result<(), DuplicateOperation>;
}
```

执行方可运行的操作表。

#### `Registry::register::<O>() -> Result<(), DuplicateOperation>`

将操作 `O` 加入 Registry。如果 `O::ID` 已被另一操作占用则返回 `Err`。

Registry 不存储 `O` 本身，只存储一个函数：解码其输入、运行它、投递其输出。

### `Reply`

```rust
impl Reply {
    pub fn finish<T: Serialize>(self, value: &T) -> Result<()>;
    pub fn send<T: Serialize>(self, value: &T) -> Result<Channel>;
}
```

执行方发送操作第一条回复的句柄。由 `deliver` 持有。

#### `Reply::finish(value) -> Result<()>`

将 `value` 编码为 `{"ok":<value>}`，带 `finish` 标记发送，并消耗 reply。Channel 对双方结束。
普通输出使用此方法。

`value` 序列化失败、Channel 已结束或 Session 关闭时返回 `Err`。

#### `Reply::send(value) -> Result<Channel>`

将 `value` 编码为 `{"ok":<value>}`，不带 `finish` 发送，并返回 Channel。Channel 保持开启；
输出可继续发送消息。流式输出使用此方法。

成功时返回 Channel，失败情况与 `finish` 相同时返回 `Err`。

### `StreamingOutput<T>`

```rust
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

由函数生产的 `T` 流。一个操作输出最多携带一个流。

#### `StreamingOutput::new(producer) -> Self`

创建流，其 item 由 `producer` 发送。producer 是一个函数，接收 `StreamSender<T>` 并通过
它发送 item。

producer 在第一次 `next()` 调用时启动，包括执行方 `deliver` 运行时。它在独立 task 中运行；
`StreamSender` 的 `send` 在生产者与消费者之间缓冲 64 个 item 时等待。

producer 返回 `Err` 时，流以该错误作为最后一个 item 结束。producer panic 时，流以包装的
panic 消息结束。

#### `StreamingOutput::next() -> Option<Result<T>>`

返回流的下一个 item。

- `Some(Ok(item))`：producer 生产的一个 item。
- `Some(Err(error))`：producer 返回 `Err` 或 panic。这是最后一个 item；后续 `next()`
  调用返回 `None`。
- `None`：producer 返回 `Ok(())`。流已结束。

调用方，`error` 是 `RemoteError`（item 来自对端）。执行方（本地流），`error` 是 producer
的 `anyhow::Error`。

流结束前丢弃它会中止 producer task（本地）或中止 Channel（远端）。

#### `StreamSender::send(item) -> Result<(), StreamClosed>`

将 `item` 发送给消费者。缓冲 64 个 item 时等待。

消费方离开时返回 `Err(StreamClosed)`：流被丢弃或 Channel 被中止。

## 行为细节

### 错误传播

执行方的所有 `anyhow::Error` 连同完整原因链传给调用方：

- `run` 返回 `Err(e)`：`e` 的原因链序列化为嵌套的 `{"message","source"}` 并作为
  `{"error":{...}}` 发送。调用方重建为 `RemoteError` 并包裹在 ``operation `ID` failed`` 下。
- 流 producer 返回 `Err(e)` 或 panic：同样序列化，作为流错误 `{"error":{...}}` 带 `finish`
  发送。调用方的 `next()` 返回 `Some(Err(e))`，包裹在 `remote stream failed` 下。

示例原因链：``operation `load` failed: failed to read /missing: No such file or directory``。
外层上下文由 `call` 添加，中层上下文由 `run` 添加，内层错误由 I/O 操作添加。

### 取消

- **丢弃 `call` 的 future**：Channel 被中止。执行方的 `run` 如果尚未返回则被丢弃。
- **丢弃远端 `StreamingOutput`**：Channel 被中止。执行方的 `deliver` 循环退出，producer
  task 被中止。
- **丢弃本地 `StreamingOutput`**：producer task 被中止。Channel（如果有）不受影响。

### 流式缓冲与反压

- **本地流**（producer 与 consumer 在同一进程）：64 个 item 在缓冲区等待时 producer 暂停。
  consumer 的 `next()` 从此缓冲区拉取 item。
- **远端流**：
  - 执行方：`deliver` 从 producer 拉取并在 Channel 上发送 `{"item":<item>}`。producer
    在它与 `deliver` 之间缓冲 64 个 item 时暂停。在 Channel 上发送的 item 进入 Session
    的无界发送队列：慢消费者不会让 producer 在 64 item 缓冲之外暂停。**消费者比 producer
    慢时，执行方内存增长。**
  - 调用方：`next()` 从 Channel 接收 `{"item"}`。Channel 的入站队列无界，慢消费者会在
    那里积累 item。

### 第一条回复后的 Channel 所有权

操作层只管理 Channel 到第一条回复：
- **执行方**：`deliver` 接收包裹 Channel 的 `Reply`。`reply.finish()` 发送结果并消耗
  Channel（已结束）。`reply.send()` 发送结果并将 Channel 返回给输出。普通输出使用 `finish`；
  流式输出使用 `send` 并持有 Channel 发送 item。
- **调用方**：`receive` 获得第一条回复的 value 和 Channel。普通输出解码 value 并丢弃
  Channel（已被对端结束）。流式输出持有 Channel 并从中接收 item。

## 线上格式

每条消息都是操作 Channel 上的一个 messaging body。

| 消息 | 发送方 | Body |
| --- | --- | --- |
| 请求 | 调用方 | `{"op":"add","input":[2,3]}` |
| 结果 | 执行方 | `{"ok":5}`，普通输出带 `finish` |
| 错误 | 执行方 | `{"error":{"message":"…","source":{"message":"…"}}}`，带 `finish` |
| 流握手 | 执行方 | `{"ok":{}}` |
| 流 item | 执行方 | `{"item":0}` |
| 流错误 | 执行方 | `{"error":{…}}`，带 `finish` |
| 流结束 | 执行方 | 无 body，`finish` |

- `source` 是下一个原因，最后一个原因省略该字段。
- 协议不期望的消息（例如调用方的第二条消息）会使操作失败。

## 错误

| 类型 | 含义 |
| --- | --- |
| `RemoteError` | 来自对端的错误；`source` 链保存远端原因。 |
| `ProtocolError` | `Invalid`（请求、响应或流消息格式错误）、`Missing`（响应前 Channel 已结束）、`Unexpected` |
| `DuplicateOperation` | 两个已注册操作的 `ID` 相同。 |
| `StreamClosed` | 流的消费方已离开。 |
