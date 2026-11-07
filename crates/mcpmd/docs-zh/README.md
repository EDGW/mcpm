# mcpmd

mcpmd 执行远端 operation，并维护可复用的 SSH link。

| 命令 | 内容 |
| --- | --- |
| [`mcpmd serve`](#mcpmd-serve) | 在远端主机上通过 stdin/stdout 运行 operation registry |
| [`mcpmd link`](#mcpmd-link) | 按用户维护 link，每个 SSH 目标复用一条连接 |

## `mcpmd serve`

```text
mcpmd serve
```

从 stdin 读取 operation 请求，向 stdout 写入协议帧。stdout 仅用于协议帧；诊断信息写入 stderr 和日志流。`mcpmd link` 通过 SSH 启动该进程，通常不直接调用。

## `mcpmd link`

### 前台 link

```text
mcpmd link [--mcpmd <PATH>] [--idle-timeout <SECONDS>] SSH
```

| 参数或选项 | 说明 |
| --- | --- |
| `[SSH]` | SSH 连接命令，格式同 `--ssh`。 |
| `--mcpmd <PATH>` | 显式指定远端 mcpmd 可执行文件，优先于安装位置记录。 |
| `--idle-timeout <SECONDS>` | 无客户端且无在途调用时，经过该秒数后退出；`0` 表示不按空闲退出。默认 `600`。 |

已运行的 link 保留启动时的值；要应用修改后的设置，需先停止对应 link。

### 远端程序查找

- 未显式指定路径时，先探测远端命令解释器，不根据本机 OS 选择远端查找规则。
- POSIX 主机读取 `~/.mcpm/mcpmd.path`；Windows 主机读取 `%LOCALAPPDATA%\mcpm\mcpmd.path`。Windows SSH 的默认 shell 必须是 cmd，且需要 Windows PowerShell。
- 记录为 UTF-8 文本，只包含一个绝对可执行文件路径，可带 LF 或 CRLF 结尾；内容作为数据读取，不作为 shell 代码执行。
- 文件不存在时发出 warning，再尝试远端 PATH 中的 `mcpmd`；此时只有 fallback 也失败才返回 error。warning 会转交启动 link 的 mcpm，不受 `--remote-logs` 开关控制。
- 记录不可读、内容损坏、记录目标不存在或程序启动失败时直接报错，不静默换用另一个程序。
- 远端可执行文件缺失时，mcpm 会在错误后用英文 Tip 说明非交互 SSH 和 PATH 配置。mcpmd 缺失时额外说明安装位置记录，并建议在记录未生效时考虑重装；普通数据文件缺失和认证失败不显示这条 Tip。

### `mcpmd link list`

```text
mcpmd link list [--raw]
```

| 选项 | 说明 |
| --- | --- |
| `--raw` | 强制输出 TSV。 |

无响应的记录标记为 stale 并清理。

### `mcpmd link stop`

```text
mcpmd link stop (SSH | --all) [--force] [--mcpmd <PATH>]
```

| 参数或选项 | 说明 |
| --- | --- |
| `[SSH]` | SSH 连接命令，格式同 `--ssh`。省略时需 `--all`。 |
| `--all` | 停止全部 link。 |
| `--force` | 立即中止在途调用，不等待完成。 |
| `--mcpmd <PATH>` | 指定远端 mcpmd 路径；省略时按 SSH 命令匹配已登记的 link。 |

重编译不会替换已运行的 link。版本不兼容时停止对应 link，再用匹配构建重试。

[mcpm 命令](../../mcpm/docs-zh/README.md) · [Backend 模块](../../mcpm-backend/docs-zh/README.md) · [English](../docs/README.md)
