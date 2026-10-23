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
颜色和依赖路径样式由本地控制。远端错误始终显示；远端日志需要 `--remote-logs`，并遵循 `--log-level`。失败返回非零状态。
`--parallelism <COUNT>` 默认为 10，控制 `--all` 和名称/ID 查询建立目录索引时的
文件扫描并发，本地与 SSH 均适用。多个独立文件输入仍顺序处理，不受此参数影响。
每个扫描项完成后回传进度，包括失败和未识别文件；本地依据 stderr 是否为 TTY 显示进度。

两端均需协议版本 3。结构化 `mod.info` 请求见 [Remote](../remote.md)。

通用选项见 [Loader 选择](README.md#loader-选择)，颜色、忽略文件提示和 `--raw` 行为见[终端输出](../README.md#终端输出)。

## 识别缓存

- 默认查询 `~/.mcpm/cache/mods`。`--cache-dir PATH` 改用指定的已初始化目录；`--no-cache` 关闭缓存查询并直接识别文件；两者互斥。
- 默认目录不存在或为空时自动初始化；指定目录必须合法，可先运行 `mcpm cache mods init --directory PATH`。非空无标记目录、manifest 损坏或不支持的目录版本均失败。
- 先使用 imohash 候选索引和当前原生文件身份匹配硬链接；没有身份匹配时计算完整 BLAKE3，因此内容相同的独立副本也能复用。
- 已缓存的“未识别”也会复用。缺失、过期或无效结果正常识别。检查只读取已有缓存，不导入文件、不建立硬链接、不写回 metadata；存储结果可显式调用 [mod cache](cache.md)，或分别使用 cache import 和 refresh。
- Console 空一行后展示 `A fully cached · B partially cached · C total items`；list TUI 在 skipped 状态行下方展示同一统计。全部命中指所有请求 Loader 结果均来自缓存，包括“未识别”；部分命中指只有部分结果来自缓存。总数按规范化路径统计实际检查的独立文件，含未识别和失败项，筛选前计算；重复查询、嵌套声明不增加总数。关闭缓存时命中数均为 0，Raw 文本输出不添加统计行。
- SSH 模式下，缓存选择与路径在远端解释；普通缓存管理命令仍在本地执行。

```sh
mcpm mod info example.jar
mcpm mod info example.jar --no-cache
mcpm mod info example.jar --cache-dir /path/to/cache
```

[命令参考](../README.md) · [mod 概览](README.md) · [English](../../docs/mod/info.md)
