# `server.loader.add`

Host 侧流式 operation，为已登记的 v4 原版（Vanilla）服务端原地安装 `forge`、`fabric` 或 `neoforge`。

## 输入与结果

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `path` | `PathBuf` | 服务端所在 Host 上的目录路径。 |
| `family` | `LoaderFamily` | `forge`、`fabric` 或 `neoforge`。 |
| `loader_version` | `Option<String>` | 精确加载器版本；`None` 表示自动选择当前 Minecraft 版本的推荐或最新版本。 |
| `download_config` | `Vec<u8>` | 经 `DownloadConfig` codec 编码的下载配置。 |
| `force` | `bool` | 允许覆盖冲突的非原版运行产物。 |

返回 `StreamingOutput<AddLoaderEvent>`：

| 事件 | 含义 |
| --- | --- |
| `Stage { stage, message }` | 阶段切换（`preflight`、`resolve`、`download`、`install`、`verify`、`publish`）。 |
| `DownloadProgress { message, received, total }` | 元数据或 installer JAR 下载进度。 |
| `InstallerAttempt { source, attempt }` | 本次 installer 执行使用的下载源与轮次。 |
| `Notice(String)` | 警告或提示信息。 |
| `Completed(Box<ServerInfo>)` | 提交后的服务端摘要（只读 `inspect`，不创建 `mods/`）。 |

## 执行流程

1. **预检**：校验下载源（镜像源同时按 SHA-256 校验 Host 可执行文件旁的 `mcloader-injector-agent.jar`）、`workspace::require_local` 与无活跃 `running.lock`，并快照 `.mcpm/server.json`、`launch.toml`、`conf.toml` 与原版 JAR。若存在未完成的 v1 发布日志，持 `.mcpm/server.lock` 恢复旧配置后报错退出。
2. **解析**：经 `download::loader` 解析并固定 installer 工件。阶段 1–2 不写磁盘（恢复残留发布日志除外）。
3. **下载与安装**：补齐 `.mcpm/.gitignore` 中的 `/loader-install`，创建 `.mcpm/loader-install/<id>/`，下载 installer JAR，镜像源复制代理 JAR 到暂存目录，并在断流取消监听下于隔离目录执行 installer。
4. **验证**：按需展开平台参数文件、合并 `launch.toml`（不修改 `conf.toml`），经 `BUILTIN_LOADERS` 验证加载器身份。
5. **发布**：进入不可中断窗口，持 `.mcpm/server.lock` 并确认无活跃 `running.lock`，复检快照与写前冲突后落盘白名单产物，经 `config::commit` 原子提交 `.mcpm/server.json` 与 `launch.toml`。
6. **清理**：删除暂存目录并发送 `Completed(ServerInfo)`。

## 下载源与错误

- **`.sha1` 缺失**：全部已配置源均无法获取 `.sha1` 且未取消时，发出 `Notice` 并跳过校验继续下载；只要获取到校验和，不匹配即失败。
- **镜像注入**：选用首个提供静态 `MappingTable` 的源执行 installer。非空映射表挂载 `-javaagent:<staged-agent>` 与 `-Dmcloader.injector.table=...`；空表（`official`）直连执行。
- **安装失败**：installer 按所选源执行一次，不自动切源重试；非零退出返回携带下载源 ID 与日志尾部的 `InstallerFailed`。

[Server operations](README.md) · [English](../../docs/server/loader.md)
