# `mcpm mod info`

```text
mcpm mod info [OPTIONS] [INPUT]...
```

按照参数顺序检查一个或多个 `.jar` 或 `.jar.disabled` 文件，不接受目录。输出包括外层文件路径、根据文件名推断的启用状态、Loader 记录、Mod 名称、ID、版本和依赖声明，也包含已识别的嵌套 Mod。

```sh
mcpm mod info example.jar
mcpm mod info example.jar another.jar.disabled --loaders fabric,neoforge
mcpm --raw mod info example.jar > info.txt
```

未指定 Loader 筛选时，所有 Loader 都无法识别的文件视为错误；指定筛选时，此类文件会被跳过。Loader 识别错误写入标准错误，成功的记录仍会显示，但命令返回失败状态。后续文件继续处理。

支持 Console 和 Raw 模式；使用 `--raw` 或重定向 stdout 进入 Raw。
SSH 同样使用这两种模式，由本地控制输出格式：

```sh
mcpm --ssh 'ssh user@host' mod info --directory /srv/mods --modid example
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'D:\mcpm.exe' --raw mod info --directory 'D:\mods' --all
```

输入依次按文件名、精确 Mod ID、精确名称匹配。可重复使用 `--file`、`--modid`、
`--name` 明确选择方式，或使用 `--all` 检查目录中的所有文件。
`--directory` 默认为 `.`。stdin 每行追加一个输入，`--input file|modid|name|unspecified`
决定其解释方式；`--all` 不能与查询混用。
SSH 模式在远端完成选择、文件读取和 Loader 解析，本地先读取 stdin，再发送结构化请求。
颜色和依赖路径样式由本地控制。远端诊断遵循 `--remote-logs`，失败仍返回非零状态。
`--parallelism <COUNT>` 默认为 10，控制 `--all` 和名称/ID 查询建立目录索引时的
文件扫描并发，本地与 SSH 均适用。多个独立文件输入仍顺序处理，不受此参数影响。
每个扫描项完成后回传进度，包括失败和未识别文件；本地依据 stderr 是否为 TTY 显示进度。

两端均需协议版本 3。结构化 `mod.info` 请求见 [Remote](../remote.md)。

通用选项见 [Loader 选择](README.md#loader-选择)，颜色、忽略文件提示和 `--raw` 行为见[终端输出](../README.md#终端输出)。

[命令参考](../README.md) · [mod 概览](README.md) · [English](../../docs/mod/info.md)
