> 尚未迁移到新文档体系。此处保留旧文档；已迁移命令以文档根目录中的新版本为准。

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
mcpm --ssh 'ssh user@windows' --ssh-mcpmd 'C:\mcpmd.exe' init ./workspace --remote 'D:\EDGW-26Summer'

# 创建工作目录后，后续命令自动使用保存的连接
mcpm info ./workspace
mcpm mods list --server ./workspace
```

## 参数解释

- `[PATH]`：已存在的目录，默认 `.`。普通初始化时是服务端目录；使用全局 `--ssh` 时是远端服务端目录；同时指定 `--remote` 时则是本地工作目录。
- `--remote <REMOTE_PATH>`：在本地 `PATH` 创建 Remote 工作目录，指向指定的远端目录。必须配合全局 `--ssh`；全局 `--ssh-mcpmd` 可选。目标必须已经是可打开的 Local Server。此选项与 `--start-script`、`--minecraft-version`、`--loader`、`--loader-version` 互斥。
- `--start-script <FILE>`：静态识别指定启动脚本。必须使用相对于实际服务端目录的路径，不接受绝对路径或越界路径。与所有手动版本及 Loader 参数互斥。
- `--minecraft-version <VERSION>`：Minecraft 版本，别名 `--mc-version`。信息完整时直接登记；不完整时作为自动识别候选的筛选条件。不能为空白。
- `--loader <LOADER>`：服务端加载器，支持 `forge`、`neoforge`、`fabric`、`vanilla`。已知原版必须显式填写 `vanilla`，不能通过缺少 Loader 证据推断原版。
- `--loader-version <VERSION>`：Loader 版本，与 Minecraft/FML 版本区分。模组加载器的完整手动登记需要此参数；`vanilla` 禁止指定此参数。不能为空白。
- `--force`：允许替换已有登记，包括损坏的 `server.json`。本地导入替换服务端元数据和 `launch.toml`，已有 `conf.toml` 逐字节保留；`--remote` 模式替换本地保存的连接配置。不能用它切换 Local/Remote 类型，也不能替换配置路径上的目录。
- `--foverride-conf`：与 `--force` 同时使用，在服务端所属主机重新生成 `conf.toml`，覆盖设置和注释；与 `--remote` 互斥。
- `--git`：要求实际服务端目录自身具有 Git 仓库，即使属于父目录仓库也会创建；已有仓库或 worktree 直接复用。与 `--no-git` 互斥。使用 SSH 或 `--remote` 时均在远端 Host 执行，不操作本机 stub 的 Git。
- `--no-git`：跳过实际服务端所属主机的 Git 检查和创建。Shallow 登记仍生成元数据忽略文件，但始终不创建本机 stub 的 Git 仓库。
- `--no-ui`：关闭候选选择 TUI，使用 Console 模式。多个候选必须通过更具体的识别条件消除歧义；与全局 `--raw` 同用时使用 Raw 模式。
- `--full-command`：存在旧命令快照时显示完整内容；新本地导入不在元数据中保存完整命令。

SSH 连接与远端 `mcpmd` 路径复用[全局参数](../README.md#全局选项)。

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

`mcpm-backend::workspace` 独占 `mcpm.json` 和 Remote `server.json` 的编解码；`mcpm-lib` 管理 Local 服务端元数据。缺少 `mcpm.json` 时按 Local 处理；兼容读取 Local v1/v2，读取不改写，显式初始化写入仅含身份的 v3。

完整指定 `--minecraft-version`、`--loader`，以及非 vanilla 所需的 `--loader-version`，会直接登记，不读取启动脚本。不完整时默认识别 `run.bat`（Windows）或 `run.sh`（Unix），并以已提供的字段筛选可靠候选。

识别只读取脚本和身份依据，不运行脚本、Java、安装器，也不下载文件。支持常见 Forge/NeoForge、Forge shim、Fabric 安装布局及带有版本信息的原版 JAR。静态识别支持明确赋值、引号、续行、目录切换、脚本引用和 Java 参数文件；动态条件、循环、管道、无法确定的变量和递归引用等布局可能无法识别。识别成功不保证服务端可启动；无法识别时应提供完整手动参数。

唯一候选直接登记；多个候选在 TUI 中使用上下键选择、Enter 确认、Esc/Ctrl-C 取消。TUI 要求本地 stdout/stderr 为终端且未指定 `--raw`/`--no-ui`；Console 和 Raw 模式要求唯一候选。stdout 重定向时自动使用 Raw。

配置导入在文件所属主机发布，包括 SSH 执行：

- 原命令中已有的 Java、内存、GC 和服务器设置在 `conf.toml` 中生效；没有指定的设置保持注释。
- 固定参数、入口、实际工作目录与明确的环境覆盖保存到 `launch.toml`。主类、JAR、模块入口各自保留原始含义；源码入口和无法确定参数边界的命令明确报错。
- 参数文件按 Java 规则静态展开，导入后不再依赖它们。修改来源脚本后需要显式重新导入。
- 原命令中的 `-javaagent` 必须指向已存在、可读取的文件，相对路径按原命令工作目录解释；缺失时导入失败，不自动下载。
- 根据 JAR manifest 中的 `Premain-Class` 识别 authlib-injector，支持重命名文件；将 agent 与可表达的认证属性移入 `conf.toml` 的 `[authlib-injector]`。导入后 `jar` 为手动路径，不复制文件、不生成托管 meta。其他 agent、未知或无法无损表达的属性保留原始参数；多个 authlib-injector agent 会报歧义。
- 首次初始化保留已有 `conf.toml`；重新导入必须 `--force`，替换 conf 额外需要 `--foverride-conf`。保留配置的语义错误或冲突会阻止发布。
- 后端统一发布注册和两份配置，并保存可恢复日志。中断后，下一次显式初始化先恢复旧文件；恢复未完成前禁止启动。
- backend operation 在服务端文件发布后写入工作区类型标记；标记失败会报告已经提交的文件状态。
- 成功后输出 [`server info`](info.md) 信息及文件变更汇总，具体设置直接查看配置文件。使用 [`launch`](launch.md) 启动。

### 远程行为

以下两条命令使用相同的全局 SSH 参数，作用不同：

```text
mcpm --ssh COMMAND [--ssh-mcpmd EXE] init <REMOTE_PATH>
mcpm --ssh COMMAND [--ssh-mcpmd EXE] init <PATH> --remote <REMOTE_PATH>
```

| 用法 | 位置参数 | `--remote` 参数 | 登记写入位置 |
|---|---|---|---|
| 不带 `--remote` | 远端服务端目录 | 无 | 远端目录，登记为 Local |
| 带 `--remote` | 本地工作目录 | 已登记的远端 Local Server 目录 | 本地目录，登记为 Remote |

不带 `--remote` 时，脚本、平台默认值、元数据、配置发布和 Git 都属于 SSH 主机；候选选择及输出留在本机。本地和远端调用同一套类型化 operation。

带 `--remote` 时，先由 Host operation 校验远端 Local Server，并在远端执行 Git 检查或创建；成功后，Shallow operation 仅发布本机连接 stub，不操作本机 Git。`.mcpm/mcpm.json` 为 `{"version":1,"type":"remote"}`，Remote `.mcpm/server.json` 格式如下：

```json
{
  "version": 3,
  "ssh": "ssh user@host",
  "ssh_mcpmd": null,
  "path": "/srv/minecraft"
}
```

`ssh_mcpmd` 省略或为 `null` 时使用远端 OS 的安装位置记录；文件不存在时发出 warning 并尝试远端 PATH 中的 `mcpmd`，fallback 也失败才报错。远端目录按远端系统解释；连接共享 link，不同服务端路径不产生新的 link。

后续 `server init/info` 和已支持远程的 `server mods list/info/deps/source` 在没有显式全局 SSH 参数时自动使用保存的连接。`server init ./workspace --force ...` 因此会重新登记远端 Local Server；若要修改本地保存的连接，应重新使用完整的 `--ssh ... server init ./workspace --remote ... --force`。显式 `--ssh` 优先，位置参数直接按远端路径处理；单独指定 `--ssh-mcpmd` 仍然报错。

Host operation 拒绝实际目标为 Remote 的目录，禁止链式引用。`server mods check/dep-graph` 和 launch 仍仅支持本地。未迁移的 mods/cache 保留 `mcpm-tools remote`；已迁移的 info/init 在远端仅需 `mcpmd`。配置或连接失败不回退本地执行。
