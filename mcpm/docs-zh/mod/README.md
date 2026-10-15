# `mcpm mod`

检查本地 Minecraft Mod 归档，只读取文件。

```text
mcpm mod <COMMAND>
```

| 命令 | 说明 |
| --- | --- |
| [`list`](list.md) | 列出文件和目录中的 Mod 声明。 |
| [`info`](info.md) | 检查一个或多个文件的元数据和依赖。 |
| [`deps`](deps.md) | 显示直接依赖声明及其来源归档。 |
| [`dep-graph`](dep-graph.md) | 构建目录依赖图、校验快照，或查看单个 Mod 的依赖树。 |

## Loader 选择

`list`、`info`、`deps` 和 `dep-graph build` 都支持以下选项：

| 选项 | 说明 |
| --- | --- |
| `--loader <LOADER>` | 选择一个 Loader，可重复指定。 |
| `--loaders <LOADERS>` | 选择以逗号分隔的多个 Loader，可重复指定或与 `--loader` 组合使用。 |

可用值为 `forge`、`neoforge` 和 `fabric`。不指定时尝试全部三个 Loader。多个选择取并集并去重，记录按内置顺序显示：Forge、NeoForge、Fabric。

```sh
mcpm mod list mods/ --loader fabric --loader neoforge
mcpm mod info example.jar --loader forge --loaders neoforge,fabric
```

同一个归档可以被多个 Loader 识别。即使 Mod ID 相同，各 Loader 的记录仍分别保留。识别成功表示能够读取元数据，并不保证该 Mod 能在对应 Loader 中运行。

## 归档路径样式

`mod list` 和 `mod deps` 支持 `--path-style <STYLE>`。以下以 `mods/example.jar!META-INF/jars/child.jar!lib/grandchild.jar` 中的嵌套 Mod 为例：

| 样式 | 显示内容 |
| --- | --- |
| `inner` | `grandchild.jar` |
| `full`（默认） | `example.jar!META-INF/jars/child.jar!lib/grandchild.jar` |
| `outer` | `example.jar` |

此选项控制 `list` 中嵌套归档的文件名和 `deps` 中全部 `Source` 值。`list` 中的普通外层声明始终显示外层文件名。它只改变路径格式，内部 Mod 声明仍然包含在结果中。

对于嵌套声明，最外层仅显示 JAR 文件名，不包含文件系统目录；内部各层保留归档成员路径。对于 `deps` 中的外层声明，`full` 保留传入的文件路径。`mod info` 没有归档来源字段，不支持此选项。

```sh
mcpm mod list mods/ --expanded --path-style inner
mcpm mod list mods/ --expanded --path-style full
mcpm mod deps example.jar --path-style outer
```

彩色终端输出中，归档分隔符 `!` 使用亮白色，路径使用灰色。`--raw` 和重定向输出保留相同的路径文本，不显示颜色。

[命令参考](../README.md) · [English](../../docs/mod/README.md)
