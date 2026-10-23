# Mod 缓存约定

缓存保存不可变的 Mod 文件内容及可复用的识别结果，由 `local::mcmod::cache` 负责。
Loader 独立识别文件，不参与缓存策略；前端选择缓存并展示库返回的业务类型。

## 一致性原则：允许缺失，已有表示必须正确

**不保证写入的数据一定存在；高并发下可以漏掉写入。所有已表示并接受复用的部分必须正确。**

- 允许跳过写入、数据消失、并发更新丢失；不要求保留每次写入，也不要求最终合并全部 Loader 结果。
- 已表示的结果必须对应正确内容和受支持的识别版本。明确“未识别”是成功识别后的断言；缺失或失败不能写成未识别。
- 多个写者可以从同一旧快照生成有效更新。Forge 更新覆盖 Fabric 更新、丢失 Fabric 结果可以接受；发布半份 JSON 或无关 metadata 不可以。
- 读取方拒绝损坏、不兼容、关联错误的记录。缓存缺失或不可用时回退识别，存在文件不等于缓存可用。
- 发布成功描述本次操作，不保证之后持续存在、可靠备份持久性，或排除并发删除。
- 并发删除和发布可以留下孤立的一半对象，也可以在删除后重新产生对象；删除不阻止并发重新发布。
- 显式操作失败仍须返回错误；允许漏写不允许把失败的对象发布报告为成功。

## 内容身份与目录协议

`CacheId` 是文件完整的 BLAKE3-256，以恰好 64 位小写十六进制字符表示。
BLAKE3 相同即视为同一内容，不做碰撞区分；一个内容 ID 只有一个规范数据文件名。

```text
<cache-dir>/
├── .mcpm/
│   ├── mcpm.json              # {"version":2}
│   ├── index.sqlite3          # 可选的派生索引
│   ├── index.sqlite3-wal      # SQLite 可能创建的运行时文件
│   ├── index.sqlite3-shm
│   └── index.lock             # 仅协调索引恢复
└── <BLAKE3前两位十六进制字符>/
    ├── <完整BLAKE3>.dat
    └── <完整BLAKE3>.data.json
```

- `.dat` 是文件内容的唯一真相，发布后不得原地修改。
- `.data.json` 保存识别结果和校验信息，可以缺失；没有摘要的数据仍可在校验内容后导出。
- 目录版本 `CACHE_DIRECTORY_VERSION = 2`。拒绝 version 1 GUID 缓存；初始化不迁移、删除或接管旧内容。
- `ModsCache::user_default()` 选择 `~/.mcpm/cache/mods`。访问时初始化不存在或空目录；非空无 manifest 目录视为损坏。
- `ModsCache::new(path)` 选择指定目录。即使目录为空，访问也要求已有合法 manifest。
  `initialize()` 显式初始化不存在或空目录，也接受已有合法目录。
- manifest 损坏和不支持版本是目录错误，与单对象的 `CacheStatus` 分开。仅构造对象不执行 I/O。
- 首次初始化使用根目录父目录中的持久 `.mcpm-cache-init.lock`，阻塞获取并要求父目录可写。
  已有合法目录访问不获取此锁；操作活跃期间不要删除协调文件。
- 受管理缓存路径不使用符号链接。staging 文件和目录是临时实现细节，不属于正式对象。

## Metadata 协议与唯一解释边界

摘要包含以下字段：

| 字段 | 含义 |
| --- | --- |
| `version` | 摘要格式版本，当前 `CACHE_FORMAT_VERSION = 2` |
| `blake3` | 内容 ID，必须与规范对象文件名一致 |
| `size` | 文件字节数 |
| `sha256` | 展示及内容/来源校验用 SHA-256，不是第二个对象 ID |
| `metadata_version` | 识别规则版本，当前 `MOD_METADATA_VERSION = 1` |
| `loaders` | 各 Loader 的识别结果 |

Loader 的三个状态必须区分：

| 持久化表示 | 业务类型 | 含义 |
| --- | --- | --- |
| 缺少 Loader 键 | `CachedIdentification::Missing` | 未检查 |
| `null` | `CachedIdentification::Unrecognized` | 成功检查但未识别 |
| Metadata 对象 | `CachedIdentification::Recognized` | 已识别的 `CachedModMeta` |

- 非空且全部为 null 的 `loaders` 合法。识别错误不能写成 null；空 map 不构成可用识别摘要。
- `ModMeta` 持有运行时 Loader 实例；`CachedModMeta` 保存 Loader ID、直接声明、依赖和递归的相对归档路径。
  捕获、恢复通过类型转换完成，不处理 JSON 或文件系统。
- 恢复运行时 metadata 使用调用方的 Loader 注册表；缓存抽象不引用具体 Loader 实现。
- `cache::directory` 是 manifest 的唯一编解码者；`cache::storage` 是对象摘要的唯一编解码者；
  `cache::index` 负责 SQL schema、索引版本和采样算法。
- 所有持久化数据解释必须经过所有者；接口信息不足时扩展所有者，不在 `open()`、Loader 或 CLI
  增加第二套解析逻辑，也不在这些层直接检查 JSON。
- `CacheDigest::identification()`、`identifications()` 暴露三态，调用方无需解释序列化中的 null/missing。

## 访问对象与可用状态

| 对象 | 操作 | Receiver |
| --- | --- | --- |
| `ModsCache` | 选择目录、初始化、打开 Reader/Writer | 构造函数或 `&self` |
| `CacheReader` | `get`、`list`、`verify`、`export` | `&self` |
| `CacheWriter` | `import`、`import_with_mode`、`refresh_metadata`、`remove`、`rebuild_index` | `&mut self` |
| `CacheWriter` | 全部 Reader 操作 | 只读 `Deref` |

- Reader/Writer 区分访问能力，均不持有全局或对象锁；Writer 生命周期不构成排他事务。
- 独立对象的操作可以交错，`&mut self` 不排除其他实例、线程或进程；多次调用间没有一致快照，也没有批次回滚保证。
- `CacheEntry` 是一次观察，路径和状态可以立即变化。
- 没有对象共享锁升级或重入协议；SQLite、索引恢复、初始化各自有独立同步机制。

| 状态 | 含义 |
| --- | --- |
| `Available` | 数据、摘要结构、大小和 metadata 版本有效，至少检查过一个 Loader |
| `NotFound` | 数据和摘要均不存在 |
| `Unavailable(MissingDigest)` | 数据存在，摘要缺失 |
| `Unavailable(InvalidData)` | 数据缺失、文件类型错误、摘要损坏/不兼容，或完整性校验失败 |
| `Unavailable(UnexpectedError(String))` | 意外的文件系统或操作系统访问失败 |

`get` 检查结构和大小，不执行完整哈希，无法检测同大小的内容损坏。
`list` 包含孤立对象及单对象不可用状态，忽略临时名字；枚举失败返回 `CacheError`。
`verify(Structure)` 检查结构和大小；`verify(Content)` 对可用或缺少摘要的数据计算哈希，
校验 BLAKE3 文件名及已有指纹。没有摘要但内容正确的对象仍为 `MissingDigest`。
校验不修复数据，不通过改写保存的指纹来接受变化后的内容。

## 显式修改与导出

### 导入和重复发布

- `import()` 默认 `Copy`，返回内容 ID。`import_with_mode()` 支持 `Copy`、`HardLink`、
  `PreferHardLink`，返回 `ImportOutcome`。
- 导入不识别 Mod、不生成摘要。先固定源句柄，计算 BLAKE3 和 imohash，再决定是否传输。
  规范数据名已存在时，校验内容 ID 后直接复用，不创建 staging、不尝试硬链接、不改变源权限，
  也不替换已有 identity；已有目标无效时返回错误。
- mode 仅用于新对象。因此即使 `HardLink` 无法跨文件系统链接源文件，已有对象仍可复用。
  未命中后才按模式在缓存文件系统准备 staging，验证其哈希与最初源 ID 一致，保护内容后原子不覆盖发布。
  从哈希、权限保护到发布，使用保留的文件观察验证源及 staging，不能重新取基线掩盖中途变化。
- Copy 在 staging 发布前刷新新写入字节；HardLink 发布已有字节，不写内容，也不额外刷新只读源句柄，
  不承诺源文件在断电后的持久性。
- 并发发布使用相同的不覆盖规则；竞争失败方验证并复用胜出方数据。临时名字正常清理。
- `published = true` 表示本次发布 `.dat`，`method` 为 `Some(Copy)` 或 `Some(HardLink)`。
  复用时 `published = false`、`method = None`。
- 复用内容不等于源文件变成硬链接。独立副本仍然独立；导入不替换源文件名来建立关联。
- 硬链接 staging 校验原生 identity。内容保护影响全部链接，包括源文件；即使稍后失败或输掉发布竞争，
  已应用的保护也可能保留；最初已命中合法对象时没有这些副作用。
- `PreferHardLink` 仅在跨文件系统或不支持操作时回退复制，不对权限失败或任意 I/O 错误回退。
  文件系统 helper 保留原生错误，导入层负责回退策略。
- 普通复制无法把正在被外部修改的源文件变成文件系统级瞬时快照；有严格备份要求的调用方须稳定输入。

### 刷新与内部显式 metadata 写入

- `refresh_metadata` 使用选定 Loader 识别缓存字节，不读取原导入来源；空 Loader 选择不修改对象。
- `MissingOrOutdated` 跳过当前版本已检查结果，包括未识别；`Force` 重新解析选中 Loader。
  全部选中尝试成功后才发布，失败保留原摘要或缺失状态。
- 保留未选中的当前结果；metadata 版本变化时丢弃旧结果，仅记录本次按当前规则检查的结果。
  当前版本记录损坏则拒绝，不静默接受。
- 识别前计算指纹，保留并验证文件观察；提交通过共用路径重新检查 BLAKE3、SHA-256、大小。
- **`CacheWriter::write_metadata` 仅为 `pub(crate)`，任何情况下不得在 crate 外暴露或使用。**
  不当的 metadata 关联会破坏数据一致性。每个调用处必须注明此限制，并建立结果来自当前规则下该内容的来源保证。
- 该受信接口接收内容 ID、预期 SHA-256 和类型化 Loader 结果；哈希及结构检查不能证明语义来源，调用方必须保证。
  `None` 断言成功检查但未识别，省略项保留当前结果；空更新不创建缺失摘要。
- `cache::storage` 将完整快照编码到私有临时文件，刷新后原子替换摘要；编码内容相同则跳过发布。
  并发合并允许丢失更新，无需 metadata 合并锁。
- 不修改 `.dat`。并发删除可以留下仅有摘要的对象，读取方拒绝不完整组合。

### 导出与删除

- 导出属于 Reader 操作，与识别无关。可接受缺少摘要，拒绝损坏的内容描述。
  即使 metadata 过期，也校验 BLAKE3 和已有的内容指纹。
- 目标必须不存在、位于缓存外，父目录必须已存在。复制导出先准备、校验独立临时文件，再不覆盖发布。
  硬链接导出校验内容、创建链接，再校验结果。
- `HardLink` 要求建立链接；`PreferHardLink` 仅在文件系统能力失败时回退，返回实际方式。
- `remove` 删除缓存的两个名字，并尽力清理索引项；其他硬链接继续有效。
  两个名字都不存在时返回 false；部分删除失败必须报错。
- 不隐含自动淘汰、引用计数或备份引用追踪。批量调用的顺序及部分成功由调用方负责。

## 只读缓存识别

- `ModOpenContext::default()` 关闭缓存 I/O。选择默认或指定缓存时验证目录，不扫描全部对象；
  一个 context 可用于并行 open。选择默认目录时仍可能执行首次初始化。
- `CacheLookup::find` 固定输入句柄，获取原生 identity 并计算 imohash。
  解析到规范 `.dat` 路径可直接提供候选，同时使用快速索引候选；比较当前固定的输入、候选 identity，
  验证文件观察后接受快速路径。
- Unix identity 是设备号加 inode，Windows 是卷标识加 File ID。固定句柄防止比较期间 identity 复用。
  路径或采样哈希本身不能证明相等。
- 没有身份匹配时，计算完整 BLAKE3，直接查规范对象路径，因此内容相同的独立副本也可以复用 metadata。
- 身份快速路径依赖缓存内容不可变；采样不能发现所有原地修改，原生 identity 也不是完整性校验和。
- `open()` 复用有效的已识别和未识别结果；缺失、不可用或无法使用的 Loader 结果正常识别，保留原有错误回调。
  Loader 不处理缓存策略。
- open 不导入文件、不创建硬链接、不改变内容权限、不写 metadata、不更新或修复索引。
  存储和刷新均通过显式接口；SQLite 内部可能维护自己的 WAL 运行时文件。
- `ModOpenReport` 包含 `ModFile` 及 disabled/miss/matched 缓存使用报告，包括对象状态和各 Loader 使用情况，
  没有写回结果。返回文件保留输入路径、启用状态，嵌套路径保持相对。
- full/partial/none 按请求 Loader 的实际复用统计，缓存未识别也算复用，空 Loader 选择不算命中。
  前端在筛选或展开声明前聚合文件检查；open 不产生缓存新增/修改统计。

## 可选 imohash 索引与恢复

- `cache::index` 负责 imohash-1.0：MurmurHash3 x64-128；至少 128 KiB 的文件在开头、中点、末尾各采样
  16 KiB，小文件完整采样。索引版本同时固定 schema 与算法参数，当前为 1。
- SQLite 仅保存 `(imohash BLOB(16), blake3 BLOB(32))` 关联，联合唯一，并建立反向 BLAKE3 索引。
  不保存 ModMeta、绝对路径或持久 identity；内容与摘要仍是唯一真相。
- imohash 碰撞只增加候选，不证明内容相同。索引可漏项、可陈旧；验证候选，未命中回退 BLAKE3。
- 导入尽力插入关联，删除尽力清理；仅修改 metadata 不更新索引。SQLite 使用 WAL 事务，busy timeout 为 0，
  索引忙时可丢弃可选更新，不使数据操作失败。
- 查询只读打开索引。缺失、损坏、不兼容或忙时记录性能回退日志，不创建、重建或修复索引。
  导入可初始化缺失索引；已有损坏索引需要显式恢复。
- `CacheWriter::rebuild_index()` 扫描规范数据名字，校验 BLAKE3，采样有效内容并重建派生数据库。
  跳过无效或缺失数据；并发内容变化仍可能导致漏项或陈旧项。返回本次登记数量，不是持久对象总数保证。
- `.mcpm/index.lock` 为数据库连接提供非阻塞共享 lease，为重建发布提供独占恢复访问，防止删除仍在使用的 WAL 数据库。
  无法获取时查询回退、可选更新跳过、显式重建报错；它不是全局对象或 metadata 锁。
- 数据发布与索引事务独立，不要求跨文件事务或无损恢复日志。刷新私有文件和原子发布不保证断电后的多文件持久性。
  不自动删除未知孤立内容。

## 权限与文件系统边界

- 新数据发布前保护内容，目录保留写权限以便删除或替换名字。Unix 移除写权限位，不扩大读权限。
- Windows 拒绝 `FILE_WRITE_DATA`、`FILE_APPEND_DATA`，保留其他 ACL 权限和删除行为，不使用只读属性。
  null DACL 明确不支持；无法保护时返回错误。
- 硬链接共享内容、权限和 identity；使用方写入新文件后替换自己的文件名，不原地覆写、追加或截断。
  导出的普通副本独立可写。
- 所有者、管理员、既有可写句柄可以绕过后续权限变更；这不是对抗性不可变边界。
  Windows 替换还取决于共享标志及文件占用。
- 文件系统 identity、权限保护、硬链接 helper 不依赖 Cache 类型或 Loader。
  Cache 负责命名、内容校验和导入/导出策略，`open` 负责复用/回退，前端负责选择与展示。

[English](../../docs/conventions/cache.md)
