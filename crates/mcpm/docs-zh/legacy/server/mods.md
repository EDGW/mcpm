> 尚未迁移到新文档体系。此处保留旧文档；已迁移命令以文档根目录中的新版本为准。

# 服务器 Mod 检查、摘要与缓存

服务器 Mod 命令使用已登记服务器的 loader 和 mods 目录。本地服务器、显式 SSH 连接和已保存的 Remote 工作区遵循相同行为。

- `--server PATH` 选择服务器，默认 `.`。
- ITEM 支持文件名、精确 Mod ID、精确名称，以及 `--file`、`--modid`、`--name`、`--all`。stdin 每行追加一项并自动匹配；info/deps/cache/digest 还支持 `--input file|modid|name|unspecified`。
- 文件输入相对于服务器 mods 目录，接受 `mods/` 前缀，拒绝父路径穿越和绝对文件路径。
- SSH 模式下，服务器元数据、文件选择和缓存路径均在远端解析，输出在本地展示。Remote 工作区自动使用已保存的连接。
- 缓存资源取自 Server 的运行环境。当前各缓存用途共享文件主机的 global 缓存（`~/.mcpm/cache/mods`），暂未提供 Server 专属缓存设置。
- 远端错误始终显示；远端日志需要 `--remote-logs`，并遵循 `--log-level`。

## 摘要

```sh
mcpm mods digest example.jar --server /srv/minecraft
mcpm mods digest create --murmur2
mcpm mods digest 'Create' --headers --raw
mcpm mods digest --all --parallelism 4
printf '%s\n' create jei | mcpm mods digest --input modid
```

- 与 `mods info XXX` 一样，位置参数按文件名、精确 Mod ID、精确名称依次匹配；也可组合显式选择和 stdin。空输入不扫描目录，完整范围需使用 `--all`。
- 始终计算整个外层文件的大小和 SHA-1；`--murmur2` 追加 CurseForge 兼容指纹。嵌套 Mod ID/name 命中时，摘要仍属于外层归档。
- 文件选择与 `--all` 不解析 Mod 元数据，因此损坏或无法识别的 JAR 也能计算摘要；ID/name 选择才使用服务器声明的 loader 建立索引。`--all` 非递归枚举 `.jar` 和 `.jar.disabled`，按路径排序。
- 重复选择或文件路径别名只输出一次，保留首次命中的顺序。`--parallelism` 为正整数，默认 10，约束选择索引和摘要读取。不访问 Mod 缓存，不查询来源服务，不上传文件。
- 支持 Console 字段组和 Raw TSV。Raw 列固定为 `file`、`size`、`sha1`、`murmur2`；未请求的指纹和失败文件的摘要字段留空，`--headers` 在首条结果前输出列名。
- 选择索引扫描和摘要计算分别显示 stderr 进度条；摘要进度按已完成的输入位置推进，包含失败项，不等待有序结果输出。独立文件入口也显示摘要进度。仅 stderr 为终端时显示，重定向 stdout 不关闭进度；`--raw` 始终关闭。完成或出错退出前清理进度条。
- 选择或单文件失败后继续输出有效结果；任何失败均返回非零。选择索引扫描中的文件错误也会计为失败，即使该文件最终未被选中。选择错误写 stderr，读取失败的文件保留空摘要行。
- 支持显式 `--ssh` 和已登记 Remote workspace，通过现有 `mod.digest` operation 在文件主机执行，远端使用与本机匹配的 `mcpmd`。`mcpm-server mods digest` 和 `mcpm-tools server mods digest` 使用相同入口。

独立的 `mcpm-tools mods digest FILE...` 仍将参数和 stdin 解释为字面文件路径，不进行服务器选择；重复路径共享读取但保留每个输出位置。

## Info 与依赖

```sh
mcpm mods info --server /srv/minecraft --all --parallelism 4
mcpm --ssh 'ssh user@host' mods info --server /srv/minecraft --modid create
mcpm mods deps --server /srv/minecraft --modid create
```

两个命令均支持 Console 和 Raw。`--parallelism` 默认 10，约束完整扫描和 modid/name 延迟索引；多个独立文件输入仍顺序处理。

Info 默认查询 Server 运行环境的 Mod 缓存：

- `--no-cache` 禁用本次操作的缓存访问。
- 命中后复用已有 loader 结果，缺失结果直接解析；查询不会导入文件或写入元数据。
- Console 显示全部缓存、部分缓存、总项目数，使用小写英文，统计前空一行。

Deps 保持直接识别，不查询持久缓存。

## 依赖图与检查

```sh
mcpm mods dep-graph build --server /srv/minecraft -o graph.json
mcpm mods dep-graph view -i graph.json --duplicate-primary
mcpm mods dep-graph view -i graph.json --problem
mcpm mods dep-graph view -i graph.json --problem --no-duplicate-primary
mcpm mods check --server /srv/minecraft
```

- Server `dep-graph view` 和独立 Mod 图共用筛选参数，详见[重复 Primary 与问题视图](../mod/dep-graph.md#重复-primary-与问题视图)。
- `check` 使用同一后端问题视图，同时展示依赖问题和同一 loader/modid 下的多个 Primary candidate；相同版本也算重复，Nested/External 不计数。没有依赖边的重复节点仍会展示。
- 没有问题且识别完整时显示 `Check passed. No problems found.`；没有问题树但存在识别失败时显示 incomplete，不将不完整结果作为检查通过。
- 本地、显式 SSH 和 Remote workspace 共用相同判定；远端通过现有图 operation 和 mcpmd 执行，不新增检查协议或快照格式。

## 列表

```sh
mcpm --ssh 'ssh user@host' mods list --server /srv/minecraft --parallelism 4
mcpm mods list --server /srv/minecraft --modid create --expanded
mcpm mods list --server /srv/minecraft --no-cache --no-ui
```

- 支持 TUI、Console（`--no-ui`）和 Raw。
- 没有选择输入时列出全部文件。嵌套声明选择保留原始归档身份。
- 与 info 一样只读查询 Server 缓存，支持 `--no-cache`。TUI 来源识别沿用同一有效缓存目标。
- Console、TUI 显示全部缓存、部分缓存、总项目数；TUI 放在 skipped files 统计下方。按展示过滤之前实际检查的唯一文件计数。
- 过滤、列配置和布局在本地处理。TUI 来源查询复用 SSH 会话读取远端文件摘要，来源服务请求在本地执行。

## 缓存

```sh
mcpm mods cache --server /srv/minecraft --all
mcpm mods cache --server /srv/minecraft --modid create --metas meta
mcpm --ssh 'ssh user@host' mods cache --server /srv/minecraft --file example.jar --mode hard-link
printf '%s\n' create jei | mcpm mods cache --server /srv/minecraft --input modid
```

- Console 默认仅显示进度条和最终汇总，`--details` 显示逐文件详情；错误始终显示。Raw 仍逐文件输出结构化记录，两者复用 [`mods cache`](../mod/cache.md) 的逐文件结果。空输入提示选择项目；完整缓存需显式传 `--all`。
- 默认复制完整外层归档，仅为服务器声明的 loader 缓存完整嵌套元数据，不提供 loader 覆盖参数。
- 内容导入和来源结果发布均使用 Server 运行环境的 Mod 缓存。
- `--mode copy|hard-link|prefer-hard-link` 决定新对象的发布方式。硬链接与源文件共享内容保护；已存在的 BLAKE3 对象直接复用，不关联源文件或替换缓存 inode。
- 缺失、过期的元数据在缓存文件上重新识别。`--force` 强制重新识别声明的 loader，尽可能保留其他 loader 的结果。
- `--parallelism COUNT` 默认 10，约束选择扫描和缓存导入、识别。
- 重复路径仅处理一次，对象按 BLAKE3 去重。Console 汇总新增、复用对象及文件、选择失败数；Raw 每个选中的外层文件输出一个 JSON 对象。
- 单文件失败后继续处理后续文件，任何失败令 CLI 返回非零状态；已完成写入保留。缓存遵循[弱一致性保证](../cache.md)。

## 远程协议

协议版本为 5，客户端与远端必须同步升级。Server 请求拒绝原来的 `cache` 目录覆盖字段和 `writing.cache_dir`。

- `server.mods.info`、`server.mods.deps` 接受 `server`（默认 `.`）、`selection`、正整数 `parallelism`（默认 10）及 `no_cache`（默认 false），复用[检查事件流](../remote.md)。
- `server.mods.list` 接受相同字段，事件为 `scope {loaders,environment}`、`scan {completed,total}`、`error {error}`、有序 `item {index,record}`、`done {files,input_errors,failed,cache}`。记录复用 mod-list 表示，可附带 `records` 选择键，每个键包含 `metadata` 和 `record` 索引。
- Info/list 默认使用 Server 缓存，`no_cache:true` 禁用本次查询；deps 始终直接识别。完成事件的 `cache` 包含 `fully_cached`、`partially_cached`、`total`。List scope 的 `environment` 为 `{cache}`，携带文件主机解析后的目录选择；禁用时为 null。后续 TUI 查询沿用此目标，不在客户端打开远端路径。
- `server.mods.source.identify` 接受 `{server,selection,no_cache,parallelism}`，返回 scan/error 事件后，以 `done {paths,failed,environment}` 结束。客户端使用此有效环境进行只读来源识别，平台配置和凭据留在本地。
- List 的失败数覆盖唯一失败扫描文件，包括未选中的索引项；重复展示不增加计数。
- `server.mods.cache` 请求为 `{server,selection,writing}`。`server`、`selection` 必填，选择不能为空，或使用 `all:true`。`writing` 接受 `mode`（`copy`、`hard_link`、`prefer_hard_link`，默认 `copy`）、可选 `metas`（`meta`/`ident` 列表，默认两类）、追加列表 `meta`、`no_metas`（默认 false）、`force`（默认 false）及正整数 `parallelism`（默认 10）。显式 meta 列表覆盖默认集合；`no_metas` 与两个列表冲突。
- 缓存请求首条 data 为 `{cache}`，携带文件主机解析后的 Mod 缓存目录，后续查询和来源结果发布沿用此目标。之后的缓存事件使用 `{event,contents}`：`selection` 包含共享扫描 `progress`/`diagnostic` 事件；`progress` 包含导入、metadata 或 ident 阶段的 `{stage,completed,total}`；`stage_done` 包含 `{stage,success}`；`file` 与 Raw 的 JSON 记录一致；`done` 包含 `{files,failed_files,selection_failures,objects_added,objects_reused}`。并行完成不改变文件输出顺序。导入失败时 ID 为 null，元数据失败可能已发布对象。
- 操作失败体现在缓存记录及完成计数中；包含失败的完整请求仍可正常结束，由 CLI 决定非零退出。处理器失败、取消仍使用传输层错误，此时不要求完成事件。

## 缓存结果类别

`server mods cache` 与 `mods cache` 共用 `--metas meta,ident`（默认两类）和
`--sources modrinth,curseforge`（默认全部）。meta 仅用服务器声明的 Loader，ident 查询所选来源，
两类均导入 缓存内容文件。SSH／Remote 时平台 HTTP 和 API key 留在客户端，验证后的结果通过
`cache.mods.identification.write` 发布到文件主机。Identify 保持只读。
