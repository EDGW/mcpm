# mcpm 命令参考

`mcpm` 用于检查本地 Minecraft 客户端和 Mod 归档。可用命令包括 `client list`、`client check`、`mod list`、`mod info`、`mod deps` 和 `mod dep-graph build`。Mod 归档只被读取，依赖图可另存为文件。

[客户端列表](client/list.md) · [完整性检查](client/check.md)

[Mod 命令](mod/README.md)：[list](mod/list.md) · [info](mod/info.md) · [deps](mod/deps.md) · [dep-graph](mod/dep-graph.md)

[English](../docs/README.md)

## 快速开始

```sh
mcpm client list "$HOME/Downloads/.minecraft"
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

标准输出被重定向或接入管道时，不显示颜色和终端专属提示，`list` 使用制表符分列，不进行列对齐和换行。`--raw` 在终端中也使用此行为，并禁用诊断信息的颜色及进度指示。普通诊断信息写入标准错误；`list` 只有在标准输出和标准错误都连接到终端时才显示进度指示。

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
