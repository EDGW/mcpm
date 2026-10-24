# `mcpm mod cache`

显式缓存选中的 Mod 完整归档及其识别摘要。

```text
mcpm mod cache [OPTIONS] [INPUT]...
```

支持 Console 和 Raw；重定向 stdout 或使用 `--raw` 时，每个选中的文件输出一行 JSON。
当前为本地命令，SSH 参数会明确报错。

## 选择文件

输入规则与 [mod info](info.md) 共用：

- 位置参数依次匹配文件名、精确 Mod ID、精确 Mod 名称。
- 可以重复、混合使用 `--file FILE`、`--modid ID`、`--name NAME`，按参数出现顺序查询。
- `--directory PATH` 指定源目录，默认 `.`；相对文件名在此目录解析，名称和 ID 在此目录查找。
- `--all` 选择源目录直属的全部 `.jar`、`.jar.disabled`，不能与其他查询或 stdin 输入混用。
- 重定向 stdin 每行追加一个查询；`--input file|modid|name|unspecified` 只控制 stdin。
  默认 `unspecified` 使用文件名、ID、名称的依次匹配规则。
- 即使命中嵌套 Mod，也缓存其完整外层 JAR，以及所选 Loader 的全部声明和嵌套识别树。
- 同一规范路径重复选中只处理一次，保留首次选中位置。内容相同的不同文件复用同一个 BLAKE3 对象。

```sh
mcpm mod cache example.jar another.jar.disabled
mcpm mod cache --directory ./mods --modid zeta
mcpm mod cache --directory ./mods --name 'Example Mod'
mcpm mod cache --directory ./mods --all --parallelism 4
printf '%s\n' example.jar another.jar | mcpm mod cache --input file
```

位置参数不接受目录；批量处理目录使用 `--directory PATH --all`。

## 存储和识别

| 选项 | 行为 |
| --- | --- |
| `--cache-dir PATH` | 目标缓存目录，与源 `--directory` 分开 |
| `--mode copy` | 默认模式，新增对象使用复制，不关联源文件 |
| `--mode hard-link` | 新增对象要求硬链接，源文件内容也会被写保护 |
| `--mode prefer-hard-link` | 优先硬链接，仅文件系统能力不支持时回退复制 |
| `--loader` / `--loaders` | 选择 Loader，默认全部内置 Loader |
| `--contents metadata,ident` | 默认两类：Loader metadata 和来源识别结果；可只选一类 |
| `--sources modrinth,curseforge` | 默认全部来源；可逗号分隔选择子集，支持 `curseforge@KEY` |
| `--force` | 强制重新查询所选 Loader／source 的已有结果 |
| `--parallelism COUNT` | 名称／ID 扫描和导入／识别阶段的最大并发文件数，默认 10 |

默认目标为 `~/.mcpm/cache/mods`，不存在或为空时初始化。
指定目录必须已经合法，可先运行 `mcpm cache mods init --directory PATH`。不自动迁移 version 1 缓存。

内容已经存在时直接复用，不替换缓存 identity，也不把另一个源文件连接到它。
导入模式只用于新增对象。硬链接共享内容和权限，修改时应替换文件名，不可原地覆盖、
截断或追加。并发发布竞争中，失败发布者已经应用的源文件写保护可能保留。

导入后从实际缓存字节识别，补全缺失或过期结果；当前版本已经检查的结果直接保留，
除非使用 `--force`。Loader 成功检查但未识别会明确写入摘要，属于成功；source 只保存验证通过的匹配，未匹配或错误不持久化。
ID／名称选择可能先进行目录识别；选择扫描得到的 metadata 不直接写入缓存。

```sh
mcpm cache mods init --directory /path/to/cache
mcpm mod cache example.jar --cache-dir /path/to/cache --loader fabric
mcpm mod cache example.jar --mode hard-link
mcpm mod cache example.jar --force
```

[cache mods import](../cache.md) 保留按路径导入、不识别的底层语义。
`mod info/list` 默认只读取已有缓存，`--no-cache` 可关闭；不保存文件或识别结果。

两类结果均依赖同一 `.dat`：Loader 使用 `.data.json`，来源匹配使用 `.identification.json`，
各自可以独立缺失或损坏。所有 CurseForge 配置共用缓存命名空间，key／endpoint 不进入缓存。
`--contents ident` 仍导入文件，但不会创建 Loader metadata。整批平台请求共用持续存活的客户端 runtime。

```sh
mcpm mod cache --directory ./mods --all --contents metadata
mcpm mod cache example.jar --contents ident --sources modrinth
```

## 结果与失败

- 按选择顺序输出结果，实际任务可以并发完成。
- 单文件失败继续后续文件，已完成的写入保留。
- 导入失败没有成功导入结果。metadata 失败可能留下已导入的 `.dat`，记录仍包含
  BLAKE3、是否本次发布及观察到的可用状态。
- 识别错误不会被写成“未识别”。
- 查询和选择阶段的目录扫描错误写入 stderr，计入失败；任一文件或选择失败导致非零退出。
- Console 默认显示进度条和最终汇总；`--expanded`（`-E`）才展示逐文件源路径、
  BLAKE3、新增／复用、实际传输方式及摘要状态。错误始终显示。
  汇总前空行，汇总全部小写，区分文件数量和唯一缓存对象数。
  本次新增过的对象只计新增；文件失败和选择／发现阶段失败分别计数，同一文件可能在两个阶段失败。
- Raw 每个文件一行 JSON，字段为 `path`、`id`、`published`、`method`、`status`、
  `reason`、`data_path`、`digest_path`、`metadata_requested`、`identification`、`success`、`failed_stage`、`error`、`error_remote`。
  `identification` 为各来源的 matched／unmatched／failed 及独立错误与来源。
  复用时 method 为 null，导入失败时没有 ID；Raw 不添加汇总行。

缓存允许漏写或丢失并发摘要更新，但作为可用结果返回的部分必须正确；不提供事务式备份保证。


对于已登记的 Local、Remote 服务器，可使用 [`server mods cache`](../server/mods.md#缓存) 自动采用服务器 mods 范围和声明的 loader。

[命令参考](../README.md) · [English](../../docs/mod/cache.md)
