# Mod 与来源缓存约定

内容、目录管理、原子发布、传输和 imohash 查询统一由 `local::cache` 负责。
版本 3 目录协议和通用接口参见[通用本地缓存](local-cache.md)。

## 职责与选择

- `local::cache::mods::ModsCacheDir` 是 Mod 缓存入口，内部私有封装通用 `CacheDir`。
- 通用缓存只管理不可变内容与具名 metadata；Mod 的 metadata 名为 `meta` 和 `ident`。
- `ModsCacheDir::import` 只导入内容；`verify` 只验证内容结构或 BLAKE3，不解释任何 metadata。
- `local::cache::mods::meta` 负责 Loader 快照与 Mod 刷新策略。
- `local::cache::mods::ident` 负责来源 `FileMatch` 快照与来源刷新策略。
- 两者在 `ModsCacheDir` 内部使用 `CacheDir` 和类型化 `Codec`，不各自实现文件存储、目录命名、权限、硬链接或索引。
- 所有持久化元数据均实现 `Codec`：`CacheDirMetadata`、`CacheDigest` 和来源快照。
  JSON 的解释只发生在对应 Codec 内。
- `ModsCacheSelection::Default.open()` 初始化不存在或空的 `~/.mcpm/cache/mods`；
  `Directory { path }.open()` 要求目录已合法初始化。保留默认位置，但不静默接管或迁移版本 1/2 目录。

## 运行资源

- `local::runtime::RuntimeEnvironment` 在缓存层之上组合操作资源；存储与 metadata codec 不依赖它。
- `new()` 不访问文件系统。`cache.global`、`cache.mods`、`cache.backup`、`cache.stash` 默认共享同一个 `Arc<RuntimeCache>`，其中只有 `mods` 可空。
- `RuntimeCache` 保留 `ModsCacheSelection`，首次使用时通过其唯一目录策略打开缓存。环境克隆共享成功打开的资源；失败保留错误上下文，后续可重试。
- `with_mods_cache(None)` 禁用 Mod 查询，不回退 global；禁用时 `require_mods_cache()` 拒绝显式写入。备份创建使用 `cache.backup`。
- `MinecraftServer::runtime()` 借用 Server 持有的环境；当前提供全 global 默认值，不读取或写入 Server 专属设置。
- 独立 Mod 命令入口从现有参数选择资源；Server 命令入口使用 Server 环境，仅应用 `--no-cache` 等本次操作策略。
- 缓存管理继续显式使用 `ModsCacheDir`。运行环境不改变查询与写入保证，也不接管 metadata 解释。

## Loader 元数据

`<key>.meta.json` 保存格式版本 2、BLAKE3 key、大小、SHA-256、识别规则版本
`metadata_version`（当前为 1）以及 Loader map。

| 持久化状态 | 类型化状态 | 含义 |
| --- | --- | --- |
| 缺少 Loader key | `CachedIdentification::Missing` | 尚未检查 |
| `null` | `Unrecognized` | 成功检查但未识别 |
| 对象 | `Recognized` | 有效 `CachedModMeta` |

- 运行时 `ModMeta` 与持久化 `CachedModMeta` 通过对应类型转换；通用缓存不引用具体 Loader。
- 在 `ModsCacheDir` 上使用 `mod_metadata`、`list_metadata`、`verify_metadata` 和 `refresh_metadata`；
  这些是明确的 Loader metadata 操作，不属于通用 `CacheDir`。
- `mod_metadata` 检查快照结构及内容关联，不读取完整内容做哈希。
  `verify_metadata(Content)` 额外校验完整 BLAKE3 和保存的 SHA-256。
  仅结构校验无法发现同尺寸内容损坏。
- 刷新读取固定的实际缓存字节，收齐全部选中 Loader 结果，再重新核对确切字节来源后提交。
  任一失败均保留原快照或其缺失状态。
- `MissingOrOutdated` 复用当前已检查结果，包括未识别；`Force` 重新识别选中 Loader。
  保留未选中的当前结果，规则版本变化时丢弃旧结果，拒绝合并损坏的当前快照。
- Loader 原始提交入口保持 crate 私有。调用方必须证明所有结果来自当前规则下的确切内容；
  哈希检查本身不能证明语义来源。前端不提交原始 Loader 结果。

## 来源识别

`<key>.ident.json` 是独立的格式 1 快照，保存 BLAKE3、大小、SHA-1 和非空来源 map，
其有效性不依赖 Loader 元数据。

- 在同一个 `ModsCacheDir` 上使用 `ident_info`、`write_ident_info` 和 `refresh_ident_info`。
- Codec 验证来源 key、引用、文件名、大小和 SHA-1，map key 必须与对应结果的来源一致。
  仅保存成功且验证通过的匹配。
- 缺来源表示未检查；错误和未匹配不形成负缓存。
- `Source::identification_cache_id` 负责持久命名空间。CurseForge 配置共享 `curseforge`，
  凭据和 endpoint 不进入快照。
- 刷新固定实际缓存内容，验证 BLAKE3，计算来源要求的摘要，查询来源后只发布有效成功结果。
  失败或没有匹配时保留此前成功记录。
- `refresh_ident_info_with_outcome` 另外返回本次 `RefreshStatus`（`Reused`、`Matched`、
  `Unmatched`）和刷新后的 `CacheIdentInfo`。本次未匹配时，可能仍有此前成功记录对应的
  可用快照；没有此前记录则不创建快照。本次状态不进入持久化数据。
- `write_ident_info` 接受客户端/远程编排取得的可信 provider 结果；调用方负责 provider 来源，
  库验证实际内容的 BLAKE3、大小和 SHA-1。
- 复用或传输前，将缓存匹配与查询文件摘要核对；附加文件结果保留实际附加下载。

## 可用性、查询与并发

- Loader 和来源分别报告独立的 `CacheStatus`，区分缺失快照、无效数据和意外 I/O 失败。
- 任何可复用快照都必须具有普通内容文件；孤立 JSON 不代表可用，但仍可由通用 `scan`、`remove` 观察和删除。
- Mod 打开和来源查询通过 `ModsCacheDir::find_metadata` 和 `find_ident_info`，
  内部使用 `CacheDir::find_content`，再交给对应的类型化读取逻辑。
  保留的句柄避免命中后再次完整哈希。
- 查询不导入、不创建硬链接、不改变权限、不发布结果、不修复索引。
  无法复用的业务记录回退正常识别；可选索引缺失/无效时回退完整 BLAKE3，imohash 本身不证明相等。
- 元数据原子发布完整快照；文件名被替换后，已打开的完整旧快照仍可读取。
  并发写入允许丢失部分更新，但不允许半份 JSON 或错误关联结果，不提供批量事务。
- 异步来源刷新包含同步哈希与存储操作；批处理使用适当的 blocking worker，并保证 provider
  HTTP runtime 在全部请求完成前持续存活。

[English](../../docs/conventions/cache.md)
