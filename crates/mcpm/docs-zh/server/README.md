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
| [创建服务端](new.md) | `new`；服务器的创建与下载。 |
| [基础](basics.md) | `info`、`info --upgrade`、`init`；明确的服务端身份与登记格式 |
| [启动与连接](launch.md) | `launch`、`attach`；服务器的启动和监视窗口。 |
| [Stash](stash.md) | `create`、`switch`、`status`、`list`、`info`、`rename`、`delete`、`verify` |

## 目标选择

| 目标传入方式 | 命令 |
| --- | --- |
| 直接通过主参数传入路径 | `new`、`info`、`init` |
| 通过 `--server PATH` 指定 | `launch`、`attach`、`stash` |

省略目标路径时，默认使用当前目录 `.`。

`mods` 等尚未归入本目录的命令见 [旧文档](../legacy/README.md)。

## 输出

所有命令都支持 Console 和 Raw，字段与 Raw 列顺序见各命令页面。诊断和进度使用 stderr；大量记录前的确认直接读写终端。stdout 被重定向时使用 Raw。

`info`、`init` 和 `new` 使用相同的服务端信息摘要，包含身份、RCON 状态及启动配置文件状态。摘要中的 RCON 密码脱敏；`new` 创建 RCON 后另行显示完整密码。
