# Server 命令族

入口：

| 入口 | 命令格式 |
| --- | --- |
| `mcpm` | `mcpm <命令>` |
| `mcpm-server` | `mcpm-server <命令>` |
| `mcpm-tools` | `mcpm-tools server <命令>` |

以下以 `mcpm` 展示。通用术语、全局选项和输出模式见 [README](../README.md)。

| 页面 | 内容 |
| --- | --- |
| [基础](basics.md) | `info`、`init` |
| [Stash](stash.md) | `create`、`switch`、`status`、`list`、`info`、`rename`、`delete`、`verify` |

## 目标选择

| 命令 | 目标参数 | 默认值 |
| --- | --- | --- |
| `info` | 位置参数 PATH | `.` |
| `init` | 位置参数 PATH；带 `--remote` 时 PATH 是本机工作区 | `.` |
| `stash` | `--server PATH`，可放在子命令前后 | `.` |

- 显式 `--ssh` 指定远端主机，PATH 按那台主机上的路径解释。
- 没有 `--ssh` 时，看 PATH 指向的目录：如果它是 Remote workspace（本机上保存 SSH 连接和远端服务端路径的目录），命令会作用到它登记的远端服务端；否则作用于本机的 Local Server。
- `init --remote` 必须同时给出 `--ssh`，它的 PATH 是本机的 Remote workspace 目录，而不是服务端目录。
- 命令总是在保存服务端的那台机器上执行，远端通过 `mcpmd` 完成，同一个 SSH 目标会复用连接。
- 连接失败、登记损坏等错误不会改为在本机执行。
- 远端需要安装与本机构建一致的 `mcpmd`。

本页的命令都支持 Remote workspace 和显式 `--ssh`。`launch` 与 `mods` 暂时不在其中，见 [旧文档](../legacy/README.md)。

## 输出

所有命令都支持 Console 和 Raw，字段与 Raw 列顺序见各命令页面。诊断和进度使用 stderr；大量记录前的确认直接读写终端。stdout 被重定向时使用 Raw。
