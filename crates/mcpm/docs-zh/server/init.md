## `mcpm init`

为现有服务端建立 Local 登记，或者执行 `server init --remote`：先在 SSH 主机上的 R 目录完成服务端识别与初始化，再在调用方机器上的 P 目录登记 Remote placeholder。此命令不安装服务端，也不启动游戏进程。

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

# 在 host R 上识别/初始化服务端，并在 caller P 上登记 Remote placeholder
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm-tools.exe' init ./workspace --remote 'D:\EDGW-26Summer' --start-script run.bat

# 创建工作目录后，后续命令自动使用保存的连接
mcpm info ./workspace
mcpm mods list --server ./workspace
```

## 参数解释

- `[PATH]`：已存在的目录，默认 `.`。普通初始化时是服务端目录；使用全局 `--ssh` 时是 host 上的 R；同时指定 `--remote` 时是调用方机器上的 P placeholder。
- `--remote <REMOTE_PATH>`：在 host 的 R 上执行识别与服务端注册，然后在 caller 的 P 上登记 Remote workspace。必须配合全局 `--ssh`；`REMOTE_PATH` 由 host 平台解释。
- `--start-script <FILE>`：传给 host recognition；`--remote` 下允许使用，空值时由 host 平台选择默认脚本。
- `--minecraft-version`、`--loader`、`--loader-version`：传给 host recognition；完整组合直接在 host 注册，不完整组合由 host 识别并由 caller 选择候选。
- `--force`：只控制 host R 的 server registration/config replacement；不能替换 caller P 的 Remote registration。
- `--replace`：只控制 caller P 的 Remote registration replacement；仅对 `--remote` 有效。
- `--foverride-conf`：host R 重新生成 `conf.toml`，仍要求 `--force`，可与 `--remote` 同用。
- `--git` / `--no-git`：只作用于 host R；caller P 不执行 Git。
- `--no-ui`：关闭候选选择 TUI。多个候选必须通过更具体的 host recognition 条件消除歧义。
- `--full-command`：存在旧命令快照时显示完整内容；新本地导入不在元数据中保存完整命令。
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

`--remote` 有两个明确目录：P 是调用方机器上的 Remote workspace placeholder，只保存连接登记；R 是 SSH host 上的服务端目录，所有 recognize、`server.json`、`conf.toml`、`launch.toml`、配置发布和 Git detect/init 均在 R 上执行。

```text
mcpm --ssh COMMAND [--ssh-mcpm EXE] init P --remote R [recognition options] [--force] [--replace]
```

完整手动元数据直接在 R 上注册；否则在 R 上识别，候选选择在 caller 执行。空 `--start-script` 由 host 平台选择默认脚本，不能使用 caller 平台推断。

`--force` 只替换 host registration/config；`--replace` 只替换 P 的 Remote 登记。`--git` 与 `--no-git` 只作用于 R。P 的 Remote registration 文件是 `.mcpm/remote_info.json`，`.mcpm/server.json` 仍只属于 Local server metadata。

如果 host 初始化成功但 P 登记失败，不回滚 R；命令会明确报告 host 已完成、P 尚未登记，并提示使用 `--replace` 重新登记或按现有 workspace registration 能力手工登记。
