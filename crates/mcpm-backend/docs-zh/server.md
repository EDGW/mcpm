# Server operations

Server operation 处理实际服务端目录。Host 指实际保存服务端目录的机器；Executor 决定在本机还是远端执行，operation 只处理传入的路径和数据。mcpm 负责参数、交互和展示。

## 操作

| Operation | 分类 | 输入 | 输出 |
| --- | --- | --- | --- |
| `server.info` | Host | `InfoInput { path }` | `ServerInfo` |
| `server.init.recognize` | Host | `Recognize { path, start_script, minecraft_version, loader, loader_version }` | `StreamingOutput<RecognizeEvent>` |
| `server.init` | Host | `Initialize { path, metadata, force, no_git, git, local }` | `StreamingOutput<InitEvent>` |
| `server.init.prepare_remote` | Host | `PrepareRemoteInput { path, no_git, git }` | `Initialized` |

`Recognize` 的筛选字段可为 null；`Initialize.local` 可为 null。`PrepareRemoteInput` 的 `git`、`no_git` 为布尔字段。实际目标必须是 Local Server；目标本身是 Remote workspace 时拒绝执行。

## `server.info`

读取服务端身份、历史启动快照，以及 `launch.toml`、`conf.toml` 是否存在。配置文件只检查存在性，不解析内容，也不生成启动计划。

返回的路径和启动参数保留来源主机的平台写法。服务端登记缺失或损坏时返回错误。

## `server.init.recognize`

静态读取启动脚本和身份依据，不执行脚本、Java 或安装器。已有服务端登记损坏不影响识别；识别本身不写文件、不创建 Git 仓库、不发布配置。

版本和 Loader 参数只筛选识别出的候选，不补造缺失字段。每个候选包含元数据、来源行、主类和配置草稿。

流依次返回阶段事件，最后返回候选列表。前端在本机选择候选，再把所选草稿提交给 `server.init`；服务端不保存识别会话状态。

## `server.init`

接收完整元数据，来源可以是手动参数或识别结果。负责校验、Git 准备、配置发布和工作区标记。

`local` 为 `Some` 时，调用 mcpm-lib 发布身份、启动配置和 `conf.toml`；为 `None` 时只登记身份。完整手动身份不生成启动配置。

Git 行为：

| 选项 | 行为 |
| --- | --- |
| 默认 | 复用目标仓库、父目录仓库或 worktree；均不存在时创建仓库 |
| `git` | 要求服务端目录自身具有仓库 |
| `no_git` | 跳过检查和创建 |

`git` 与 `no_git` 不能同时为真。不自动暂存或提交。仓库损坏或 Git 执行失败时返回错误。

流依次返回校验、写入阶段，最后返回 `Initialized { server, git, publication }`。mcpm-lib 协调身份、launch 和 `conf.toml` 的发布；发布成功后写入 Local 工作区类型标记。

## `server.init.prepare_remote`

用于创建 Remote workspace 前准备已有远端服务端。校验目标为 Local Server，并执行请求的 Git 检查或创建；不发布服务端配置，`publication` 为 `None`。

返回 `Initialized { server, git, publication: None }`。本机 stub 由 Shallow `workspace.register_remote` 随后写入；该 operation 不操作本机 stub，也不创建本机 Git 仓库。

## 进度与失败

识别和初始化使用流式输出，最后一条业务事件携带结果。阻塞文件或库操作通过 `spawn_blocking` 执行。

流被丢弃后，生产方在下次发送事件时停止；已经运行的阻塞任务不能中途取消。文件提交后，连接中断不会自动回滚。发布、类型标记或传输失败后，重试前先检查服务端文件。
