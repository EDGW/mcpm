# `mcpm-tools mods deps`

```text
mcpm-tools mods deps [OPTIONS] [INPUT]...
```

显示一个或多个 `.jar` 或 `.jar.disabled` 文件中，每个已识别 Mod 的直接依赖声明。文件按参数顺序处理，不接受目录。输出按外层文件、Loader 和 Mod 分组，包含 Mod 版本和声明所在归档的路径。

使用 `--path-style inner|full|outer` 调整 `Source` 路径格式，默认值为 `full`，详见[归档路径样式](README.md#归档路径样式)。

```sh
mcpm-tools mods deps example.jar
mcpm-tools mods deps example.jar another.jar --loader neoforge
mcpm-tools --raw mods deps example.jar > dependencies.txt
```

嵌套来源使用 `!` 分隔归档层级，保留外层文件名和各层内部归档成员路径。`list` 的嵌套 Mod 文件名列也使用此格式，例如：

```text
example.jar!META-INF/jars/child.jar!META-INF/jars/grandchild.jar
```

依赖关系保留 `required`、`optional`、`recommended`、`suggested`、`breaks` 和 `conflicts` 六类。每条声明只引用一个目标。版本条件使用括号，`&&` 表示全部版本条件，`||` 表示可选版本条件。`side`、`ordering` 和 `reason` 显示在方括号中。加载顺序描述声明者相对于目标的位置。

```text
required: render_api: (>=1 && <2)
optional: client_api: >=2 [side=client; ordering=after target]
breaks: legacy_api: <1
```

这些是依赖声明，尚未进行依赖解析。命令不会查找已安装的候选 Mod、判断版本兼容性或决定哪些嵌套候选项实际会被加载。目前未实现 `--tree` 和 `--dir`。

如果 Loader 识别失败，成功记录仍会保留，文件标记为 `Identification: incomplete` 并列出失败的 Loader ID。命令返回失败状态，并继续处理其他文件。没有 Loader 筛选时，无法识别的文件视为错误；有筛选时跳过。`deps` 目前不输出忽略文件的提示。

支持 Console 和 Raw 模式；使用 `--raw` 或重定向 stdout 进入 Raw。
SSH 同样使用这两种模式，由本地控制输出格式：

```sh
mcpm-tools --ssh 'ssh user@host' mods deps --directory /srv/mods --modid example
mcpm-tools --ssh 'ssh user@windows' --ssh-mcpm 'D:\mcpm-tools.exe' --raw mods deps --directory 'D:\mods' --all
```

输入依次按文件名、精确 Mod ID、精确名称匹配。可重复使用 `--file`、`--modid`、
`--name` 明确选择方式，或使用 `--all` 检查目录中的所有文件。
`--directory` 默认为 `.`。stdin 每行追加一个输入，`--input file|modid|name|unspecified`
决定其解释方式；`--all` 不能与查询混用。
SSH 模式在远端完成选择、文件读取和 Loader 解析，本地先读取 stdin，再发送结构化请求。
颜色和依赖路径样式由本地控制。远端错误始终显示；远端日志需要 `--remote-logs`，并遵循 `--log-level`。失败返回非零状态。
`--parallelism <COUNT>` 默认为 10，控制 `--all` 和名称/ID 查询建立目录索引时的
文件扫描并发，本地与 SSH 均适用。多个独立文件输入仍顺序处理，不受此参数影响。
每个扫描项完成后回传进度，包括失败和未识别文件；本地依据 stderr 是否为 TTY 显示进度。

两端均需协议版本 4。结构化 `mod.deps` 请求见 [Remote](../remote.md)。

通用选项见 [Loader 选择](README.md#loader-选择)，颜色和 `--raw` 行为见[终端输出](../README.md#终端输出)。

[命令参考](../README.md) · [mod 概览](README.md) · [English](../../docs/mod/deps.md)
