# Backend 模块

Backend 定义 operation 和 mcpm 自身数据的 codec。operation 不决定执行位置；本机或远端由 mcpm 的 Executor 选择。

| 模块 | 内容 |
| --- | --- |
| [Server operations](server/README.md) | 服务端信息、初始化、Remote stub 准备与 Mod 快照 |
| [Workspace operations](workspace.md) | 本机 workspace 检查和 Remote 连接登记 |

当前 operation 覆盖 server 和 workspace；其他命令仍在 mcpm 的 legacy 路径。

[mcpm 命令](../../mcpm/docs-zh/README.md) · [English](../docs/README.md)
