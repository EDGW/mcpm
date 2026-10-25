# mcpm 命令参考

所有入口共享同一套实现和配置：

| 可执行文件 | 命令 |
| --- | --- |
| `mcpm`、`mcpm-server` | `init`、`info`、`mods`、`stash` |
| `mcpmc`、`mcpm-client` | `list`、`check`、`launch` |
| `mcpm-tools` | `cache`、`remote`、`customize`、`debug`、`mods`、`server`、`client` |

长短名称的行为相同，帮助和命令提示使用所选名称。Tools 也保留 `mcpm-tools server info`、`mcpm-tools client list`。服务端和客户端入口只接受各自命令族；远程会话使用 `mcpm-tools remote`。

检查命令只读取 Mod 归档；`mcpm-tools mods cache` 显式存储文件与摘要，可选择硬链接保护。依赖图可另存为文件。

[客户端列表](client/list.md) · [完整性检查](client/check.md) · [启动客户端](client/launch.md)

[初始化服务端](server/init.md) · [服务端信息](server/info.md) · [服务端 Mod 来源](server/source.md)

[服务端 Mod 快照与切换](server/stash.md)

[Mod 命令](mod/README.md)：[list](mod/list.md) · [info](mod/info.md) · [cache](mod/cache.md) · [deps](mod/deps.md) · [dep-graph](mod/dep-graph.md)

[自定义界面](customize.md) · [调试调色板](debug/palette.md)

[持久远程会话](remote.md)

[Mod 缓存管理](cache.md) · [Mod 文件备份](mod/backup.md)

[English](../docs/README.md)

## 快速开始

```sh
mcpmc list "$HOME/Downloads/.minecraft"
mcpmc launch 1.18.1 --directory "$HOME/Downloads/.minecraft" --java /path/to/java
mcpm-tools mods list
mcpm-tools mods list mods/ example.jar other-mods/
mcpm-tools mods info example.jar another.jar.disabled
mcpm-tools mods deps example.jar --loader fabric
mcpm-tools --raw mods list mods/ -C MF > mods.tsv
```

使用 `cargo build --manifest-path mcpm/Cargo.toml --release --bins` 构建全部五个程序。
`cargo run --manifest-path mcpm/Cargo.toml -- info` 默认启动 `mcpm`。
其他入口使用 `--bin` 选择：

```sh
cargo run --manifest-path mcpm/Cargo.toml --bin mcpmc -- list
cargo run --manifest-path mcpm/Cargo.toml --bin mcpm-tools -- mods list
```

## 全局选项

`--ssh`、`--ssh-mcpm`、`--remote-logs` 仅在服务端和 Tools 入口提供。SSH 默认调用远端的 `mcpm-tools`；显式配置的 `--ssh-mcpm` 路径保持原样。`--ssh` 和 `--ssh-mcpm` 为顶层全局参数，`mods list`、`mods info`、`mods deps`、`mods source identify`、`server init`、`server info` 和 `server mods list/info/cache/deps/source search/files/identify` 支持使用。详见 [Remote 会话](remote.md)。

```text
mcpm-tools [OPTIONS] <COMMAND>
```

| 选项 | 说明 |
| --- | --- |
| `--raw` | 将输出流视为非终端：禁用颜色、终端排版、进度指示和仅终端可见的提示。 |
| `--remote-logs` | 在 stderr 显示远端日志，默认隐藏，并遵循 `--log-level`；远端错误始终显示。 |
| `-h`、`--help` | 显示当前命令的帮助。 |
| `-V` | 在顶层显示程序版本。 |
| `--version` | 显示版本、构建身份（`official` / `unofficial`）、构建配置、Git commit ID 和构建时记录的贡献者（提交数、姓名、邮箱）；无法读取 Git 信息时标明对应信息不可用。 |

详细信息展示完整 Commit ID；受跟踪文件存在已暂存或未暂存的变更时追加 ` + modified`，未跟踪文件不计入。信息在构建时记录，不受运行目录影响。

非正式构建的版本号追加 `.<commit 前六位>`；有受跟踪文件变更时使用 `.<commit 前六位>.modified`；无法读取 Git 信息时追加 `.unknown`。`-V` 与 `--version` 使用同一版本号，官方构建保留 Cargo 包版本。

非正式发布的 release 构建在执行命令时向 stderr 提示：该程序可能来自第三方构建，或包含尚未正式发布的改动。提示不改变命令结果和退出状态，`--raw` 和 `--log-level off` 不会隐藏它。Debug 构建不显示启动提示，`-V` 仅显示程序名称和版本。

`--raw` 是全局参数，可以放在子命令之前或之后：

```sh
mcpm-tools --raw mods info example.jar
mcpm-tools mods list mods/ --raw
```

使用各命令的帮助查看可用参数：

```sh
mcpm --help
mcpmc list --help
mcpmc check --help
mcpmc launch --help
mcpm-tools mods list --help
mcpm-tools mods info --help
mcpm-tools mods cache --help
mcpm-tools mods deps --help
mcpm-tools mods dep-graph build --help
mcpm-tools mods dep-graph test --help
mcpm-tools mods dep-graph view --help
```

## 终端输出

当标准输出连接到终端时，`list` 根据终端宽度对齐列并换行。Mod 名等关键内容使用主色加粗，二级信息使用附色，路径和补充说明使用白色弱化；警告、成功、错误分别使用亮黄、亮绿、亮红。Loader 使用配置中的专属颜色。通过 [`mcpm-tools customize`](customize.md) 调整主题。依赖类型 `required` 使用白色加粗，`recommended` 使用附色，`suggested` 使用普通白色，`optional` 使用弱化文字，`breaks` 使用错误色，`conflicts` 使用警告色。

当 Loader 筛选跳过文件时，`list` 和 `info` 会在输出结束后空一行显示忽略文件的提示。`list --expanded` 还会列出被跳过的文件名。这些提示仅在标准输出连接到终端时显示，使用 `--raw` 时隐藏。

所有命令和诊断使用统一终端策略：显式传入 `--raw` 时禁用样式与交互；stdout 非 TTY 时，命令结果自动使用 Raw 格式。stderr 独立控制诊断颜色与进度显示，stdin 不参与模式检测。Raw 列表使用制表符分列，不对齐和换行；客户端命令需要显式指定版本 ID，或对 `check` 使用 `--all`。

设置 `NO_COLOR` 可禁用颜色，同时保留终端排版和提示：

```sh
NO_COLOR=1 mcpm-tools mods list mods/
```

## 退出状态

| 状态码 | 含义 |
| --- | --- |
| `0` | 成功完成，包括空结果或因 Loader 筛选而跳过文件。帮助和版本请求也返回成功。 |
| `1` | 发生操作错误，例如路径无效、归档不可读、识别失败或输出失败。可能已经输出部分结果。 |
| `2` | 命令行解析失败，例如未知选项、缺少必需参数或选项值无效。 |

## 诊断日志

全局 `--log-level` 使用 `tracing-subscriber` 控制 stderr 日志，默认 `warn`。
`info` 显示当前步骤及游戏进程启动、退出等重要事件；普通完成提示、会话清理、传输统计、
库选择、路径和最终脱敏 Java 参数列表使用 `debug`，
`trace` 增加每个 native 条目和必需文件检查的详情。

```sh
mcpmc --log-level info launch 1.18.1 --no-logging-config
mcpmc launch 1.18.1 --no-logging-config --log-level debug
```

日志写入 stderr，并与进度条协调显示。颜色遵循终端状态、`NO_COLOR` 和 `--raw`。
日志级别不控制普通命令结果、操作失败诊断或游戏 stdout/stderr。
同时，每次运行会自动创建 `~/.mcpm/logs/YYYY-MM-DD/HH-MM-SS.nnnnnnnnn.log`，
日期和时间取程序启动时的本地时间，文件名兼容 Windows。同一时刻启动的进程在文件名冲突时追加 UUID，避免覆盖；跨午夜后继续写入同一文件。
文件固定记录到 `debug`（包括 error、warn、info、debug），独立于 `--log-level`；
即使 `--log-level off` 也会保留文件日志。文件包含时间戳、模块和错误链，不含颜色。
远端日志默认不在 stderr 显示，添加全局参数 `--remote-logs` 后才显示，并遵循 `--log-level`。远端操作错误始终以 `remote error:` 显示，不受这两个选项影响。文件日志和失败退出码不受此开关影响。
Remote 进程同样在远端用户的主目录保存日志。
启动命令日志对身份 token 和已知凭据参数脱敏，不记录环境变量的值。

[服务器 Mod 检查、列表与缓存](server/mods.md)
