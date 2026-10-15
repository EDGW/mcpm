# mcpm 命令参考

`mcpm` 用于检查和启动本地 Minecraft 客户端、检查 Mod 归档。可用命令包括 `client list`、`client check`、`client launch`、`mod list`、`mod info`、`mod deps` 和 `mod dep-graph build`。Mod 归档只被读取，依赖图可另存为文件。

[客户端列表](client/list.md) · [完整性检查](client/check.md) · [启动客户端](client/launch.md)

[Mod 命令](mod/README.md)：[list](mod/list.md) · [info](mod/info.md) · [deps](mod/deps.md) · [dep-graph](mod/dep-graph.md)

[English](../docs/README.md)

## 快速开始

```sh
mcpm client list "$HOME/Downloads/.minecraft"
mcpm client launch 1.18.1 --directory "$HOME/Downloads/.minecraft" --java /path/to/java
mcpm mod list
mcpm mod list mods/ example.jar other-mods/
mcpm mod info example.jar another.jar.disabled
mcpm mod deps example.jar --loader fabric
mcpm --raw mod list mods/ -C MF > mods.tsv
```

在仓库中直接运行时，将 `mcpm` 替换为 `cargo run --manifest-path mcpm/Cargo.toml --`。

## 全局选项

```text
mcpm [OPTIONS] <COMMAND>
```

| 选项 | 说明 |
| --- | --- |
| `--raw` | 将输出流视为非终端：禁用颜色、终端排版、进度指示和仅终端可见的提示。 |
| `-h`、`--help` | 显示当前命令的帮助。 |
| `-V`、`--version` | 在顶层显示程序版本。 |

`--raw` 是全局参数，可以放在子命令之前或之后：

```sh
mcpm --raw mod info example.jar
mcpm mod list mods/ --raw
```

使用各命令的帮助查看可用参数：

```sh
mcpm --help
mcpm client list --help
mcpm client check --help
mcpm client launch --help
mcpm mod list --help
mcpm mod info --help
mcpm mod deps --help
mcpm mod dep-graph build --help
mcpm mod dep-graph test --help
mcpm mod dep-graph view --help
```

## 终端输出

当标准输出连接到终端时，`list` 根据终端宽度对齐列并换行。`info` 和 `deps` 高亮 Mod 身份信息和依赖目标，路径与补充信息使用灰色。依赖标签的颜色分别为：`required` 绿色、`recommended` 蓝色、`suggested` 亮白色、`optional` 灰色、`breaks` 红色、`conflicts` 黄色。

当 Loader 筛选跳过文件时，`list` 和 `info` 会在输出结束后空一行显示忽略文件的提示。`list --expanded` 还会列出被跳过的文件名。这些提示仅在标准输出连接到终端时显示，使用 `--raw` 时隐藏。

所有命令和诊断使用统一终端策略：显式传入 `--raw`，或标准输入、标准输出、标准错误中任一个不是 TTY 时，自动进入 raw 模式，禁用全部 TUI 交互、颜色、终端排版、进度指示和终端专属提示。`list` 使用制表符分列，不对齐和换行。客户端命令需要显式指定版本 ID，或对 `check` 使用 `--all`。

设置 `NO_COLOR` 可禁用颜色，同时保留终端排版和提示：

```sh
NO_COLOR=1 mcpm mod list mods/
```

## 退出状态

| 状态码 | 含义 |
| --- | --- |
| `0` | 成功完成，包括空结果或因 Loader 筛选而跳过文件。帮助和版本请求也返回成功。 |
| `1` | 发生操作错误，例如路径无效、归档不可读、识别失败或输出失败。可能已经输出部分结果。 |
| `2` | 命令行解析失败，例如未知选项、缺少必需参数或选项值无效。 |

## 诊断日志

全局 `--log-level` 使用 `tracing-subscriber` 控制日志，默认 `warn`。
`info` 显示启动/检查关键步骤，`debug` 增加库选择、路径和最终脱敏 Java 参数列表，
`trace` 增加每个 native 条目和必需文件检查的详情。

```sh
mcpm --log-level info client launch 1.18.1 --no-logging-config
mcpm client launch 1.18.1 --no-logging-config --log-level debug
```

日志写入 stderr，并与进度条协调显示。颜色遵循终端状态、`NO_COLOR` 和 `--raw`。
日志级别不控制普通命令结果、操作失败诊断或游戏 stdout/stderr。
启动命令日志对身份 token 和已知凭据参数脱敏，不记录环境变量的值。
