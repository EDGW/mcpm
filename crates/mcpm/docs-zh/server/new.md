# `mcpm new`

在新目录或已有空目录安装原版 Minecraft 服务端，并生成 mcpm 的登记和启动配置。

```text
mcpm new [OPTIONS] [DIR_PATH]
```

## 参数和选项

| 参数或选项 | 说明 |
| --- | --- |
| `[DIR_PATH]` | 默认 `.`。目标必须不存在或为空目录，父目录必须已经存在。目标不能是文件或符号链接。 |
| `--version <VERSION>` | 精确 Minecraft 版本 ID，或 `latest`（最新正式版）、`latest-snapshot`。默认 `latest`。 |
| `--source <SOURCE1,SOURCE2,...>` | 按顺序尝试的下载源：`bmclapi`、`official`。显式列表完整替换默认的 `bmclapi,official`。 |
| `--source-timeout <CONNECT[:READ]>` | 正整数秒。只给 CONNECT 时 READ 为其 3 倍；省略时连接 10 秒、读取 30 秒。读取超时针对单次响应读取，不是整个下载时长。可独立于 `--source` 使用。 |
| `--eula` | 获取并确认当前 Minecraft EULA。默认拒绝。 |
| `--rcon-port <PORT>` | 自动生成的 RCON 端口，默认 `25575`。 |
| `--rcon-password <PASSWORD>` | RCON 密码。省略时在服务端主机生成随机 UUID/GUID，创建完成后显示实际密码。 |
| `--no-rcon` | 不自动生成 `server.properties`。与 `--rcon-port`、`--rcon-password` 互斥。 |
| `--no-git` | 跳过服务端所属主机上的 Git 检查与创建。 |
| `--git` | 确保服务端目录自身具有 Git 仓库。与 `--no-git` 互斥。 |
| `--full-command` | 安装后显示完整启动命令。 |

## 示例

```sh
mcpm new ./server --version 1.20.1
mcpm new ./server --version 1.20.1 --source official --source-timeout 5:20 --eula
mcpm new ./server --version latest-snapshot --source bmclapi,official --source-timeout 5 --no-git
mcpm --ssh 'ssh user@host' new /srv/minecraft --version 1.20.1 --source official
```

## 安装结果

```text
server/
├── server.jar          # 原版服务端
├── eula.txt            # EULA 同意状态
├── server.properties   # 按版本和选项生成的 RCON 设置
├── launch.toml         # server.jar 启动入口
├── conf.toml           # 用户设置，默认启用 nogui
└── .mcpm/
    ├── server.json     # Vanilla 身份登记
    └── mcpm.json       # Local 工作区登记
```

下载或初始化失败后，目录和已经完成的文件保留；继续操作前应检查实际内容。

## 自动 RCON 配置

按发布时间判断，不早于 `1.9-pre4` 的版本默认创建以下设置：

```properties
enable-rcon=true
rcon.port=25575
rcon.password=<本次生成的随机 UUID>
```

- 旧于 `1.9-pre4` 的版本，显式指定 `--rcon-port` 或 `--rcon-password` 时在版本预检阶段报错；`--no-rcon` 可正常使用。
- 生成的密码保存在 `server.properties` 中，Unix 下文件仅允许当前用户读写。
- 创建结果显示完整 RCON 密码，请妥善保管终端录屏或重定向输出。
- RCON 在服务端启动时启用，请保护密码并按需限制端口访问。

## EULA 确认

先校验所选 Minecraft 版本，再从 <https://www.minecraft.net/en-us/eula> 获取当前 EULA。`latest` 使用版本预检时确定的版本。

输入 `y` 或 `yes`（大小写不敏感）表示同意，写入 `eula=true`；其他输入取消安装。版本校验或 EULA 获取失败时终止创建。

未加 `--eula` 时写入 `eula=false`。启动前需审阅 EULA 并编辑 `eula.txt`。

`--eula` 需要交互终端，与 `--raw` 互斥。EULA 获取同样遵循下载源设置；使用该选项时，源列表需包含 `official`。

## 执行位置与 Git

默认在本机安装；显式 `--ssh` 时，目录检查、版本解析、服务端下载、Git 和登记在远端完成，并使用本次指定的源顺序和超时。EULA 确认在本机进行。

默认复用目标或上级 Git 仓库，没有仓库时在服务端目录创建。其他 Git 行为见 [`init` 文档](basics.md#git)，通用选项与输出模式见 [README](../README.md)。
