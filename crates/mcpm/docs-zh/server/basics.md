# 基础命令

登记与检查命令。目标选择见 [README](README.md)。

## `mcpm info`

```text
mcpm info [OPTIONS] [PATH]
```

### 参数

| 参数 | 说明 |
| --- | --- |
| `[PATH]` | 服务端目录。默认 `.`，即当前目录。未指定 `--ssh` 且 PATH 是已登记的 Remote workspace 时，改为使用登记中的远端服务端。 |

### 选项

| 选项 | 说明 |
| --- | --- |
| `--full-command` | 根据 `launch.toml` 和 `conf.toml` 生成完整启动命令。配置无效时返回错误；不检查可执行文件或 JAR 是否存在。 |

### 输出

#### Console 输出

- **服务端**：名称、路径、Minecraft 版本、Loader 及版本。
- **配置**：`launch.toml` 和 `conf.toml` 是否存在。
- **远端连接**：Remote 工作区额外显示远端路径、脱敏后的 SSH 命令和 mcpmd 路径。

#### RCON 状态

RCON 状态取自服务端的 `server.properties`：

- **未配置**：文件或设置缺失时显示 `disabled`。
- **配置无效**：RCON 字段值无效和 properties 文件解析失败分别报告。
- **密码脱敏**：仅保留首字符；单字符密码全部隐藏。

#### Raw 输出

Raw 使用无表头 TSV，字段依次为：

```text
名称 路径 Minecraft版本 Loader Loader版本 启动命令
```

Remote 工作区在这些字段前添加远端路径、脱敏后的 SSH 命令和 mcpmd 路径。未请求生成启动命令或缺少启动配置时，命令字段为 `unavailable`。

#### 登记异常

登记缺失或损坏时报错。旧登记按输出模式处理：

- **Console**：先提示执行 `mcpm info --upgrade`，再显示旧登记，以非零状态退出。
- **Raw**：报错，不输出登记内容。

旧登记不支持用 `--full-command` 生成启动命令。

### 示例

```sh
mcpm info ./server
mcpm info ./workspace
mcpm --ssh 'ssh user@host' info /srv/minecraft
mcpm info ./server --full-command
```

## `mcpm info --upgrade`

将 Local 登记从版本 1–3 升级到版本 4。普通 `info` 不改写登记。

```sh
mcpm info ./server --upgrade
mcpm --ssh 'ssh user@host' info /srv/minecraft --upgrade
```

### 登记格式

- **版本 4**：要求非空 `loader`，例如 `{"type":"vanilla"}`、`{"type":"unrecognized"}` 或 `{"type":"forge","version":"47.4.0"}`。已是版本 4 时无需升级。
- **版本 1–3**：仅用于检查和显式升级，统一按旧 V3 结构解析。`loader: null` 视为 Unrecognized；忽略历史 `start` 字段。其他服务端操作要求版本 4。
- **其他版本**：报错且不改写。

Remote 工作区的连接登记使用独立格式，保持不变。

### Vanilla 确认

旧登记已有具体 Loader 时保留原 ID 和版本。对于 Unrecognized 登记，按 `launch.toml` 的入口类型确定主类：

| 入口类型 | 主类来源 |
| --- | --- |
| JAR | 按 `process.workingDirectory` 定位 JAR，再读取 manifest。 |
| Class / Module | 使用显式 `mainClass`。 |

缺少 `launch.toml`、module 未显式指定主类或主类不匹配时，保持 Unrecognized。配置损坏或 JAR 不可读时报错。

发现原版入口时，提示确认（输入不区分大小写）：

```text
Set loader to Vanilla? [y(es)/n(o)/C(ancel)]
```

| 输入 | 结果 |
| --- | --- |
| `y`、`yes` | 写入 V4 Vanilla |
| `n`、`no` | 写入 V4 Unrecognized |
| `c`、`cancel`、Enter、Esc、Ctrl-C、EOF | 取消，登记不变 |

### 执行条件

- **输出模式**：显式升级在 Console 模式下执行。Raw 模式下拒绝升级，不写入。
- **远端目标**：在服务端主机检查和写入，本机负责确认。
- **确认有效期**：登记或启动证据发生变化时，本次选择失效。

### 升级结果

升级只修改 `.mcpm/server.json`。输出分别显示 Vanilla 和 Unrecognized，两者的 Loader 版本列均为空。升级后，`--full-command` 根据当前配置生成启动命令。

## `mcpm init`

为已有服务端登记，或创建指向已有服务端的 Remote 工作区。

在脚本识别和手动初始化模式中，`[PATH]` 默认为 `.`，按以下规则定位服务端：

| 调用方式 | 服务端位置 |
| --- | --- |
| 显式指定 `--ssh` | SSH 主机上的 `[PATH]`。 |
| 未指定 `--ssh`，且 `[PATH]` 是 Remote 工作区 | 登记中的远端服务端。 |
| 未指定 `--ssh`，且 `[PATH]` 是 Local 或未登记目录 | 本机的 `[PATH]`。 |

识别、Git 和写入在服务端主机执行，候选选择在本机进行。

### 1. 从启动脚本识别

```sh
mcpm init ./server
mcpm init ./server --start-script scripts/start.sh --no-ui
```

| 参数或选项 | 行为 |
| --- | --- |
| `[PATH]` | 已存在的服务端或工作区目录。 |
| `--start-script <FILE>` | 扫描指定的服务端相对路径，并跟踪支持的脚本引用。省略时，Windows 先试 `run.bat` 再试 `start.bat`，其他平台先试 `run.sh` 再试 `start.sh`；找到候选即停止。 |
| `--no-ui` | 使用 Console 编号列表，不打开 TUI 卡片。 |
| `--raw` | 禁用交互。 |
| `--force` | 已有服务端登记或 `launch.toml` 时必需。替换登记和 `launch.toml`，默认保留已有 `conf.toml`。 |
| `--foverride-conf` | 重新生成 `conf.toml`，覆盖设置和注释。要求 `--force`。 |
| `--git` / `--no-git` | 要求服务端目录自身具有 Git 仓库，或完全跳过 Git；两项互斥。 |
| `--full-command` | 显示按最终配置生成的完整启动命令。 |
| `--ssh <COMMAND>` / `--ssh-mcpmd <PATH>` | 显式指定服务端主机；此时 `[PATH]` 是远端路径。mcpmd 路径可省略，指定时要求 `--ssh`。 |

#### 启动命令扫描

- **脚本引用**：从入口脚本开始，跟踪可静态解析的调用，例如 `sh scripts/launch.sh` 或 `call scripts\launch.bat`。候选记录 Java 命令实际所在的脚本和行号。
- **不支持的语法**：跳过带有不支持运算符或命令替换的行，以及不支持的控制流块。后续命令如果依赖无法确定的工作目录或变量，则不能导入。
- **候选资格**：Java 命令必须既能确定服务端身份，又能生成启动配置草稿。某条命令提取身份或生成草稿失败时，其他候选仍可保留。

#### 服务端身份与版本

- **匹配一个 Loader**：生成一个候选，记录 Minecraft 版本、Loader 类型和 Loader 版本。
- **匹配多个 Loader**：每个 Loader 分别生成一个候选，由用户选择采用哪一种服务端身份。
- **未匹配 Loader**：仅当命令以 `java -jar` 启动、且 JAR 的主类是受支持的原版服务端入口时，才尝试识别原版版本。

原版 Minecraft 版本按以下顺序查找：

1. **`version.json`**：读取启动 JAR 内的版本 ID。
2. **`META-INF/versions.list`**：根据清单定位内嵌服务端 JAR，并核对其 `version.json`。
3. **离线 SHA-1 表**：前两种元数据都缺失时，对整个 JAR 计算 SHA-1，匹配 `18w47b` 之前未修改的官方服务端下载。

元数据损坏或不一致时，该命令报错，不回退到 SHA-1。用 `--log-level info` 可查看原版版本的识别依据。

#### 候选选择

只有一个候选时直接使用。多个候选按[输出模式](../README.md#输出模式)处理：

| 模式 | 选择方式 |
| --- | --- |
| TUI | 上下键切换，Enter 确认，Esc 或 Ctrl-C 取消。 |
| Console | 输入编号确认，空 Enter、Esc、Ctrl-C 或 EOF 取消。 |
| Raw | 列出候选并报错。改用 TUI 或 Console 选择，或提供完整手动身份。 |

Console 从控制终端读取输入，不读取重定向的 stdin。候选列表展示脚本位置、主类、入口和应用参数。

#### 配置导入

选定候选后才运行 Git 并写入文件。导入内容如下：

| 文件 | 内容 |
| --- | --- |
| `.mcpm/server.json` | v4 服务端身份。 |
| `.mcpm/mcpm.json` | Local 工作区类型。 |
| `.mcpm/.gitignore` | 元数据忽略规则。 |
| `launch.toml` | 入口、固定参数、工作目录和显式环境变量覆盖。 |
| `conf.toml` | 识别出的 Java、内存、GC 和服务端选项。 |

导入时展开 Java 参数文件。修改脚本后需重新导入。

#### Java Agent 识别与导入

每个 `-javaagent` 引用都按原 Java 命令的工作目录解析。目标必须是可打开的文件；缺失、不可读或指向目录时，该命令不能导入。

判断 Agent 类型时，检查 `-javaagent:<JAR>=<API_URL>` 指向的 JAR。manifest 的 `Premain-Class` 为 `moe.yushi.authlibinjector.Premain` 时识别为 authlib-injector，文件名可以不同。其他 Agent 保留原 `-javaagent` 参数及其选项。

识别出一个 authlib-injector 后：

- **JAR 和 API 地址**：写入 `conf.toml` 的 `[authlib-injector]`，分别使用 `jar` 和 `apiUrl`。继续引用已有 JAR。
- **支持的 JVM 属性**：仅出现一次且能写成配置字段的 `-Dauthlibinjector.*` 属性，一并导入 `[authlib-injector]`。
- **其余 JVM 属性**：重复、未知或无法写入配置的属性，保留原 JVM 参数。

同一命令中识别出多个 authlib-injector 时，无法将它们合并为一份配置，该命令不进入候选列表。

#### 配置发布与恢复

新启动配置与保留的 `conf.toml` 冲突时，发布失败。配置发布中断后，重新运行 `init` 恢复，再尝试[启动服务端](launch.md)。

### 2. 使用完整身份手动初始化

```sh
mcpm init ./server --minecraft-version 1.21.1 --loader vanilla
mcpm init ./server --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
mcpm --ssh 'ssh user@host' init /srv/minecraft --minecraft-version 1.21.1 --loader vanilla --no-git
```

| 参数或选项 | 行为 |
| --- | --- |
| `[PATH]` | 已存在的服务端或工作区目录。 |
| `--minecraft-version <VERSION>` | 必填，别名 `--mc-version`。 |
| `--loader <LOADER>` | 必填：`vanilla`、`fabric`、`forge` 或 `neoforge`。 |
| `--loader-version <VERSION>` | 模组服务端必填，`--loader vanilla` 禁用。这里填加载器版本，不是 Minecraft 或 FML 版本。 |
| `--force` | 替换服务端登记，并移除已有的 `launch.toml`。默认保留已有 `conf.toml`。 |
| `--foverride-conf` | 重新生成 `conf.toml`，覆盖设置和注释。要求 `--force`。 |
| `--git` / `--no-git` | 要求服务端目录自身具有 Git 仓库，或完全跳过 Git；两项互斥。 |
| `--full-command` | 若有可用启动配置，则显示完整启动命令。 |
| `--ssh <COMMAND>` / `--ssh-mcpmd <PATH>` | 显式指定服务端主机；此时 `[PATH]` 是远端路径。mcpmd 路径可省略，指定时要求 `--ssh`。 |

身份选项缺项时报错。

手动初始化只写入 Local 登记和设置，不生成 `launch.toml`。要导入启动配置，请使用脚本识别模式。

### 3. 登记 Remote 工作区

```sh
mcpm --ssh 'ssh user@host' init ./workspace --remote /srv/minecraft --git
mcpm --ssh 'ssh user@new-host' init ./workspace --remote /srv/new-server --force --no-git
```

| 参数或选项 | 行为 |
| --- | --- |
| `[PATH]` | 已存在的本地工作区目录，默认 `.`。 |
| `--remote <REMOTE_PATH>` | 必填：SSH 主机上已登记为 Local Server 的服务端目录。 |
| `--ssh <COMMAND>` / `--ssh-mcpmd <PATH>` | 必须显式指定 `--ssh`；可选指定远端 mcpmd 路径。 |
| `--force` | 替换已有 Remote 工作区保存的连接和服务端路径；不能将 Local 工作区转成 Remote。 |
| `--git` / `--no-git` | 要求远端服务端目录自身具有 Git 仓库，或完全跳过 Git；两项互斥。 |
| `--full-command` | 若远端服务端有可用启动配置，则显示完整启动命令。 |

登记顺序：

1. 在 SSH 主机上校验目标是已登记的 Local Server，并准备 Git。
2. 在本机写入连接登记：SSH 命令、可选 mcpmd 路径和远端目录。

要沿用已保存的连接初始化服务端，请不带 `--remote`，使用脚本识别或手动初始化模式。

### Git 默认行为

优先复用服务端目录自身、父目录或 worktree 的 Git 仓库；均不存在才在服务端目录创建。`--force` 不跳过 Git。不会自动暂存或提交文件。

### 写入限制

- **替换范围**：`--force` 不能切换工作区类型，也不能替换占用 `.mcpm/server.json` 路径的目录。
- **中途失败**：发布或连接中断时，部分文件可能已经写入。重试前请检查目标。

全局输出和日志开关见[通用选项](../README.md#通用选项)。
