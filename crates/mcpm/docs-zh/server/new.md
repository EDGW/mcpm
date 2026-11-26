# `mcpm new`

在新目录或空目录创建 Minecraft 服务端，可选在原版初始化完成后立即安装 Mod 加载器。

```text
mcpm new [OPTIONS] [DIR_PATH]
```

## 参数和选项

| 参数或选项 | 说明 |
| --- | --- |
| `[DIR_PATH]` | 目标目录，默认 `.`。必须不存在或为空目录，且父目录已存在；不接受文件和符号链接。 |
| `--version <VERSION>` | Minecraft 版本 ID、`latest`（最新正式版）或 `latest-snapshot`。默认 `latest`。 |
| `--loader <LOADER[:VERSION]>` | 原版创建完成后立即安装的加载器：`forge`、`fabric` 或 `neoforge`，可附带 `:<version>`。 |
| `--loader-version <VERSION>` | 精确加载器版本。需配合 `--loader`，与内联 `:<version>` 互斥。 |
| `--source <SOURCE1,SOURCE2,...>` | 按顺序尝试的下载源：`bmclapi`、`official`。显式传入时替换默认的 `bmclapi,official`。 |
| `--source-timeout <CONNECT[:READ]>` | 正整数秒。仅填 `CONNECT` 时 `READ` 为其 3 倍；默认 `10:30`。`READ` 针对单次响应读取，而非整个下载时长。 |
| `--eula` | 拉取当前 Minecraft EULA 并提示确认，默认拒绝。 |
| `--rcon-port <PORT>` | 生成的 RCON 端口，默认 `25575`。 |
| `--rcon-password <PASSWORD>` | RCON 密码。省略时在服务端主机生成随机 UUID/GUID，创建完成后显示。 |
| `--no-rcon` | 不生成 `server.properties`。与 `--rcon-port`、`--rcon-password` 互斥。 |
| `--no-git` | 跳过服务端主机上的 Git 检查与仓库创建。 |
| `--git` | 确保服务端目录自身存在 Git 仓库。与 `--no-git` 互斥。 |
| `--full-command` | 安装后显示完整启动命令。 |

## 示例

```sh
mcpm new ./server --version 1.20.1
mcpm new ./server --version 1.21.1 --loader fabric
mcpm new ./server --version 1.20.1 --loader forge:47.3.0 --eula
mcpm new ./server --version 1.20.1 --source official --source-timeout 5:20
mcpm --ssh 'ssh user@host' new /srv/minecraft --version 1.21.1 --loader neoforge
```

## 生成的文件

```text
server/
├── server.jar          # 原版服务端
├── eula.txt            # EULA 状态
├── server.properties   # RCON 配置（1.9-pre4+，未指定 --no-rcon 时）
├── launch.toml         # 启动入口
├── conf.toml           # 用户配置（默认开启 nogui）
└── .mcpm/
    ├── server.json     # 服务端身份登记
    └── mcpm.json       # Local 工作区登记
```

指定 `--loader` 时，mcpm 会在原版基础上继续安装加载器，写入运行所需文件（如 `libraries/` 与启动 JAR），并更新 `.mcpm/server.json` 和 `launch.toml`。

原版下载或初始化中途失败时，已写入的文件会留在目录中。

## Mod 加载器

- 指定 `--loader` 时，加载器版本和下载源在 `--eula` 确认前完成校验，此阶段不写磁盘。
- 原版创建并登记完成后，加载器安装沿用 [`mcpm add-loader`](add-loader.md) 的隔离安装、镜像和中断行为。
- 若原版创建成功但加载器安装失败，原版服务端保持完整可用，stderr 输出错误和 `mcpm add-loader` 重试命令，进程以非零状态退出。

## RCON

发布时间不早于 `1.9-pre4` 的版本默认在 `server.properties` 中写入 RCON 配置：

```properties
enable-rcon=true
rcon.port=25575
rcon.password=<生成的随机 UUID>
```

- 更早版本显式指定 `--rcon-port` 或 `--rcon-password` 会在版本校验阶段报错；`--no-rcon` 可正常使用。
- Unix 下生成的 `server.properties` 仅限当前用户读写。
- 创建结果会显示完整 RCON 密码，注意保护终端记录或重定向输出。

## EULA

- mcpm 先完成 Minecraft 版本（及可选 `--loader`）校验，再拉取 <https://www.minecraft.net/en-us/eula>。
- EULA 始终直连 `www.minecraft.net`（使用 `--source-timeout`），不受 `--source` 限制。
- 输入 `y` 或 `yes`（不区分大小写）写入 `eula=true`，其余输入取消创建。
- 未指定 `--eula` 时写入 `eula=false`。
- `--eula` 需要交互终端，与 `--raw` 互斥。

## 远端执行与 Git

默认在本机创建。传入 `--ssh` 时，目录检查、版本解析、下载、加载器安装、Git 和登记均在远端主机执行，沿用本次的 `--source` 与 `--source-timeout`；EULA 确认仍在本机进行。

默认复用目标或上级目录的 Git 仓库，无仓库时在服务端目录初始化。Git 细节见 [`init`](basics.md#git)，通用选项与输出模式见 [README](../README.md)。
