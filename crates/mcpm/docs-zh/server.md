# Server 命令族

入口：

| 入口 | 命令格式 |
| --- | --- |
| `mcpm` | `mcpm <命令>` |
| `mcpm-server` | `mcpm-server <命令>` |
| `mcpm-tools` | `mcpm-tools server <命令>` |

以下以 `mcpm` 展示。通用术语、全局选项和输出模式见 [README](README.md)。

## `mcpm info`

```text
mcpm info [OPTIONS] [PATH]
```

### 参数

| 参数 | 说明 |
| --- | --- |
| `[PATH]` | 服务端目录。默认 `.`，即当前目录。未指定 `--ssh` 且 PATH 是已登记的 Remote workspace 时，改为使用登记中的远端服务端。 |

### 选项

| 选项 | 说明 |
| --- | --- |
| `--full-command` | 展开旧登记中的历史启动命令。不解除 SSH 脱敏；launch 不使用该快照。 |

### 输出

Console 显示名称、路径、Minecraft 版本、Loader 和版本，以及服务端所属主机上 `launch.toml`、`conf.toml` 是否存在。Remote workspace 额外显示远端路径、脱敏 SSH 和 mcpmd 路径。

Raw 为 TSV，无表头，字段依次为：

```text
名称 路径 Minecraft版本 Loader Loader版本 历史命令
```

Remote workspace 在字段前增加远端路径、脱敏 SSH、mcpmd 路径。历史命令缺失时为 `unavailable`。登记缺失或损坏时返回错误。

### 示例

```sh
mcpm info ./server
mcpm info ./workspace
mcpm --ssh 'ssh user@host' info /srv/minecraft
mcpm info ./server --full-command
```

## `mcpm init`

```text
mcpm init [OPTIONS] [PATH]
```

### 参数

| 参数 | 说明 |
| --- | --- |
| `[PATH]` | 已存在的目录。默认 `.`。通常为服务端目录；带 `--remote` 时为本机 Remote 工作区目录；显式 `--ssh` 且不带 `--remote` 时为远端服务端目录。 |

### 选项

| 选项 | 说明 |
| --- | --- |
| `--remote <REMOTE_PATH>` | 将 PATH 登记为 Remote 工作区，指向已有远端 Local Server。要求 `--ssh`。与 `--start-script`、手动身份选项、`--foverride-conf` 互斥。 |
| `--start-script <FILE>` | 静态识别服务端目录内的启动脚本，使用相对路径。与手动身份选项互斥。 |
| `--minecraft-version <VERSION>` | Minecraft 版本。别名 `--mc-version`。 |
| `--loader <LOADER>` | Loader：`fabric`、`forge`、`neoforge` 或 `vanilla`。 |
| `--loader-version <VERSION>` | Loader 版本，不是 Minecraft 或 FML 版本。 |
| `--force` | 服务端初始化时替换已有登记和 `launch.toml`；`--remote` 时替换已有连接登记。默认保留已有 `conf.toml`。 |
| `--foverride-conf` | 同时重新生成 `conf.toml`，覆盖已有设置和注释。要求 `--force`，与 `--remote` 互斥。 |
| `--no-git` | 跳过服务端所属主机上的 Git 检查与创建。与 `--git` 互斥。 |
| `--git` | 要求实际服务端目录自身具有 Git 仓库。与 `--no-git` 互斥。 |
| `--no-ui` | 禁用候选选择 TUI，使用 Console。 |
| `--full-command` | 展开结果中的历史启动命令。 |

### 路径与执行位置

| 场景 | PATH | 实际写入目标 |
| --- | --- | --- |
| 无 `--ssh`，PATH 为本机 Local 或未登记目录 | 本机服务端目录 | 本机服务端 |
| 无 `--ssh`，PATH 为已登记 Remote workspace | 本机工作区目录 | 登记中的远端服务端 |
| 有 `--ssh`，无 `--remote` | 远端服务端目录 | 指定远端服务端 |
| 有 `--ssh`，有 `--remote REMOTE_PATH` | 本机工作区目录 | 本机连接登记；REMOTE_PATH 为已有远端 Local Server |

实际目标必须是 Local 服务端；Remote workspace 不能再指向另一个 Remote workspace。

### 初始化方式

#### 从启动脚本识别

```sh
mcpm init ./server
mcpm init ./server --start-script scripts/start.sh --no-ui
```

未指定 `--start-script` 时，使用服务端所属主机上的默认脚本：Windows 为 `run.bat`，其他平台为 `run.sh`。脚本路径必须位于服务端目录内。

识别静态读取脚本和身份依据，不执行脚本。动态脚本或证据不足时可能失败，此时应提供完整手动身份。

候选包含初始化所需的配置草稿。选定候选后才进行 Git 准备和配置写入；仅识别或取消选择不会写入。

#### 完整手动身份

```sh
mcpm init ./server --minecraft-version 1.21.1 --loader vanilla
mcpm init ./server --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
```

| 组合 | 要求 |
| --- | --- |
| 任意非 vanilla Loader | 必须同时提供 `--minecraft-version` 和 `--loader-version` |
| vanilla | 必须显式 `--loader vanilla`，禁止 `--loader-version` |

完整手动身份不读取启动脚本，也不生成 `launch.toml`。需要生成启动配置时，应通过可识别的启动脚本初始化。

#### 部分身份筛选候选

```sh
mcpm init ./server --loader forge --no-ui
mcpm init ./server --minecraft-version 1.20.1 --no-ui
```

不完整身份仍执行识别；已给字段只筛选识别出的候选。无匹配候选时失败；Console 或 Raw 下多候选也失败。

#### 候选选择

| 候选数 | 行为 |
| --- | --- |
| 1 | 直接使用，不打开 TUI |
| 多个，默认模式 | 本机 TUI：上下键移动，Enter 确认，Esc 或 Ctrl-C 取消 |
| 多个，`--no-ui` 或 `--raw` | 报错，要求更具体的识别参数或完整手动身份 |

TUI 要求本机 stdout/stderr 为终端。远端初始化时，选择界面仍在本机。

### 写入的文件

| 文件 | 内容 |
| --- | --- |
| `.mcpm/server.json` | 服务端身份登记 |
| `.mcpm/mcpm.json` | Local 工作区类型 |
| `.mcpm/.gitignore` | 元数据忽略规则 |
| `launch.toml` | 从候选草稿导入的启动配置；手动身份不生成 |
| `conf.toml` | 用户设置 |

已有 `conf.toml` 默认保留。新启动配置与保留的 `conf.toml` 冲突时，发布失败。

### 重复初始化与覆盖

```sh
# 替换登记和启动配置，保留已有 conf.toml
mcpm init ./server --start-script run.sh --force

# 同时重新生成 conf.toml
mcpm init ./server --start-script run.sh --force --foverride-conf
```

| 情况 | 行为 |
| --- | --- |
| 已有登记或 `launch.toml`，无 `--force` | 失败 |
| 有 `--force`，通过启动脚本初始化 | 替换登记和 `launch.toml`，保留 `conf.toml` |
| 有 `--force`，手动身份 | 替换登记；不生成 `launch.toml`，已有 `launch.toml` 被移除 |
| 有 `--force --foverride-conf` | 同时替换 `conf.toml` |
| 已有 Local/Remote 类型 | 不允许用 `--force` 转换类型 |
| `.mcpm/server.json` 是目录 | 拒绝替换 |

发布、类型标记或连接在中途失败时，部分文件可能已经写入。重试前先检查实际文件。

### Git

| 选项 | 行为 |
| --- | --- |
| 默认 | 复用目标仓库、父目录仓库或 worktree；均不存在时在服务端目录创建仓库 |
| `--git` | 要求服务端目录自身具有仓库；已有仓库直接复用 |
| `--no-git` | 跳过检查和创建 |

不自动暂存或提交。未使用 `--no-git` 时，服务端所属主机必须安装 Git；仓库损坏或 Git 执行失败时返回错误。`--force` 不跳过 Git。

### 远端初始化

```sh
mcpm --ssh 'ssh user@host' init /srv/minecraft \
  --minecraft-version 1.21.1 --loader vanilla --no-git
```

PATH 是远端服务端目录。识别、Git、服务端登记和配置发布均在远端完成。

### Remote 工作区

```sh
# 为已有远端服务端创建本机入口
mcpm --ssh 'ssh user@host' init ./workspace --remote /srv/minecraft --git

# 替换本机保存的连接和路径
mcpm --ssh 'ssh user@new-host' init ./workspace --remote /srv/new-server --force --no-git

# 沿用本机入口，重新初始化远端服务端
mcpm init ./workspace --force --minecraft-version 1.21.1 --loader vanilla --no-git
```

| 调用 | 作用 |
| --- | --- |
| `init ./workspace --remote REMOTE_PATH` | 校验远端 Local Server，完成 Git 准备，再在本机写入连接登记 |
| 不带 `--remote`，PATH 为 Remote workspace | 对登记中的远端服务端执行初始化 |
| 已有 Remote 登记，无 `--force` | 拒绝替换连接 |
| 本机目录已是 Local 工作区 | 不能转换为 Remote |

创建 Remote 工作区时，Git 操作针对远端服务端目录，本机工作区不创建 Git 仓库。

本机工作区只保存 SSH 命令、可选 mcpmd 路径和远端目录。远端服务端必须已登记为 Local Server；未登记时先执行远端初始化。

## `mcpmd link`

### 前台 link

```text
mcpmd link [--mcpmd <PATH>] [--idle-timeout <SECONDS>] SSH
```

| 参数或选项 | 说明 |
| --- | --- |
| `[SSH]` | SSH 连接命令，格式同 `--ssh`。 |
| `--mcpmd <PATH>` | 远端 mcpmd 可执行文件；省略时使用远端环境变量 PATH 中的 `mcpmd`。 |
| `--idle-timeout <SECONDS>` | 无客户端且无在途调用时，经过该秒数后退出；`0` 表示不按空闲退出。默认 `600`。 |

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

[Backend Server](../../mcpm-backend/docs-zh/server.md) · [Workspace](../../mcpm-backend/docs-zh/workspace.md) · [尚未迁移命令](legacy/README.md)
