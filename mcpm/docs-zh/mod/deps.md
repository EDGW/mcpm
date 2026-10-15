# `mcpm mod deps`

```text
mcpm mod deps [OPTIONS] <FILE>...
```

显示一个或多个 `.jar` 或 `.jar.disabled` 文件中，每个已识别 Mod 的直接依赖声明。文件按参数顺序处理，不接受目录。输出按外层文件、Loader 和 Mod 分组，包含 Mod 版本和声明所在归档的路径。

使用 `--path-style inner|full|outer` 调整 `Source` 路径格式，默认值为 `full`，详见[归档路径样式](README.md#归档路径样式)。

```sh
mcpm mod deps example.jar
mcpm mod deps example.jar another.jar --loader neoforge
mcpm --raw mod deps example.jar > dependencies.txt
```

嵌套来源使用 `!` 分隔归档层级，保留外层文件名和各层内部归档成员路径。`list` 的嵌套 Mod 文件名列也使用此格式，例如：

```text
example.jar!META-INF/jars/child.jar!META-INF/jars/grandchild.jar
```

依赖关系保留 `required`、`optional`、`recommended`、`suggested`、`breaks` 和 `conflicts` 六类。条件组使用括号，`&&` 表示全部条件，`||` 表示可选条件。引用自身的规则，例如 `if present`、`unless`、`side`、`ordering` 和 `reason`，显示在方括号中。加载顺序描述声明者相对于目标的位置。

```text
required: (render_api: >=1 || alternate_api: *)
optional: client_api: >=2 [side=client; ordering=after target]
breaks: legacy_api: <1 [unless compatibility: >=2]
```

这些是依赖声明，尚未进行依赖解析。命令不会查找已安装的候选 Mod、判断版本兼容性或决定哪些嵌套候选项实际会被加载。目前未实现 `--tree` 和 `--dir`。

如果 Loader 识别失败，成功记录仍会保留，文件标记为 `Identification: incomplete` 并列出失败的 Loader ID。命令返回失败状态，并继续处理其他文件。没有 Loader 筛选时，无法识别的文件视为错误；有筛选时跳过。`deps` 目前不输出忽略文件的提示。

通用选项见 [Loader 选择](README.md#loader-选择)，颜色和 `--raw` 行为见[终端输出](../README.md#终端输出)。

[命令参考](../README.md) · [mod 概览](README.md) · [English](../../docs/mod/deps.md)
