## `mcpm init`

为现有服务端建立 Local 登记，或者在本机建立指向远端 Local Server 的 Remote 工作目录。此命令不安装服务端，也不启动游戏进程。

```sh
# 在本机识别并登记服务端
mcpm init ./server
mcpm init ./server --start-script run.bat

# 使用完整信息直接登记本机服务端
mcpm init ./server --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
mcpm init ./server --minecraft-version 1.21.1 --loader vanilla

# 跳过 Git 检测与初始化
mcpm init ./server --minecraft-version 1.21.1 --loader vanilla --no-git

# 即使属于父目录仓库，也在 server 目录建立自身仓库
mcpm init ./server --force --minecraft-version 1.21.1 --loader vanilla --git

# 在远端目录初始化 Local Server，位置参数是远端路径
mcpm --ssh 'ssh user@host' init /srv/minecraft --minecraft-version 1.21.1 --loader vanilla

# 在本机目录创建 Remote 工作目录：位置参数是本地路径，--remote 后是远端路径
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm-tools.exe' init ./workspace --remote 'D:\EDGW-26Summer'

# 创建工作目录后，后续命令自动使用保存的连接
mcpm info ./workspace
mcpm mods list --server ./workspace
```

## 参数解释

- `[PATH]`：已存在的目录，默认 `.`。普通初始化时是服务端目录；使用全局 `--ssh` 时是远端服务端目录；同时指定 `--remote` 时则是本地工作目录。
- `--remote <REMOTE_PATH>`：在本地 `PATH` 创建 Remote 工作目录，指向指定的远端目录。必须配合全局 `--ssh`；全局 `--ssh-mcpm` 可选。目标必须已经是可打开的 Local Server。此选项与 `--start-script`、`--minecraft-version`、`--loader`、`--loader-version` 互斥。
- `--start-script <FILE>`：静态识别指定启动脚本。必须使用相对于实际服务端目录的路径，不接受绝对路径或越界路径。与所有手动版本及 Loader 参数互斥。
- `--minecraft-version <VERSION>`：Minecraft 版本，别名 `--mc-version`。信息完整时直接登记；不完整时作为自动识别候选的筛选条件。不能为空白。
- `--loader <LOADER>`：服务端加载器，支持 `forge`、`neoforge`、`fabric`、`vanilla`。已知原版必须显式填写 `vanilla`，不能通过缺少 Loader 证据推断原版。
- `--loader-version <VERSION>`：Loader 版本，与 Minecraft/FML 版本区分。模组加载器的完整手动登记需要此参数；`vanilla` 禁止指定此参数。不能为空白。
- `--force`：允许替换已有登记，包括损坏的 `server.json`。本地导入替换服务端元数据和 `launch.toml`，已有 `conf.toml` 逐字节保留；`--remote` 模式替换本地保存的连接配置。不能用它切换 Local/Remote 类型，也不能替换配置路径上的目录。
- `--foverride-conf`：与 `--force` 同时使用，重新生成本地 `conf.toml`，覆盖已有设置和注释；不支持 SSH 或 `--remote`。
- `--git`：要求目标目录有自身的 Git 仓库，即使属于父目录仓库也会在目标目录创建。目标已有仓库或 worktree 时直接复用，不删除或重建。与 `--no-git` 互斥。SSH 初始化时作用于远端 Server；使用 `--remote` 时作用于本地工作目录。
- `--no-git`：跳过 Git 仓库检测与自动初始化，仍按正常规则生成 `.mcpm/.gitignore`。与 `--git` 互斥。SSH 初始化时作用于远端 Server；使用 `--remote` 时作用于本地工作目录。
- `--no-ui`：关闭候选选择 TUI，使用 Console 模式。多个候选必须通过更具体的识别条件消除歧义；与全局 `--raw` 同用时使用 Raw 模式。
- `--full-command`：存在旧命令快照时显示完整内容；新本地导入不在元数据中保存完整命令。

SSH 连接和远端 mcpm 程序均复用[全局参数](../README.md#全局选项)，不是 `server init` 的独立参数。

## 行为

### 本地行为

未指定全局 SSH 参数、且 `PATH` 为 Local 或尚未登记时，在本机初始化。成功后目录中包含：

```text
conf.toml      # Java、内存、GC 和用户设置
launch.toml    # 识别导入时生成；手动登记不伪造入口
.mcpm/
├── .gitignore   # 忽略 /cache、/data 和 /registration.lock
├── mcpm.json    # version: 1, type: local
└── server.json  # Local 服务端身份元数据，version: 3
```

初始化 Local 或 Remote 工作目录时自动创建 `.mcpm/.gitignore`，内容为 `# Added by mcpm`、一个空行以及 `/cache`、`/data`、`/registration.lock`、`/launch.lock`、`/launch-publication.json` 等规则，不新增 stash 忽略规则。已有文件保留原内容；忽略空行后，若文件末尾已与完整内容一致，则不追加，否则换行后追加完整内容。因此，连续使用 `--force` 初始化不会重复追加。

Git 默认行为：

- 写入登记前，用 Git 检测目标目录是否已属于仓库；已有仓库、父目录仓库或 worktree 均直接复用。
- 未发现仓库时，在目标目录执行 `git init`，不自动暂存或提交文件。
- Git 必须安装在执行初始化的主机上；Git 不可用、仓库损坏或初始化失败时明确报错。可用 `--no-git` 跳过 Git 操作。
- 单独识别脚本或取消候选选择不会初始化 Git。
- `--force` 同样执行 Git 设置；`--git` 要求目标目录自身的仓库，`--no-git` 则跳过。
- Console 模式在服务端信息前用白色显示一行通知，再空一行，说明仓库已创建或因已有仓库而跳过初始化，并标明目标路径；SSH 操作的结果也在本地显示。所有初始化成功后均在服务端信息后显示绿色 `Initialized successfully.`，包括使用 `--no-git` 时。Raw 输出仍只包含原有服务端信息，`--no-git` 不显示 Git 提示。`NO_COLOR` 关闭颜色。

`mcpm.json` 由 mcpm 管理，决定工作目录类型；Local 的 `server.json` 由 mcpm-backend 解析和持久化。没有 `mcpm.json` 的旧目录按 Local 处理。Local `server.json` 的 v1、v2 仍可读取；读取不改写文件，重新初始化时写为 v3，不再保存旧 `start` 命令快照。

完整指定 `--minecraft-version`、`--loader`，以及非 vanilla 所需的 `--loader-version`，会直接登记，不读取启动脚本。不完整时默认识别 `run.bat`（Windows）或 `run.sh`（Unix），并以已提供的字段筛选可靠候选。

识别只读取脚本和身份依据，不运行脚本、Java、安装器，也不下载文件。支持常见 Forge/NeoForge、Forge shim、Fabric 安装布局及带有版本信息的原版 JAR。静态识别支持明确赋值、引号、续行、目录切换、脚本引用和 Java 参数文件；动态条件、循环、管道、无法确定的变量和递归引用等布局可能无法识别。识别成功不保证服务端可启动；无法识别时应提供完整手动参数。

唯一候选直接登记；多个候选在 TUI 中使用上下键选择、Enter 确认、Esc/Ctrl-C 取消。TUI 要求本地 stdout/stderr 为终端且未指定 `--raw`/`--no-ui`；Console 和 Raw 模式要求唯一候选。stdout 重定向时自动使用 Raw。

本地导入保存所选候选的完整配置：

- 原命令中已有的 Java、内存、GC 和服务器设置在 `conf.toml` 中生效；没有指定的设置保持注释。
- 固定参数、入口、实际工作目录与明确的环境覆盖保存到 `launch.toml`。主类、JAR、模块入口各自保留原始含义；源码入口和无法确定参数边界的命令明确报错。
- 参数文件按 Java 规则静态展开，导入后不再依赖它们。修改来源脚本后需要显式重新导入。
- 原命令中的 `-javaagent` 必须指向已存在、可读取的文件，相对路径按原命令工作目录解释；缺失时导入失败，不自动下载。
- 根据 JAR manifest 中的 `Premain-Class` 识别 authlib-injector，支持重命名文件；将 agent 与可表达的认证属性移入 `conf.toml` 的 `[authlib-injector]`。导入后 `jar` 为手动路径，不复制文件、不生成托管 meta。其他 agent、未知或无法无损表达的属性保留原始参数；多个 authlib-injector agent 会报歧义。
- 首次初始化保留已有 `conf.toml`；重新导入必须 `--force`，替换 conf 额外需要 `--foverride-conf`。保留配置的语义错误或冲突会阻止发布。
- 后端统一发布注册和两份配置，并保存可恢复日志。中断后，下一次显式初始化先恢复旧文件；恢复未完成前禁止启动。
- 工作区类型标记仍由前端发布，标记失败会明确说明服务端文件已完成的状态。
- 成功后输出 [`server info`](info.md) 信息及文件变更汇总，具体设置直接查看配置文件。使用 [`launch`](launch.md) 启动。

### 远程行为

以下两条命令使用相同的全局 SSH 参数，作用不同：

```text
mcpm --ssh COMMAND [--ssh-mcpm EXE] init <REMOTE_PATH>
mcpm --ssh COMMAND [--ssh-mcpm EXE] init <PATH> --remote <REMOTE_PATH>
```

| 用法 | 位置参数 | `--remote` 参数 | 登记写入位置 |
|---|---|---|---|
| 不带 `--remote` | 远端服务端目录 | 无 | 远端目录，登记为 Local |
| 带 `--remote` | 本地工作目录 | 已登记的远端 Local Server 目录 | 本地目录，登记为 Remote |

不带 `--remote` 时，脚本、默认入口与元数据读写属于 SSH 主机，候选选择和输出在本地。可以使用原有识别参数或完整手动参数；已有 Remote 协议只登记身份，不传输新的配置草稿，不会生成两份本地启动配置。

带 `--remote` 时，只验证远端 Local Server 并保存本地连接，不初始化或修改远端。生成的 `.mcpm/mcpm.json` 为 `{"version":1,"type":"remote"}`，`.mcpm/server.json` 由 mcpm 管理，例如：

```json
{
  "version": 2,
  "ssh": "ssh user@host",
  "ssh_mcpm": null,
  "path": "/srv/minecraft"
}
```

`ssh_mcpm` 省略或为 `null` 时使用远端 PATH 中的 `mcpm-tools`。SSH 命令中的相对路径（例如 `-i keys/key`）以本地工作目录为基准；远端目录按远端系统解释。

后续 `server init/info` 和已支持远程的 `server mods list/info/deps/source` 在没有显式全局 SSH 参数时自动使用保存的连接。`server init ./workspace --force ...` 因此会重新登记远端 Local Server；若要修改本地保存的连接，应重新使用完整的 `--ssh ... server init ./workspace --remote ... --force`。显式 `--ssh` 优先，位置参数直接按远端路径处理；单独指定 `--ssh-mcpm` 仍然报错。

`mcpm-tools remote` 只打开 Local Server，目标为 Remote 工作目录时拒绝执行，禁止多段 Remote 引用。`server mods check/dep-graph` 暂不支持 Remote 工作目录。配置损坏或连接失败会明确报错，不回退为本地操作。
