# 下载源约定

- 下载源只负责 URL 地址翻译，不解释元数据，不负责版本选择或安装；公共下载层不要求按 component 注册能力。
- 下载源顺序、请求执行、失败回退及文件校验由统一下载流程处理，不散落在调用方。
- 每类下载对象（如 authlib-injector、Minecraft、Forge）由独立的统一入口封装。官方请求地址、路径拼接、元数据编解码、校验及字段含义均由该入口维护；已有解析逻辑必须复用。
- 调用方只使用封装提供的类型化接口和数据，不在外部硬编码请求地址、拼接协议路径或重复解析元数据。
- 原始响应、JSON Value、协议 DTO 等 raw 数据保持封装内部私有，不提供绕过封装直接持有、读取或解释 raw 的接口。
- 封装提供的信息不足时，扩展原入口，不另建解析入口或透传 raw。响应中包含的下载地址也由该入口解释，再交给统一下载流程。
- URL 翻译仅适用于协议兼容的镜像；响应格式不同的版本目录接口通过可选的 `Catalog` 组合适配并由统一查询流程执行，不污染地址翻译层，也不在 `DownloadSource` / `Catalog` 内执行 HTTP I/O。

## 接口与示例

- `download::DownloadSource` 负责地址翻译，并可选提供 `catalog() -> Option<&dyn Catalog>` 声明镜像专属目录协议；`sources::MappedSource` 支持自定义目录前缀映射与 `with_catalog(Arc<dyn Catalog>)` 组合，按域名、端口和路径边界匹配。仅在原地址唯一时还原镜像地址，不猜测共用 Maven 路径的上游。
- `download::catalog::{Catalog, CatalogEndpoint}` 为纯描述型目录契约（目标 `Url` + 纯解码函数，不做 I/O），统一输出 Minecraft、authlib-injector、Forge、NeoForge 与 Fabric 的标准版本列表模型；原始上游 JSON/XML DTO 私有封装在各 `Catalog` 实现内。
- `sources::OfficialCatalog` 实现全部官方目录协议与宽容解码器（`OfficialSource::catalog()` 返回它），是官方目录地址与解码逻辑的唯一真相（其中 NeoForge 目录使用 Modrinth `neo/v0/manifest.json` 提供的显式 `MC -> NeoForge` 映射，不使用其 `url` 字段）；`sources::BmclapiCatalog` 覆写 Forge 与 NeoForge 专属 JSON 接口（`/forge/minecraft/{mc}`、`/forge/promos` 与 `/neoforge/list/{mc}`），其余目录返回 `Ok(None)` 自动经 `bmclapi()` 前缀表翻译并复用官方解码器。
- `DownloadConfig` 通过现有 `Codec` 统一读写 TOML；`sources::downloader` 装配内置 `bmclapi`、`official`。默认顺序为 BMCLAPI、官方，显式列表完整替换默认值；空列表禁用网络下载，不暗中追加来源。
- `Downloader` 按顺序请求并去重地址；网络、元数据解析或校验失败可切源，本地文件错误立即返回。`Downloader::query_catalog_with_progress` 在单次源尝试内依次执行目录覆写/翻译、解码与目标版本选择，镜像滞后找不到目标版本时自动切源。目标父目录须存在，校验通过后发布文件，不覆盖已有目标；取消可通过丢弃 future 实现。
- `download::authlib_injector::{latest, build}` 返回固定的 `Release`；调用 `Release::download_to` 下载，切源不重新选择版本。原始响应和协议类型均不公开。
- `download::mojang::resolve_server` 按精确 ID、`latest` 或 `latest-snapshot` 解析确定的原版服务端下载对象；目录、版本 JSON 和 JAR 均遵循调用方的源顺序，JAR 按上游 SHA1 和大小校验。`download::eula::fetch` 实时获取官方网页，返回前 20 个非空正文行、标题结构和剩余行数，不缓存或内嵌 EULA。
- Mojang `ServerRelease` 保留 catalog 的 `releaseTime`，不使用元数据修改时间 `time`。它的唯一 Codec 支持在 EULA 确认前后传递同一个固定 artifact，不重新解析 latest，也不在调用方解释上游原始 JSON。
- CLI 的共享 `SourceArgs` 把 `--source` 和 `--source-timeout CONNECT[:READ]` 导出为 `DownloadConfig`。显式源完整替换默认源；只给 CONNECT 时 READ 为其 3 倍。`new` 向 Host 传输同一 codec 编码的配置，Host 不改用另一份默认配置。
- `Downloader::download_to_with_progress`、`authlib_injector::latest_with_progress` 和 `Release::download_to_with_progress` 通过同步回调报告类型化事件；不输出文字、不依赖 UI。请求事件区分元数据与文件，报告实际来源；每次尝试重置字节计数。总大小优先使用协议期望，其次 HTTP 大小提示；大小提示不替代摘要验证。
- 回调必须快速返回；回调失败终止操作，不作为远端错误回退。失败通知本身出错时同时保留原请求错误；完成通知只在元数据解析或文件验证发布成功后发送，通知失败不会回滚已发布文件。取消仍由丢弃 future 实现。
- 服务端启动通过 `local::mc::server::authlib_injector` 复用此 API；该模块拥有本地 meta、验证及替换，通用下载层不依赖服务端。启动按需创建默认源下载器，尚未接入全局下载配置自动加载或 cache。调用方运行 Tokio 时须启用 I/O 和时间驱动。

在 `mcpm-lib` 目录运行：

```sh
# 查询版本；增加目标路径则下载该构建
cargo run --example download_authlib_injector -- latest
cargo run --example download_authlib_injector -- 56 ./authlib-injector.jar
```

目标路径后可选传入配置文件路径。用 `-` 代替目标路径时仅查询，不下载。配置文件使用以下 TOML 格式：

```toml
sources = ["bmclapi", "official"]
connect_timeout_seconds = 10
read_timeout_seconds = 30
```

[English](../../docs/conventions/download-source.md)
