# Remote 会话与分发

`mcpm remote` 使用一个进程承载整个会话。调用方可通过一个非 PTY 的 SSH 连接持续发送、并行执行多条请求。Remote 只负责会话、GUID 路由、并发限制、取消和全局错误；每个命令自己定义、编码和解码业务 contents，不依赖命令行参数解析。

```sh
ssh -T user@host mcpm remote
ssh -T user@windows C:/mcpm.exe remote
```

双向使用 UTF-8 NDJSON，每条消息是一个单行 JSON 加换行，写完立即刷新。字符串内换行由 JSON 转义。stdout 仅包含协议，正常运行时日志通过独立日志会话传输；stderr 仅保留启动或连接失败诊断，无颜色或终端界面。

## Command ID、调用会话 GUID 与统一回复

- **command ID**：命令类型的字符串 ID，由命令模块声明并通过 initcall 注册。当前注册 `mod.list`、`mod.info`、`mod.deps`、`mod.source.identify`、`server.info`、`server.init`、`server.init.recognize`、`server.mods.list`、`server.mods.info`、`server.mods.deps`，以及 `server.mods.source.search`、`server.mods.source.files`、`server.mods.source.identify`。
- **调用会话 GUID**（request GUID）：客户端为每次调用自动生成 UUID v4，用于关联该调用的所有回复。同一连接内不得复用；同一 command ID 可以同时发起多次调用，各自拥有不同 GUID。

请求包含自动生成的调用会话 GUID、字符串 command ID 和不透明 contents：

```json
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","command":"mod.list","contents":{"paths":["C:\\mods"],"loaders":["fabric"]}}
```

**每条回复都恰好由 `guid`、`status`、`contents` 三部分组成**：

```json
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","status":"data","contents":{"event":"count","count":1}}
```

| status | contents 解释方 | 含义 |
| --- | --- | --- |
| `ready` | Remote | 会话握手，公布版本、已注册 command ID 和并发上限。 |
| `data` | 对应命令 | 不透明的命令数据，可包含进度、结果或业务错误。 |
| `finished` | Remote | 该调用正常结束，contents 为 null；不代表业务一定成功。 |
| `error` | Remote | 全局错误，contents 为 code/message；该调用异常结束，不再发送 finished。 |
| `closed` | Remote | 会话正常结束，contents 为 null。 |

连接级控制消息使用全零 GUID `00000000-0000-0000-0000-000000000000`，该 GUID 不可用于请求。

## 握手、并行和生命周期

先等待握手：

```json
{"guid":"00000000-0000-0000-0000-000000000000","status":"ready","contents":{"version":3,"commands":["mod.deps","mod.info","mod.list","mod.source.identify","server.info","server.init","server.init.recognize","server.mods.deps","server.mods.info","server.mods.list","server.mods.source.files","server.mods.source.identify","server.mods.source.search"],"max_concurrency":8}}
```

调用方检查版本与需要的 command ID。支持能力来自 initcall 注册表，不从 CLI 子命令推断。目前 mod list、mod info、mod deps 与 mod source identify 注册。

可以连续提交请求，无需等待前一条结束。同一请求内的回复保持顺序，不同请求的回复允许交错；调用方按 request GUID 路由。远端持续读 stdin，独立执行命令，由一个写入器向 stdout 写完整消息。本地同样由一个读取器按 GUID 分发给各请求句柄。

`mcpm remote --max-concurrency 8` 设置并发上限，默认 8。超出上限的新调用返回 `busy` 全局错误，稍后可以用新的 request GUID 重试。

关闭 stdin 表示不再提交请求。服务端先完成所有已接收任务，再发送全零 GUID 的 closed 回复并成功退出，即使此前有调用失败。丢弃本地 Session 会关闭并回收子进程。 Session 关闭时以 info 日志报告 `stdin_bytes`（本地发送）、`stdout_bytes`（本地接收）和 `total_bytes`。统计包含握手、命令、日志、控制帧和换行的实际 UTF-8 字节，不含 SSH 协议开销或 stderr；异常退出也会报告一次。

## 远端日志会话

本地 Session 验证 Ready 后自动生成一个新的 UUID v4，建立独立日志会话，同时传递本地 `--log-level`：

```json
{"logs":{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","level":"debug"}}
{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","status":"ready","contents":null}
{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","status":"data","contents":{"level":"debug","target":"mcpm::remote::server","fields":{"message":"Received remote command"},"spans":[]}}
```

日志是 Remote 的连接级能力，不注册为业务命令，也不占用命令并发额度。同一连接只允许订阅一次，GUID 不能为零或与任何调用复用。日志保留级别、模块、字段和 span；命令 span 包含对应调用 GUID，允许追踪并行命令。

远端日志及远端错误诊断默认不在 stderr 显示，只有添加全局参数 `--remote-logs` 后才显示；文件日志和失败退出码不受影响。终端同时按本地 `--log-level` 过滤日志，远端日志的 `INFO`、`DEBUG` 等级别标签采用斜体，不添加前缀。Raw、重定向或 `NO_COLOR` 下不输出样式。文件日志仍至少保留到 debug，因此即使终端为 off 也会传输 debug 日志；trace 仅在选择 trace 时传输，日志文件仍固定记录到 debug。远端也保留自己的日志文件。

订阅前缓存最近 256 条启动日志，超过上限时附带丢弃数量警告。关闭 stdin 后先排空命令，再排空日志，发送日志 GUID 的 finished，最后发送全零 GUID 的 closed。本地 TUI 打开期间延迟终端日志显示，关闭面板后输出；文件日志持续写入。当前协议版本为 3，调用方和远端需同时更新。

## 全局错误与取消

Remote 解码全局错误，不把它作为业务 contents 交给命令：

```json
{"guid":"9b6dc7aa-a81c-4f39-81bd-0776b6524381","status":"error","contents":{"code":"unsupported_command","message":"unsupported remote command ID: ..."}}
```

错误码为 `unsupported_command`、`busy`、`cancelled`、`handler_failed`、`protocol`。前四类只结束目标调用，其他调用仍继续。无法分发的异常或处理器 panic 使用 handler_failed。命令参数校验、文件加载等业务错误由命令协议表达。

输入帧损坏、全零或重复的 request GUID 属于会话级 protocol 错误，使用全零 GUID 回复并失败退出，未完成任务被取消。意外 EOF、无关 GUID、缺少终止回复、异常进程退出均应判为传输失败。

取消不占用新的 request GUID：

```json
{"cancel":"9b6dc7aa-a81c-4f39-81bd-0776b6524381"}
```

取消是协作式的；mod list 在文件之间检查，不保证立即打断文件 I/O。未知或已完成 GUID 的取消无操作，完成可能赢得竞态。取消生效后返回 cancelled 全局错误，之前收到的数据是部分结果。丢弃未完成的请求句柄会尝试取消，Remote 仍排空该调用的回复，避免影响其他调用。

## mod list 的 contents

以下内容完全由 mod list 模块编解码，Remote 不认识其字段或 event。

请求支持 `parallelism` 正整数，默认 10，控制远端外层 JAR 的读取与 loader 解析并发。它独立于 identify 的并发参数，不影响 SSH 连接数或 Remote 会话分发。示例：`{"paths":["C:\\mods"],"loaders":[],"parallelism":4}`。取消会停止派发，等待正在执行的同步文件解析返回；不会强行中断 I/O。

请求：

| 字段 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `paths` | 字符串数组 | `[]` | 远端文件和目录，空数组使用远端当前目录。 |
| `loaders` | 字符串数组 | `[]` | forge、neoforge、fabric，空数组尝试全部。 |
| `parallelism` | 正整数 | `10` | 外层 JAR 加载与 loader 解析的最大并发数。 |

未知字段、非法类型或 Loader 值会发送业务 error，再发送业务 done（success:false），最后由 Remote 发送 finished。stdin 只用于会话，不能作为命令路径输入。列、筛选、展开、颜色、宽度等展示参数留在本地前端。

业务回复 contents 依次为 count、item、done：

```json
{"event":"count","count":1}
{"event":"item","index":0,"record":{"path":"C:\\mods\\example.jar","name":"example","enabled":true,"status":"unrecognized","metadata":[],"errors":[]}}
{"event":"done","success":true}
```

每项加载完成立即发送 item，同时推进进度；必须恰好 count 条，index 对应原始输入位置，完整覆盖 `0..count`，每个索引恰好出现一次；各项允许乱序到达，调用方按索引恢复列表顺序。成功、失败、未识别均保留且仅保留一条记录；重复规范化路径共享扫描结果，但每个输入位置分别返回记录，空目录贡献零项，无法解析的输入路径贡献一条失败记录。文件失败不停止其他项，使 done.success 为 false。取消和传输异常可能使结果不完整。

record 包含 path、name、enabled、status、metadata、errors。输入失败时 name/enabled 为 null；status 为 success、unrecognized、failed。metadata 包含 loader、mods 与递归 nested；errors 包含 loader（输入错误时为 null）、message 和按外层到根因排序的 causes 数组。添加 `--remote-logs` 时，本地用 `remote error:` 输出主错误，用缩进的 `caused by:` 逐层输出原因，不把整条错误链压成单行字符串。文件内多个声明、Loader、嵌套 JAR 和部分成功结果都保留在同一条记录中。

## mod info 和 deps 的 contents

`mod.info` 与 `mod.deps` 共用检查数据获取流程和结构化协议：

```json
{"directory":"D:\\mods","loaders":["fabric"],"selection":{"queries":[{"kind":"file","value":"example.jar"},{"kind":"modid","value":"nested_mod"}]}}
```

可选 `parallelism` 必须为正整数，默认 10，限制 `--all` 与名称/ID 索引扫描的文件并发。
多个独立文件查询仍按顺序读取，不受此参数影响。

所有字段均可省略。directory 默认为 `.`，loaders 为空表示全部内置 Loader；
selection 默认为无输入，成功返回空结果。queries 保留顺序和重复项，kind 可为
`auto`、`file`、`modid`、`name`，value 必须为 UTF-8。auto 依次匹配文件名、精确 ID、
精确名称。`selection.all: true` 检查目录且不可与 queries 混用。相对路径在远端
相对于 directory 解析，文件查询不接受目录。名称/ID 匹配选择所属归档及其后代，
保留该归档中的其他声明。stdin 不交给远端 CLI 解析。

未缓存的文件读取和目录扫描会发送 `scan` 进度：从 completed=0 和扫描总数 total 开始，
每完成一项递增一次，失败、未识别文件和目录中的缓存项均计入进度。空目录只发送一次 0/0。
直接命中单文件缓存或已建索引时不发送扫描事件。上一次扫描达到 total 后才能开始下一次。
进度在解析期间实时发送，早于选中文件结果；客户端校验序列，并依据本地 stderr 是否为 TTY 显示。

采集期间发送 `scan`、`file` 和 `error`，最后恰好一个 `done`。
诊断可以穿插在扫描期间，选中文件在扫描完成后发送：

```json
{"event":"scan","completed":0,"total":1}
{"event":"scan","completed":1,"total":1}
{"event":"file","index":0,"file":{"path":"D:\\mods\\example.jar","filename":"example.jar","enabled":true,"metadata":[{"loader":"fabric","loader_name":"Fabric","records":[{"info":{"modid":"example","name":"Example","version":"1.0","dependencies":[]},"source":[]}]}],"failed_loaders":[]}}
{"event":"error","error":{"message":"inspection failed","causes":["underlying cause"]}}
{"event":"done","files":1,"failed":1,"ignored":0}
```

index 从零连续递增，done.files 必须与 file 事件数量一致。records 已包含查询子树选择，
每项为类型化 Mod 声明和内部归档 source 链。路径和文件名由远端平台解析，客户端不读取
这些路径。颜色和路径显示样式留在本地。Loader 部分失败时保留成功元数据和 failed_loaders，
完整错误链单独发送。info 不显示没有元数据的文件，deps 可显示其识别不完整状态。

failed 为失败扫描文件去重计数加选择失败次数；重复失败文件共享扫描结果。名称/ID 索引
可能发现其他目录项的错误。ignored 为 Loader 筛选下跳过的未识别文件次数；未指定筛选时
未识别文件视为错误。输入和识别错误不影响后续查询。参数错误发送 error，然后发送 files=0、failed=1、
ignored=0 的 done。业务失败仍跟随 finished。执行失败（包括工作线程或消费者失败）立即终止采集并
向调用方传播；远端通过全局 error 结束，不再发送业务 done 或 finished。
取消使用 cancelled 全局状态并可留下部分结果。
在查询之间、选中文件之间、目录扫描完成项之间检查取消。客户端校验序号、数量、诊断与失败
状态的一致性以及事件生命周期。通信协议版本为 3，两端需要同步更新。

## mod source identify 的 contents

Command ID 为 `mod.source.identify`，由 identify 模块注册，**只返回文件摘要**。CLI 和 mod list 的 TUI 共用本地识别调度与平台客户端。Remote 与 SSH 通用模块不理解这些字段。

```json
{"paths":["D:\\mods\\a.jar","D:\\mods\\b.jar"],"requirements":{"murmur2":true},"parallelism":10}
```

`paths` 必须非空；`requirements.murmur2` 表示是否额外计算 CurseForge 指纹，默认 false。SHA-1 和文件大小始终计算。`parallelism` 必须为正整数，默认 10，控制远端摘要读取并发。参数不能包含未知字段，**不传平台配置、API key 或代理**。

业务事件中的 `indices` 是原始 paths 的零起始位置数组。同一物理文件的重复路径共享读取，事件同时列出这些位置。摘要一次读取得到，可供两个平台共用，不重复读取文件。

```json
{"event":"loading","indices":[0]}
{"event":"item","indices":[0],"outcome":{"status":"summary","summary":{"size":3,"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d","murmur2":1621425345}}}
{"event":"loading","indices":[1]}
{"event":"item","indices":[1],"outcome":{"status":"failed","error":{"message":"failed to summarize file ...","causes":["read failed","file not found"]}}}
{"event":"done","failed":1}
```

MurmurHash2 使用 seed=1，忽略 TAB、LF、CR 和 SPACE；未请求时 murmur2 为 null。摘要成功、失败均保留每个原始输入位置。每个位置恰好经历一次 loading 和一次 item，item 可以乱序到达；done.failed 按原始路径位置计数，再由通用 Remote 发送 finished。接收端校验索引、重复项、完整性、失败总数和摘要格式。参数无效、取消、处理器异常通过通用 error 结束调用，允许保留部分结果。

本地收到摘要后向 Modrinth / CurseForge 发起请求，验证返回文件的大小及 SHA-1，保留 primary/additional 语义。网络出口、代理和官方 API key 均属于本地，远端不访问平台。CLI 立即更新完成进度，但按输入顺序打印结果与逐项错误。远端读文件失败仅在指定 `--remote-logs` 时用 `remote error:` 输出，本地网络查询失败始终用 `error:` 输出。

SSH mod list 的 TUI 在同一 Session 上请求摘要，再使用相同的本地识别流程。`--identify-parallelism` 控制本地所有文件与平台合计的在途查询数，并传给远端控制摘要读取。退出 TUI 时取消本地网络任务和未完成的远端摘要调用，无需等待网络超时。Console 与 Raw 列表不自动识别来源。

日志用 execution=local/remote 和 phase=summary/provider 区分执行阶段，包含请求、摘要读取、结果校验、失败和耗时。远端日志通过日志会话回传，终端以斜体级别标记。本次调整改变了该 command 的 contents，客户端和远端均需更新；不保留旧的远端平台查询 schema。

## Server 命令的 contents

协议版本继续使用 3。客户端检查握手声明的命令 ID；旧版 v3 远端若没有这些命令，会明确返回 unsupported_command，使用前需更新远端程序。

- [`server.init` 与 `server.init.recognize`](server/init.md#ssh-与初始化协议)：独立的完整参数初始化和只读识别，本地选择候选。
- [`server.info`](server/info.md#ssh)：来源主机的元数据快照，在本地渲染。
- [`server.mods.info/deps`](server/mods.md)：远端解析服务器范围，复用检查事件流。
- [`server.mods.list`](server/mods.md#列表)：远端选择文件、回传进度和记录，本地展示 TUI/Console/Raw。
- [`server.mods.source.search/files/identify`](server/source.md#ssh)：远端提供兼容参数或文件摘要，来源服务请求及凭据留在本地。

非法请求或无法加载服务器元数据时，以终止性的全局 `handler_failed` 状态结束。选择和扫描错误可通过业务事件报告，同时继续返回有效结果。取消、执行错误及传输失败后不发送业务完成事件。初始化在原子写入前检查取消；超过该边界后中断，即使没有成功回复，也可能已写入配置。远端诊断需要 `--remote-logs` 才显示，但失败仍影响退出状态。

## 模块注册与 SSH

各模块通过 `inventory::submit! { Initcall(init) }` 提交自己的 initcall。程序启动统一执行 initcall，模块在自己的 init 中调用 `remote::registry::register::<Command>()`。Remote 不引用业务实现，中央入口不逐项列举命令。重复 command ID 在启动时明确报错。命令自己定义业务类型，通过 `Output::send(业务值)` 发送回复；序列化由 `Reply::new` 封装，接收端通过 `Reply::decode::<业务类型>()` 解码。Remote 仅选择握手和全局错误类型，不认识业务字段；处理器使用 checkpoint 响应取消。

SSH 公共模块位于 `src/ssh/`。`--ssh` 和 `--ssh-mcpm` 是最外层全局选项，可以出现在子命令前后；支持 `mod list/info/deps/source identify`、`server init/info` 和 `server mods list/info/deps/source search/files/identify`。列表和来源浏览的 TUI 在本地运行，检查命令支持 Console/Raw。`server mods check` 和 `dep-graph` 明确拒绝 SSH。`--ssh-mcpm` 需配合 `--ssh`，省略时使用远端 PATH 中的 mcpm。

```sh
mcpm --ssh 'ssh -i key user@host' mod list mods/ --no-ui
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm.exe' --raw mod list 'C:\mods'
```

[命令参考](README.md) · [列表命令](mod/list.md) · [English](../docs/remote.md)
