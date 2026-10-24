## `mcpm server init`

为现有服务端建立 Local 登记，或者在本机建立指向远端 Local Server 的 Remote 工作目录。此命令不安装服务端，也不启动游戏进程。

```sh
# 在本机识别并登记服务端
mcpm server init ./server
mcpm server init ./server --start-script run.bat

# 使用完整信息直接登记本机服务端
mcpm server init ./server --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
mcpm server init ./server --minecraft-version 1.21.1 --loader vanilla

# 在远端目录初始化 Local Server，位置参数是远端路径
mcpm --ssh 'ssh user@host' server init /srv/minecraft --minecraft-version 1.21.1 --loader vanilla

# 在本机目录创建 Remote 工作目录：位置参数是本地路径，--remote 后是远端路径
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm.exe' server init ./workspace --remote 'D:\EDGW-26Summer'

# 创建工作目录后，后续命令自动使用保存的连接
mcpm server info ./workspace
mcpm server mods list --server ./workspace
```

## 参数解释

- `[PATH]`：已存在的目录，默认 `.`。普通初始化时是服务端目录；使用全局 `--ssh` 时是远端服务端目录；同时指定 `--remote` 时则是本地工作目录。
- `--remote <REMOTE_PATH>`：在本地 `PATH` 创建 Remote 工作目录，指向指定的远端目录。必须配合全局 `--ssh`；全局 `--ssh-mcpm` 可选。目标必须已经是可打开的 Local Server。此选项与 `--start-script`、`--minecraft-version`、`--loader`、`--loader-version` 互斥。
- `--start-script <FILE>`：静态识别指定启动脚本。相对路径以实际服务端目录为基准，也接受绝对路径。与所有手动版本及 Loader 参数互斥。
- `--minecraft-version <VERSION>`：Minecraft 版本，别名 `--mc-version`。信息完整时直接登记；不完整时作为自动识别候选的筛选条件。不能为空白。
- `--loader <LOADER>`：服务端加载器，支持 `forge`、`neoforge`、`fabric`、`vanilla`。已知原版必须显式填写 `vanilla`，不能通过缺少 Loader 证据推断原版。
- `--loader-version <VERSION>`：Loader 版本，与 Minecraft/FML 版本区分。模组加载器的完整手动登记需要此参数；`vanilla` 禁止指定此参数。不能为空白。
- `--force`：允许替换已有登记，包括损坏的 `server.json`。普通初始化替换服务端元数据，`--remote` 模式替换本地保存的连接配置。不能用它切换 Local/Remote 类型，也不能替换配置路径上的目录。
- `--no-ui`：关闭候选选择 TUI，使用 Console 模式。多个候选必须通过更具体的识别条件消除歧义；与全局 `--raw` 同用时使用 Raw 模式。
- `--full-command`：在初始化结果中显示完整启动命令。默认 Console 输出只占一行，超出部分显示 `(+N)`；Raw 输出始终保留完整内容。

SSH 连接和远端 mcpm 程序均复用[全局参数](../README.md#全局选项)，不是 `server init` 的独立参数。

## 行为

### 本地行为

未指定全局 SSH 参数、且 `PATH` 为 Local 或尚未登记时，在本机初始化。成功后目录中包含：

```text
.mcpm/
├── mcpm.json    # version: 1, type: local
└── server.json  # Local 服务端元数据，version: 2
```

`mcpm.json` 由 mcpm 管理，决定工作目录类型；Local 的 `server.json` 由 mcpm-backend 解析和持久化。没有 `mcpm.json` 的旧目录按 Local 处理。Local `server.json` 的 v1 仍可读取，会产生一条 warning 日志；读取不改写文件，重新初始化时写为 v2。

完整指定 `--minecraft-version`、`--loader`，以及非 vanilla 所需的 `--loader-version`，会直接登记，不读取启动脚本。不完整时默认识别 `run.bat`（Windows）或 `run.sh`（Unix），并以已提供的字段筛选可靠候选。

识别只读取脚本和身份依据，不运行脚本、Java、安装器，也不下载文件。支持常见 Forge/NeoForge、Forge shim、Fabric 安装布局及带有版本信息的原版 JAR。静态识别支持明确赋值、引号、续行、目录切换、脚本引用和 Java 参数文件；动态条件、循环、管道、无法确定的变量和递归引用等布局可能无法识别。识别成功不保证服务端可启动；无法识别时应提供完整手动参数。

唯一候选直接登记；多个候选在 TUI 中使用上下键选择、Enter 确认、Esc/Ctrl-C 取消。TUI 要求本地 stdout/stderr 为终端且未指定 `--raw`/`--no-ui`；Console 和 Raw 模式要求唯一候选。stdout 重定向时自动使用 Raw。

登记保存所选启动项的快照，成功后输出与 [`server info`](info.md) 相同的信息。默认不覆盖已有登记；识别失败或取消选择保留旧登记。

### 远程行为

以下两条命令使用相同的全局 SSH 参数，作用不同：

```text
mcpm --ssh COMMAND [--ssh-mcpm EXE] server init <REMOTE_PATH>
mcpm --ssh COMMAND [--ssh-mcpm EXE] server init <PATH> --remote <REMOTE_PATH>
```

| 用法 | 位置参数 | `--remote` 参数 | 登记写入位置 |
|---|---|---|---|
| 不带 `--remote` | 远端服务端目录 | 无 | 远端目录，登记为 Local |
| 带 `--remote` | 本地工作目录 | 已登记的远端 Local Server 目录 | 本地目录，登记为 Remote |

不带 `--remote` 时，脚本、默认入口与元数据读写属于 SSH 主机，候选选择和输出在本地。可以像本地初始化一样使用识别参数或完整手动参数。

带 `--remote` 时，只验证远端 Local Server 并保存本地连接，不初始化或修改远端。生成的 `.mcpm/mcpm.json` 为 `{"version":1,"type":"remote"}`，`.mcpm/server.json` 由 mcpm 管理，例如：

```json
{
  "version": 2,
  "ssh": "ssh user@host",
  "ssh_mcpm": null,
  "path": "/srv/minecraft"
}
```

`ssh_mcpm` 省略或为 `null` 时使用远端 PATH 中的 `mcpm`。SSH 命令中的相对路径（例如 `-i keys/key`）以本地工作目录为基准；远端目录按远端系统解释。

后续 `server init/info` 和已支持远程的 `server mods list/info/deps/source` 在没有显式全局 SSH 参数时自动使用保存的连接。`server init ./workspace --force ...` 因此会重新登记远端 Local Server；若要修改本地保存的连接，应重新使用完整的 `--ssh ... server init ./workspace --remote ... --force`。显式 `--ssh` 优先，位置参数直接按远端路径处理；单独指定 `--ssh-mcpm` 仍然报错。

`mcpm remote` 只打开 Local Server，目标为 Remote 工作目录时拒绝执行，禁止多段 Remote 引用。`server mods check/dep-graph` 暂不支持 Remote 工作目录。配置损坏或连接失败会明确报错，不回退为本地操作。
