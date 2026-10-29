# Remote 会话与分发

`mcpm-tools remote` 使用一个进程承载整个会话。调用方可通过一个非 PTY 的 SSH 连接持续发送、并行执行多条请求。Remote 只负责会话、GUID 路由、并发限制、取消和全局错误；每个命令自己定义、编码和解码业务 contents，不依赖命令行参数解析。

```sh
ssh -T user@host mcpm-tools remote
ssh -T user@windows C:/mcpm-tools.exe remote
```

双向使用 UTF-8 NDJSON，每条消息是一个单行 JSON 加换行，写完立即刷新。字符串内换行由 JSON 转义。stdout 仅包含协议，正常运行时日志通过独立日志会话传输；stderr 保留非正式 release 构建的启动提示，以及启动或连接失败诊断，无颜色或终端界面。

## Command ID、调用会话 GUID 与统一回复

- **command ID**：命令类型的字符串 ID，由命令模块声明并通过 initcall 注册。当前注册 `mod.list`、`mod.info`、`mod.deps`、`mod.source.identify`、`server.info`、`server.init`、`server.init.recognize`、`server.mods.cache`、`server.mods.list`、`server.mods.info`、`server.mods.deps`，以及 `server.mods.source.search`、`server.mods.source.files`、`server.mods.source.identify`。
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
{"guid":"00000000-0000-0000-0000-000000000000","status":"ready","contents":{"version":5,"commands":["mod.deps","mod.info","mod.list","mod.source.identify","server.info","server.init","server.init.recognize","server.mods.cache","server.mods.deps","server.mods.info","server.mods.list","server.mods.source.files","server.mods.source.identify","server.mods.source.search"],"max_concurrency":8}}
```

调用方检查版本与需要的 command ID。支持能力来自 initcall 注册表，不从 CLI 子命令推断。目前 mods list、mods info、mods deps 与 mods source identify 注册。

可以连续提交请求，无需等待前一条结束。同一请求内的回复保持顺序，不同请求的回复允许交错；调用方按 request GUID 路由。远端持续读 stdin，独立执行命令，由一个写入器向 stdout 写完整消息。本地同样由一个读取器按 GUID 分发给各请求句柄。

`mcpm-tools remote --max-concurrency 8` 设置并发上限，默认 8。超出上限的新调用返回 `busy` 全局错误，稍后可以用新的 request GUID 重试。

关闭 stdin 表示不再提交请求。服务端先完成所有已接收任务，再发送全零 GUID 的 closed 回复并成功退出，即使此前有调用失败。丢弃本地 Session 会关闭并回收子进程。 Session 关闭时以 info 日志报告 `stdin_bytes`（本地发送）、`stdout_bytes`（本地接收）和 `total_bytes`。统计包含握手、命令、日志、控制帧和换行的实际 UTF-8 字节，不含 SSH 协议开销或 stderr；异常退出也会报告一次。

## 远端日志会话

本地 Session 验证 Ready 后自动生成一个新的 UUID v4，建立独立日志会话，同时传递本地 `--log-level`：

```json
{"logs":{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","level":"debug"}}
{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","status":"ready","contents":null}
{"guid":"d1342616-87f2-4846-afb8-0d21c9bbc035","status":"data","contents":{"level":"debug","target":"mcpm::remote::server","fields":{"message":"Received remote command"},"spans":[]}}
```

日志是 Remote 的连接级能力，不注册为业务命令，也不占用命令并发额度。同一连接只允许订阅一次，GUID 不能为零或与任何调用复用。日志保留级别、模块、字段和 span；命令 span 包含对应调用 GUID，允许追踪并行命令。

远端日志默认不在 stderr 显示，只有添加全局参数 `--remote-logs` 后才显示；远端操作错误始终以 `remote error:` 显示，不受 `--remote-logs` 和 `--log-level` 影响。文件日志和失败退出码不受影响。终端同时按本地 `--log-level` 过滤日志，远端日志的 `INFO`、`DEBUG` 等级别标签采用斜体，不添加前缀。Raw、重定向或 `NO_COLOR` 下不输出样式。文件日志仍至少保留到 debug，因此即使终端为 off 也会传输 debug 日志；trace 仅在选择 trace 时传输，日志文件仍固定记录到 debug。远端也保留自己的日志文件。

订阅前缓存最近 256 条启动日志，超过上限时附带丢弃数量警告。关闭 stdin 后先排空命令，再排空日志，发送日志 GUID 的 finished，最后发送全零 GUID 的 closed。本地 TUI 打开期间延迟终端日志显示，关闭面板后输出；文件日志持续写入。当前协议版本为 5，调用方和远端需同时更新。

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

取消是协作式的；mods list 在文件之间检查，不保证立即打断文件 I/O。未知或已完成 GUID 的取消无操作，完成可能赢得竞态。取消生效后返回 cancelled 全局错误，之前收到的数据是部分结果。丢弃未完成的请求句柄会尝试取消，Remote 仍排空该调用的回复，避免影响其他调用。

## mods list 的 contents

以下内容完全由 mods list 模块编解码，Remote 不认识其字段或 event。

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

record 包含 path、name、enabled、status、metadata、errors。输入失败时 name/enabled 为 null；status 为 success、unrecognized、failed。metadata 包含 loader、mods 与递归 nested；errors 包含 loader（输入错误时为 null）、message 和按外层到根因排序的 causes 数组。本地始终用 `remote error:` 输出主错误，用缩进的 `caused by:` 逐层输出原因，不把整条错误链压成单行字符串。文件内多个声明、Loader、嵌套 JAR 和部分成功结果都保留在同一条记录中。

## mods info 和 deps 的 contents

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
状态的一致性以及事件生命周期。通信协议版本为 5，两端需要同步更新。

## mods source identify 的 contents

Command ID 为 `mod.source.identify`，由 identify 模块注册，**只返回文件摘要**。CLI 和 mods list 的 TUI 共用本地识别调度与平台客户端。Remote 与 SSH 通用模块不理解这些字段。

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

本地收到摘要后向 Modrinth / CurseForge 发起请求，验证返回文件的大小及 SHA-1，保留 primary/additional 语义。网络出口、代理和官方 API key 均属于本地，远端不访问平台。CLI 立即更新完成进度，但按输入顺序打印结果与逐项错误。远端读文件失败始终用 `remote error:` 输出，本地网络查询失败始终用 `error:` 输出，均不受 `--remote-logs` 和 `--log-level` 影响。

SSH mods list 的 TUI 在同一 Session 上请求摘要，再使用相同的本地识别流程。`--identify-parallelism` 控制本地所有文件与平台合计的在途查询数，并传给远端控制摘要读取。退出 TUI 时取消本地网络任务和未完成的远端摘要调用，无需等待网络超时。Console 与 Raw 列表不自动识别来源。

日志用 execution=local/remote 和 phase=summary/provider 区分执行阶段，包含请求、摘要读取、结果校验、失败和耗时。远端日志通过日志会话回传，终端以斜体级别标记。本次调整改变了该 command 的 contents，客户端和远端均需更新；不保留旧的远端平台查询 schema。

## Server 命令的 contents

协议版本为 5，两端必须同步升级。客户端检查握手版本及声明的命令 ID。Server 缓存由远端 Server 决定，请求不再接受目录覆盖。

- [`server.init` 与 `server.init.recognize`](server/init.md#远程行为)：独立的完整参数初始化和只读识别，本地选择候选。
- [`server.info`](server/info.md#ssh)：来源主机的元数据快照，在本地渲染。
- [`server.mods.info/deps`](server/mods.md)：远端解析服务器范围，复用检查事件流。
- [`server.mods.list`](server/mods.md#列表)：远端选择文件、回传进度和记录，本地展示 TUI/Console/Raw。
- [`server.mods.cache`](server/mods.md#缓存)：在文件主机显式导入服务器归档并刷新声明 loader 的元数据。
- [`server.mods.source.search/files/identify`](server/source.md#ssh)：远端提供兼容参数或文件摘要，来源服务请求及凭据留在本地。

非法请求或无法加载服务器元数据时，以终止性的全局 `handler_failed` 状态结束。选择和扫描错误可通过业务事件报告，同时继续返回有效结果。取消、执行错误及传输失败后不发送业务完成事件。初始化在原子写入前检查取消；超过该边界后中断，即使没有成功回复，也可能已写入配置。远端错误始终显示，不受 `--remote-logs` 和 `--log-level` 影响；失败返回非零退出状态。

### Server 初始化协议

`server.init.recognize` 接受 `path`（默认 `.`）、可选 `start_script`，以及可选的 `minecraft_version`、`loader`、`loader_version` 候选筛选条件。它先要求 Local 工作目录类型，再只读取脚本和身份依据，与已有 server.json 元数据独立；默认脚本由文件所属主机决定。回传事件：

```json
{"event":"stage","phase":"recognizing"}
{"event":"candidates","candidates":[{"metadata":{"version":1,"minecraft_version":"1.21.1","loader":null,"start":{"script":"run.sh","executable":"java","arguments":["-jar","server.jar"],"windows":false}},"line":1,"main_class":"-jar"}]}
```

`server.init` 接受完整 `metadata`、`path`（默认 `.`）、`force`（默认 false）、`git` 和 `no_git`（均默认 false），不要求识别 token 或缓存候选：

```json
{"path":"/srv/server","metadata":{"version":1,"minecraft_version":"1.21.1","loader":null},"force":false}
```

默认在文件所属主机检测 Git 仓库，已有仓库（含父目录仓库和 worktree）直接复用，否则执行 `git init`。`git: true` 要求目标目录自身的仓库：已有仓库或 worktree 直接复用，否则即使属于父目录仓库也会创建。`no_git: true` 跳过 Git 设置；这两个字段不能同时为 true，分别对应 CLI 的 `--git` 和 `--no-git`。识别请求始终不操作 Git。

`initialized` 事件在 Git 设置启用时还包含 `git` 对象：`{"status":"created","path":"/srv/server"}` 或 `{"status":"existing","path":"/srv/server"}`，供本地 Console 显示创建或跳过的结果；`no_git: true` 时省略此字段，`server.info` 结构保持不变。

元数据要求非空的 Minecraft/Loader 版本及显式 `loader`（原版为 `null`）。可选 `start` 为上面的完整启动快照：

- `script` 必须是相对于 Server 目录的非空路径；初始化和加载都拒绝绝对路径、`..` 越界及已有符号链接越界。识别入口的 `start_script` 同样受此约束。
- 元数据不保存 `directory`；`server.info` 快照的工作目录来自打开 Server 时传入的路径。
- 可执行文件不能为空，可执行文件及参数不能含 NUL。
- 初始化不重读脚本，也不要求引用文件仍存在，但会检查已有脚本路径组件以确认未越界。

wire v5 仍输出 metadata version 1；Local 磁盘持久化写为 version 2。初始化依次回传 `stage {phase: "validating"}`、`stage {phase: "writing"}`、`initialized {server}`，最后一个使用 `server.info` 结构。成功由通用 `finished` 结束，失败以 `error` 结束；客户端校验阶段顺序和唯一结果。需要候选选择时，识别与初始化共用同一 SSH 会话。

所有 server 端点（包括识别）都要求 Local 工作目录类型。Remote 引用仅由前端解析，`mcpm-tools remote` 不跟随引用。初始化可以创建 Local 登记，但即使带 `force` 也不能替换 Remote 工作目录。识别继续作为独立的脚本读取操作，不跟随工作目录引用。

元数据文件分别原子写入，不构成双文件事务。mcpm 登记写入持有工作目录锁，并在写入前重新检查类型。新 Remote 登记先写 `server.json`，再发布 `mcpm.json`；发布类型失败则删除刚创建的服务端文件。进程突然终止仍可能留下无法加载的未完成登记。Local 初始化可能先保存 `server.json` 再发布 Local 标记；标记失败会明确报告部分成功，缺失标记仍按 Local 处理。原子写入完成后，取消或连接中断不回滚配置；结果不确定时先通过 `server info` 检查。

## 模块注册与 SSH

各模块通过 `inventory::submit! { Initcall(init) }` 提交自己的 initcall。程序启动统一执行 initcall，模块在自己的 init 中调用 `remote::registry::register::<Command>()`。Remote 不引用业务实现，中央入口不逐项列举命令。重复 command ID 在启动时明确报错。命令自己定义业务类型，通过 `Output::send(业务值)` 发送回复；序列化由 `Reply::new` 封装，接收端通过 `Reply::decode::<业务类型>()` 解码。Remote 仅选择握手和全局错误类型，不认识业务字段；处理器使用 checkpoint 响应取消。

SSH 公共模块位于 `src/ssh/`。`--ssh` 和 `--ssh-mcpm` 是最外层全局选项，可以出现在子命令前后；支持 `mods list/info/deps/source identify`、`server init/info` 和 `server mods list/info/cache/deps/source search/files/identify`。列表和来源浏览的 TUI 在本地运行，检查命令支持 Console/Raw。`server mods check` 和 `dep-graph` 明确拒绝 SSH。`--ssh-mcpm` 需配合 `--ssh`，省略时使用远端 PATH 中的 mcpm。

SSH 默认 shell 直接启动 `mcpm-tools remote`。Windows 连接要求默认 shell 为 cmd；可执行文件路径自动加双引号，不能包含双引号、换行、NUL、`%` 或 `!`。POSIX 可执行文件使用 shell 引号转义。业务参数通过 JSON 传输，不依赖 shell 语法。

```sh
mcpm-tools --ssh 'ssh -i key user@host' mods list mods/ --no-ui
mcpm-tools --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm-tools.exe' --raw mods list 'C:\mods'
```

[命令参考](README.md) · [列表命令](mod/list.md) · [English](../docs/remote.md)

## Mod 识别缓存参数

CLI 的 `mods info/list` 默认发送使用用户缓存的选择；`--no-cache` 关闭，`--cache-dir PATH` 指定远端目录。结构化协议中的 `cache` 省略或为 null 仍表示关闭，保持已有调用者和 deps 的行为。

`mod.list`、`mod.info` 及共享请求结构的 `mod.deps` 接受可选 `cache`：

```json
{"kind":"default"}
{"kind":"directory","path":"/srv/mod-cache"}
```

- 省略或 null 表示关闭；路径与默认目录都在远端文件所属主机解释。
- 默认目录不存在或为空时初始化；指定目录必须已有受支持的缓存 manifest。目录损坏会使请求失败。
- 通过 imohash/identity 候选匹配或完整 BLAKE3 查找，复用有效的已识别及未识别结果。缺失、过期或无效结果正常识别；检查不导入文件、不建立硬链接、不写回 metadata。
- 全局持久缓存与单次命令的重复输入缓存独立。持久缓存命中仍计为一次文件检查完成；单次命令内复用已有检查才不再次产生扫描进度。

## 识别缓存统计

检查和列表的 `done` 事件包含可选 `cache` 统计：

```json
{"fully_cached":8,"partially_cached":3,"total":15}
```

- 按文件所属主机实际检查的规范化路径去重，包含失败和未识别项，展示筛选前计算；重复输入、嵌套声明不增加总数。
- 全部命中指所有请求 Loader 结果均被复用；部分命中指至少一个被复用且至少一个现场识别。“未识别”缓存也算命中；关闭缓存时两项命中数均为零。
- 与可能包含重复查询和声明选择的 file/item 数独立。客户端拒绝命中总和超过 total 的统计；旧端点缺少统计时不展示。
- 本地 Raw 文本不变；Console 与 list TUI 使用这些结构化统计生成摘要行。

忽略旧端点发送的 `added`、`updated` 字段；检查不再写入缓存数据。

## 来源识别缓存传输

Server 命令使用 Server 运行环境的缓存。Server list scope 和来源选择完成事件携带 `environment:{cache}`；显式 Server 缓存请求首条 data 为 `{cache}`。文件主机解析有效目标，客户端后续读取、发布沿用此目标。详见[服务器 Mod 协议](server/mods.md#远程协议)。

- `mod.source.identify` 可选 `cache`：null／省略关闭，`{"kind":"default"}` 或 `{"kind":"directory","path":"..."}` 选择文件主机目录；CLI 默认开启。
- 成功 summary 可含来源 map `cached`（FileMatch）；空 map 省略以兼容旧端。复用要求 缓存内容文件 且与 summary 一致；客户端检查 key 所有权、hash 格式、大小和 SHA-1 后跳过平台请求。
- `server.mods.cache.writing.metas` 选择 meta／ident，默认两类。远端 handler 导入 缓存内容文件、按需刷新 Loader metadata；客户端存储完成后用本地 provider／凭据补全来源，source 配置不跨 SSH。
- 显式 `cache.mods.identification.write` 接受 `{cache,id,file}`；id 为 BLAKE3，file 为可信的规范来源 FileMatch，cache 使用上述选择。主机验证 缓存内容文件 的 BLAKE3／size／SHA-1，通过库 codec 合并后返回一条 null data 确认。缺失／无效数据使用通用命令错误。Identify 不调用写操作。
- FileMatch 含 `reference {project:{source,id},id}`、`filename`、`size`、`hashes`（`{algorithm,value}`）、`role`（primary／additional）。所有 CurseForge 配置持久化为 `source:"curseforge"`。
- 缓存 record 新增 `metadata_requested`、各来源 `identification`（matched／unmatched／failed，失败含 error 和 remote）、主错误的 `error_remote`。多个 source 失败仍按一个文件计失败，Console 分别按真实来源展示错误。

### 分支诊断（协议 5）

- 错误保留 `message` 和有序 `causes`；可选的 `branches` 数组保存独立失败，每项仍是同样结构的错误报告，可递归嵌套。
- 组合根节点允许空消息，仅包含分支。首分支沿用当前错误或原因标签，后续分支用 `and:`；每条分支自己的原因链再缩进一级。
- 两端必须使用协议 5，避免旧端静默忽略分支而丢失错误信息。
