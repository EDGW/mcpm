# 启动与连接

`mcpm launch` 启动 Minecraft 服务端；`mcpm attach` 连接已经运行的服务端。两者使用同一套日志、状态和命令界面。

## 快速开始

```sh
mcpm init ./server --start-script run.sh
mcpm launch --server ./server
```

需要在后台运行时：

```sh
mcpm launch --server ./server --detach
```

命令输出运行 ID 后返回；之后可使用 `mcpm attach --server ./server` 重新连接。

## `mcpm launch`

```text
mcpm launch [OPTIONS]
```

读取 `launch.toml` 和 `conf.toml`，生成 Java 命令并启动。启动前会检查 Java、准备认证和必要的文件；启动完成后默认进入交互界面。

服务端已有运行实例时，`launch` 报错；请先 `attach` 并停止旧实例。

### 选项

#### 目标

| 选项 | 说明 |
| --- | --- |
| `--server <PATH>` | 已登记的服务端目录，默认当前目录。 |

#### 启动覆盖

| 选项 | 说明 |
| --- | --- |
| `-j`, `--java <EXECUTABLE>` | 指定 Java 可执行文件。 |
| `-m`, `--memory <[INITIAL:]MAXIMUM>` | 指定最大堆；也可同时指定初始堆。 |
| `-G`, `--gc <COLLECTOR>` | 指定垃圾回收器。 |
| `-p`, `--port <PORT>` | 指定服务器端口。 |
| `--world <DIRECTORY>` | 指定世界目录。 |
| `-E`, `--extra <ARGUMENT>` | 追加一个 JVM 参数。 |
| `-O`, `--override <KEY=VALUE>` | 覆盖任意配置字段。 |

详细语法见[临时覆盖](#临时覆盖)。

#### 认证选项

| 选项 | 说明 |
| --- | --- |
| `-A`, `--auth <API_URL>` | 指定本次启动使用的 authlib-injector API 地址。 |
| `--auth-jar-policy <POLICY>` | 托管认证 JAR 的获取策略：`auto`、`local`、`refresh`。 |

认证配置见[认证](#认证)。

#### 运行与界面

| 选项 | 说明 |
| --- | --- |
| `-c`, `--communication <MODE>` | 命令通道：`auto`（默认）、`rcon` 或 `stdin`。 |
| `-D`, `--detach` | 启动后输出运行 ID 并返回，不进入交互界面。 |
| `--no-ui` | 使用 Console/Raw 输出，不显示 TUI 输入区。 |
| `--input-height <LINES>` | TUI 输入区初始高度，最小 3 行，默认 4。 |

通用选项 `--ssh`、`--ssh-mcpmd`、`--raw`、`--log-level`、`--remote-logs` 见 [README](../README.md)。

### 示例

```sh
mcpm launch --server ./server
mcpm launch --server ./server --detach
mcpm launch --server ./server --communication rcon
mcpm --ssh 'ssh user@host' launch --server /srv/minecraft
```

### 临时覆盖

临时覆盖只影响本次启动，不会写回 `conf.toml`。

最常用的方式是直接使用快捷选项：

```sh
mcpm launch --server ./server -j /opt/jdk/bin/java -m 1G:4G -p 25566
mcpm launch --server ./server -G g1 --world another-world
mcpm launch --server ./server -E '-Dfile.encoding=UTF-8'
```

任意配置字段都可以通过 `-O` 覆盖：

| 写法 | 行为 |
| --- | --- |
| `-Okey=value` | 设置标量；列表追加一项或一个数组。 |
| `-Okey+=value` | 追加列表项或数组。 |
| `-Okey:=value` | 清空列表，再设置新值；省略值表示清空。 |

示例：

```sh
mcpm launch --server ./server -Ojvm.threadStack=2M
mcpm launch --server ./server '-Ojvm.extraArguments:=["-Dfirst=1"]'
mcpm launch --server ./server '-Ojvm.extraArguments=-Dsecond=2'
```

配置键使用 `conf.toml` 中的完整路径和大小写。值按 TOML 解析；也可以直接写不带引号的字符串。Shell 参数包含空格时，应为整个参数加引号。

同一字段只能设置一次；快捷选项与 `-O` 指向同一字段时也会报错。

### 配置文件

| 文件 | 内容 |
| --- | --- |
| `launch.toml` | JVM 参数、入口、固定应用参数、工作目录和环境变量。 |
| `conf.toml` | Java、内存、GC、认证、Minecraft 启动参数和用户附加参数。 |
| `.mcpm/server.json` | Minecraft 和 Loader 身份登记。 |

`launch` 从 `--server` 指定的目录读取 `launch.toml` 和 `conf.toml`；任一文件缺失时，`launch` 报错。

`launch.toml` 通常由 `mcpm init` 根据启动脚本生成。`conf.toml` 保存用户的长期设置，可直接编辑。

```toml
# launch.toml：JAR 服务端
[entry]
type = "jar"
path = "server.jar"
```

```toml
# conf.toml：Java 与内存设置
[java]
executable = "java"

[jvm.heap]
initial = "1G"
maximum = "4G"

[server]
nogui = true
```

修改启动脚本后，执行 `mcpm init --force` 重新生成启动配置。`--force` 默认保留已有的 `conf.toml`。需要同时重建 `conf.toml` 时，再加 `--foverride-conf`。

### 路径与参数

- `process.workingDirectory` 指定 Java 的工作目录；省略时使用服务端根目录。
- `java.executable` 写裸名称时从 `PATH` 查找；写相对路径时从服务端根目录解析。
- 参数数组中的每个字符串对应一个 argv，不经过 Shell 展开。
- 入口支持 `class`、`jar` 和 `module`。
- `server.port` 和 `server.world` 只生成启动参数，不修改 `server.properties`；服务端决定是否接受这些参数。
- 内存和 GC 设置中的显式 `auto` 会返回错误；省略这些字段时由 JVM 决定。

### 启动进度

启动准备阶段在 stderr 显示当前步骤，包括读取配置、检查 Java、准备认证和启动 Java。终端支持动态绘制时，进度在同一区域更新；下载会显示来源、进度和速度。`--raw` 或 stderr 重定向时改为逐行输出。

Java 启动后清除进度区域，进入交互界面或返回运行 ID。准备完成只表示 Java 进程已经创建；Minecraft 是否就绪由游戏自身决定。

### 认证

在 `conf.toml` 中添加 API 地址即可启用 authlib-injector：

```toml
[authlib-injector]
apiUrl = "https://example.com/api/yggdrasil/"
```

| 字段 | 行为 |
| --- | --- |
| `jar` 省略或为空 | mcpm 管理认证 JAR，自动选择本地版本或下载。 |
| `jar = "libraries/custom-agent.jar"` | 使用指定文件，路径相对于服务端根目录。只检查文件存在且可读。 |

`-A` 只覆盖本次启动的 API 地址；其他认证设置保持不变。

`--auth-jar-policy` 控制托管 JAR：

| 策略 | 行为 |
| --- | --- |
| `auto`（默认） | 本地文件有效时直接使用；缺失、损坏或摘要不匹配时下载最新发布。 |
| `local` | 只使用本地文件；需要修复时直接报错。 |
| `refresh` | 重新下载最新发布。 |

- `--auth-jar-policy` 只适用于托管 JAR。使用手动 JAR 或未启用认证时，指定策略会报错。
- 下载或校验失败会阻止启动，已有文件保持不变。
- 取消启动会停止下载并清理临时文件。

## `mcpm attach`

```text
mcpm attach [OPTIONS]
```

连接正在运行的服务端。连接后可查看日志、状态并发送命令。

默认回放最近一屏日志，之后继续接收实时输出。使用 `--history-lines 0` 时只显示实时输出。实例已经退出时，`attach` 报错；请使用 `launch` 创建新实例。

### 选项

| 选项 | 说明 |
| --- | --- |
| `--server <PATH>` | 已登记的服务端目录，默认当前目录。 |
| `--history-lines <LINES>` | 回放的历史行数；默认使用终端高度，无法取得时为 24。 |
| `--input-height <LINES>` | TUI 输入区初始高度，最小 3 行，默认 4。 |
| `--no-ui` | 使用 Console/Raw 输出，不显示 TUI 输入区。 |

### 示例

```sh
mcpm attach --server ./server
mcpm attach --server ./server --history-lines 100 --input-height 5
mcpm --ssh 'ssh user@host' attach --server /srv/minecraft
```

## 输出模式

| 模式 | 选择方式 | 输出方式 |
| --- | --- | --- |
| TUI | 终端默认 | 底部输入区；日志显示在终端上方历史区。 |
| Console | `--no-ui` | 普通行式输出，按行输入命令。 |
| Raw | `--raw`，或 stdout 被重定向 | 关闭交互、格式和颜色，输出原始内容。 |

TUI 要求 stdout 和 stderr 都是终端。Java stdout 作为游戏日志；stderr 和 RCON 响应会单独标识。诊断、进度和提示写入 stderr，不占用 stdout。

## 命令输入与提示

输入非空内容时，界面显示命令提示：

- Tab 填入候选；Enter 直接发送当前输入。
- 提示最多显示五个候选，完整匹配优先，长语法自动换行。
- 输入为空时，↑/↓ 浏览最近提交的命令；开始编辑后回到提示模式。
- 识别到 `/gamerule` 语法时，提供规则名和值提示。未知规则标红，Enter 仍按原文发送。
- 通过 RCON 连接后自动加载服务器 help；加载失败时提示 `Failed to fetch help`。
- Console 和 Raw 按行输入，不显示提示面板。

## 内置命令

| 命令 | 行为 |
| --- | --- |
| `@detach`、Ctrl+D、stdin EOF | 断开当前界面，服务端继续运行。 |
| `@stop` | 发送普通 stop。 |
| `@kill` | 立即强制终止 Java。 |
| `@speed` | 切换速率显示的简洁模式和完整模式。 |
| `@@detach`、`@@stop`、`@@kill`、`@@speed` | 把对应命令原样发送给服务端。 |

断开界面时，mcpm 会等待已提交命令的结果输出完毕，再结束当前客户端。

第一次 Ctrl+C 发送普通停止；停止过程中再次按 Ctrl+C 会强制终止。强制终止没有自动超时。Java 的退出码会作为命令结果返回。

## 速率显示

在 TUI 中输入 `@speed` 切换状态栏下方的速度行。

- 简洁模式显示最近 100ms 的平均收发速率。
- 完整模式显示 100ms 和 1000ms 的峰值与平均值。
- Console 和 Raw 没有持续速度行；输入 `@speed` 时输出一次当前快照。

## 多人同时连接

多个客户端可以同时连接同一服务端并发送命令。mcpm 按顺序执行普通命令。

- 其他客户端的结果带绿色边框和 `Executed by another Listener` 标签。
- Console 同样显示边框；Raw 只保留来源标签。
- 新连接只接收连接之后的命令结果，不回放旧结果。
- 客户端落后时，界面提示丢失的结果数量，服务端继续运行。

## 日志

- Java stdout 和 stderr 分别标记；两个输出流都持续接收。
- 日志保留最近一段；更早的内容会提示缺失。
- 回放位置前会显示 `History` 分割线。
- 时间戳显示为灰色；级别块按 INFO、WARN、ERROR、DEBUG、TRACE 着色；正文在 WARN 及以下为灰色。
- RCON 响应带边框，标题为 `RCON: 命令名`。
- Raw 保留 Java stdout 原始字节，日志着色、历史分割线和 RCON 边框只在其他模式下显示。
- `NO_COLOR` 关闭颜色。

## 状态与 RCON

底部状态栏显示 Java 生命周期、服务器通信状态和实际命令通道。

| 通信通道 | 行为 |
| --- | --- |
| `auto`（默认） | 优先使用 RCON；连接或认证失败时改用 stdin。 |
| `rcon` | 使用已有 RCON 配置；连接或认证失败时改用 stdin。 |
| `stdin` | 只使用 Java stdin。 |

- RCON 需要服务端已有配置。
- RCON 命令同步返回完整响应；stdin 只确认命令已发送，响应从 Java stdout 查看。
- 命令发送后断线或超时，结果标记为未知，不会重放。
- 认证成功时显示 `Server RCON Connected`，该状态不代表 Minecraft 已就绪。
- 未启用 RCON、配置缺失或使用 `stdin` 时，状态显示 `Server Unknown`。
- 状态连接丢失时，`attach` 报错并显示状态未知。
- 连接失败只表示暂时无法访问，Java 可能仍在运行。
- 同一服务端只允许一个运行实例。实例标识冲突时，新启动会被拒绝。
