# `mcpm add-loader`

为已登记的 v4 原版（Vanilla）服务端原地安装 `forge`、`fabric` 或 `neoforge`。

```text
mcpm add-loader <LOADER[:VERSION]> [OPTIONS]
```

## 参数和选项

| 参数或选项 | 说明 |
| --- | --- |
| `<LOADER[:VERSION]>` | 加载器家族（`forge`、`fabric`、`neoforge`），可附带 `:<version>`（如 `fabric:0.16.10`）。 |
| `--server <PATH>` | 已登记的原版服务端目录，默认 `.`。 |
| `--loader-version <VERSION>` | 精确加载器版本，与内联 `:<version>` 互斥。省略时选择当前 Minecraft 版本的推荐或最新版本。 |
| `--source <SOURCE1,SOURCE2,...>` | 按顺序使用的下载源：`bmclapi`、`official`。可重复传入或逗号分隔，替换默认的 `bmclapi,official`。 |
| `--source-timeout <CONNECT[:READ]>` | 正整数秒。仅填 `CONNECT` 时 `READ` 为其 3 倍；默认 `10:30`。作用于元数据与 installer JAR 下载。 |
| `--force` | 替换冲突的非原版运行时文件（如已有库文件或旧版 Fabric 启动属性）。 |
| `--full-command` | 安装后显示完整启动命令。 |

## 示例

```sh
mcpm add-loader fabric
mcpm add-loader forge --server ./server --loader-version 47.3.0
mcpm add-loader neoforge:21.1.72 --server ./server --source official
mcpm --ssh 'ssh user@host' add-loader fabric --server /srv/minecraft
```

## 适用条件

目标服务端需同时满足：

- 在 `.mcpm/server.json` 中登记为 v4 `Vanilla` 服务端。
- 当前未运行。
- `launch.toml` 适用于当前主机平台（或平台无关），使用 `-jar` 入口，且指向的原版 JAR 与登记的 Minecraft 版本一致。
- `process.workingDirectory` 位于服务端目录以内。

不满足条件时在预检阶段直接报错，不写磁盘。若检测到上次中断的配置发布日志，mcpm 会恢复旧配置并退出，检查后可重试。

## 文件变更与中断处理

| 范围 | 行为 |
| --- | --- |
| 更新 | `.mcpm/server.json`、`launch.toml`、加载器运行文件（如 `libraries/` 与启动 JAR），以及 Fabric `< 0.12.0` 的 `fabric-server-launcher.properties`（保留原有注释和其他键）。 |
| 保持不变 | `conf.toml`、原版服务端 JAR、旧启动脚本、`eula.txt`、`server.properties`、存档与 `mods/`。 |
| 暂存目录 | installer 在 `.mcpm/loader-install/<id>/` 中运行（自动加入 `.mcpm/.gitignore`），结束后清理。 |

- `--force` 仅允许覆盖冲突的非原版运行时文件，不放宽适用条件，也不会覆盖原版 JAR。
- 发布开始前收到中断（`Ctrl-C` 或 SSH 断开）会立即终止 installer 进程树并清理暂存目录，不改动服务端文件；进入发布阶段后会等待配置提交完成。

## 下载源与镜像

- **元数据与 installer JAR**：按 `--source` 顺序依次尝试并应用 `--source-timeout`。若所有配置源均无法提供 `.sha1` 校验和（例如仅用 `--source bmclapi`），mcpm 会在 stderr 发出警告并跳过校验继续下载。
- **installer 内部下载**：使用 `--source` 中首个兼容源：
  - 镜像源（如 `bmclapi`）通过随 `mcpm` / `mcpmd` 分发的镜像代理注入 URL 改写规则；若代理文件缺失或损坏，可运行 `mcpm-tools debug validate` 检查或重新安装。
  - 直连源（`official`）直接运行 installer，不挂载代理。
  - installer 内部下载失败时不会自动切换其他源；镜像缺少文件时可使用 `--source official` 重试。

## 输出与执行位置

- **Console**：显示更新后的服务端摘要（与 `info`、`init`、`new` 一致，支持 `--full-command`）。进度、警告和错误输出到 stderr。
- **Raw**（`--raw` 或 stdout 重定向）：输出 6 列服务端快照 TSV（`name`、`path`、`mc_version`、`loader`、`loader_version`、`command`）。远端目标前置 `remote_path`、脱敏后的 `remote_ssh` 和 `remote_mcpmd` 3 列。

默认在本机执行，目标为 Remote workspace 或传入 `--ssh` 时经远端 `mcpmd` 执行。通用选项与输出模式见 [README](../README.md)。
