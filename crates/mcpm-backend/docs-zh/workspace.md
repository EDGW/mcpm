# Workspace operations

Workspace 是调用方本机用于选择目标的入口。Local workspace 使用本机目录；Remote workspace（stub）保存 SSH 连接和远端路径，使用远端目标。

Workspace operation 属于 Shallow，由调用方本机执行。

## 操作

| Operation | 分类 | 输入 | 输出 |
| --- | --- | --- | --- |
| `workspace.inspect` | Shallow | `InspectInput { path }` | `WorkspaceInfo { kind, connection, remote_path }` |
| `workspace.register_remote` | Shallow | `RegisterRemoteInput { path, connection, remote_path, force }` | `()` |

## `workspace.inspect`

读取本机工作区登记，返回工作区类型。Remote 结果包含连接和远端路径；Local 结果的 `connection`、`remote_path` 为 `None`。

| 情况 | 行为 |
| --- | --- |
| 缺少 `.mcpm/mcpm.json` | 按旧 Local 布局处理 |
| 类型为 Local | 返回 Local |
| 类型为 Remote | 读取 v3 `.mcpm/server.json`，返回连接和远端路径 |
| 标记损坏、不可读或版本不支持 | 返回错误 |

只读取登记，不启动 SSH，也不读取远端服务端元数据。

## 目标选择

| 情况 | 目标 |
| --- | --- |
| 显式 `--ssh` | 指定主机；传入路径按该主机解释 |
| 没有显式 `--ssh`，路径为 Remote workspace | 登记中的远端连接和路径 |
| 没有显式 `--ssh`，路径为 Local 或未登记目录 | 本机 |
| 没有路径 | 显式 `--ssh` 或本机，不检查 workspace |

显式 `--ssh` 优先于保存的登记。连接或登记失败时不改选其他目标。

## `workspace.register_remote`

规范化为已存在的本机目录，获取登记租约，写入连接 stub 和 Remote 类型标记：

| 文件 | 内容 |
| --- | --- |
| `.mcpm/server.json` | version 3：`ssh`、`ssh_mcpmd`、`path` |
| `.mcpm/mcpm.json` | version 1，`type: remote` |
| `.mcpm/.gitignore` | 元数据忽略规则 |

`force` 允许替换已有 Remote 连接。不能把已有 Local 工作区转换为 Remote，也不能替换配置路径上的目录。并发修改登记时立即返回 busy。

两个 JSON 文件分别原子写入，不是跨文件事务。首次发布类型标记失败时，会移除新连接登记；进程突然终止可能留下无法加载的登记。

登记租约覆盖写入序列，Writer 结束时释放。

## Remote stub 创建流程

1. Host `server.init.prepare_remote` 校验远端 Local Server，并完成 Git 准备。
2. Shallow `workspace.register_remote` 写入本机连接登记。

stub 不执行 Git，也不重新发布远端服务端配置。第 2 步失败不会回滚第 1 步已经完成的 Git 准备。
