# `server.new`

## 输入与结果

`server.new` 接受两类输入，返回 `StreamingOutput<NewEvent>`：

| 输入 | 结果 |
| --- | --- |
| `NewInput::Prepare { path, version, download_config, rcon_requested, loader }` | 校验目录、Minecraft 版本、RCON 参数与可选加载器目标，返回 `Prepared`。 |
| `NewInput::Install { prepared, accept_eula, no_git, git, rcon }` | 安装已准备的原版服务端及可选加载器，返回 `Installed { server, git, java_major, rcon, loader_error }`。 |

`download_config` 使用 `DownloadConfig` codec 编码。`Prepared` 保存已固定的原版工件、可选 `PreparedLoader` 及原下载配置供 `Install` 使用。

## 创建行为

- 父目录必须已存在，目标必须不存在或为空目录；拒绝文件和符号链接。
- `Prepare` 不写磁盘，供调用方在安装前完成 EULA 确认。
- `Install` 下载并校验 `server.jar`，写入 `eula.txt`，按需生成 RCON 配置，准备 Git、发布启动配置并登记 Local 工作区。
- 原版下载或初始化失败时保留已写入的文件，并在错误中标明失败阶段。

## Mod 加载器

- 指定 `loader` 时，`Prepare` 校验下载源兼容性（镜像源同时校验随包镜像代理）并固定规范化的 `PreparedLoader` 版本。
- 原版创建完成后，`Install` 复用 [`server.loader.add`](loader.md) 流程安装加载器。
- 加载器安装失败时，`Install` 仍返回携带原版 `ServerInfo`、RCON 信息和 `loader_error: Some(...)` 的 `Installed`；流取消则以 `Err` 中止。

## RCON

- `releaseTime` 不早于 `1.9-pre4` 的版本默认生成 RCON 配置，省略密码时生成 UUID v4。
- 更早版本拒绝显式端口或密码（`rcon_requested: true`）；`--no-rcon`（`rcon: None`）仍被接受。
- 返回状态为 `Created { port, password }`、`Disabled` 或 `UnsupportedVersion`。`Created` 返回明文密码供展示，并在 `Debug` 中脱敏。

[Server operations](README.md) · [English](../../docs/server/new.md)
