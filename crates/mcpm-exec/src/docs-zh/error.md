# 错误

一个错误在 mcpm 里要经过三个环节：业务代码产生它，operation 把它从远端 Host 送回调用方，前端把它展示给用户。
`mcpm_exec::error` 定义这三个环节共用的错误形式，不依赖任何业务代码。

`anyhow::Error` 只有一条线性的原因链，但一次失败可能由多个互相独立的失败组成，例如保存失败之后，清理也
失败了。两者各有各的上下文，不能压成一条链，所以需要在 `anyhow` 之上补充这套模型。

## 该用哪种错误

| 情况 | 使用 | 调用方看到 |
| --- | --- | --- |
| 整个 operation 失败 | 返回 `Err(anyhow::Error)` | 带完整原因链的错误；失败发生在远端时会标明来自对端 |
| 一次失败包含多个独立失败 | `CombinedError` | 一个错误，其下按顺序挂着各个独立失败 |
| 一批结果中某一项失败，其余仍然有效 | 在结果里放 `PortableError` | 该项带着完整错误，其余项照常使用 |

## 普通错误

业务代码用 `.context(...)` 逐层补充信息，展示时由外到内输出整条链。具体的错误类型只在产生它的进程里
保留，错误从远端传回后只剩消息，不能再按类型判断。

## `CombinedError`

把多个独立失败合成一个错误：

```rust
let error = anyhow::Error::new(CombinedError::new(first, second));
```

- 至少两个失败；用 `push` 追加更多。
- `errors()` 按原始顺序返回各个失败，它们可以再次是 `CombinedError`。
- 它的消息只写"N independent errors"，不重复各个失败的内容。各个失败是同级的，不在 `source()` 链上，
  只遍历原因链会看不到它们，需要展示的一端自己逐个展开。

## 远端失败

operation 在远端 Host 上失败时，错误被转成可以传输的 `ErrorReport`，包含最外层消息、其余原因链和各个
独立失败。调用方收到后把它恢复成 `anyhow::Error`，并在最外面套上 `RemoteError`，表示这个失败来自另一个进程。

- 用 `error::contains::<RemoteError>(error)` 判断失败是否来自对端；它会搜索所有独立失败。
- `RemoteError` 只是来源标记，不属于原始失败。转发已经收到的失败时不会重复添加。
- operation 在本机执行时不经过传输，调用方得到原始错误，不带 `RemoteError`。

各种失败在调用方看到的形式：

| 来源 | 调用方收到 |
| --- | --- |
| `Operation::run` 返回 `Err` | 恢复出的错误，附上下文 ``operation `ID` failed`` |
| 流式输出的生产方返回 `Err` 或 panic | 恢复出的错误，附上下文 `remote stream failed` |
| 启动远端连接时出错 | 恢复出的错误，附启动连接的上下文 |

错误的传输格式取代了旧的 `message`/`source` 链，因此本机的 `mcpm` 和远端的 `mcpmd` 必须是同一个构建。

## `PortableError`

用于"一项失败不应使整个 operation 失败"的场景，例如 `Vec<Result<Entry, PortableError>>`，或流里的一个失败事件。

- 在本机由 `anyhow::Error` 构造时，保留原始错误及其类型，不加标记。
- 经传输到达调用方时，恢复出完整的错误并带上 `RemoteError` 标记；是否带标记只取决于它是否经过传输，与
  执行位置无关。
- 用 `into_inner()` 取出 `anyhow::Error`，交给统一的诊断展示。

整个 operation 的失败不要放进 `PortableError`，直接返回 `Err`，由 operation 自动传输。

## 展示

- 通过统一的诊断入口展示：先输出 `error:`，再逐级输出 `caused by:`。
- 直接渲染 `ErrorReport` 时，要递归展开其中的独立失败；只遍历 `anyhow::Error::chain()` 看不到它们。
- 不要把 `Debug` 输出当作面向用户的诊断。

## 编写约定

- 整个 operation 失败返回 `Err`；只有某一项的结果才用 `PortableError`。
- 在比调用方掌握更多信息的地方添加上下文，不要给每次调用都加。
- 不要给已经带 `RemoteError` 的失败再包一层，转发时由传输层处理。
- 最外层消息要告诉用户该做什么，细节放在原因链里。
