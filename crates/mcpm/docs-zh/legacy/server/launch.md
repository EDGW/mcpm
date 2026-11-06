> 尚未迁移到新文档体系。此处保留旧文档；已迁移命令以文档根目录中的新版本为准。

# `mcpm launch` / `mcpm attach`

从服务端根目录配置准备不可变 Java 命令，由独立的 `mcpmd shim` 持有 Java。`launch` 默认 attach；客户端退出不拥有 Java 的停止权。

```sh
mcpm init ./server --start-script run.sh
mcpm launch --server ./server
mcpm-server launch --server ./server
mcpm-tools server launch --server ./server
```

- `--server <PATH>`：执行主机上的已登记服务端目录，默认 `.`；不接受路径位置参数。显式 SSH 优先，否则遵循 Local/Remote 工作区登记。
- 支持 TUI、Console（`--no-ui`）、Raw（`--raw` 或 stdout 重定向）；TUI 要求 stdout/stderr 均为可绘制终端。
- 支持 Local、显式 SSH 和 Remote 工作区；不自动重启。
- 不执行原始脚本，不重新读取原始 Java 参数文件，不安装服务端依赖或接受 EULA；启用托管认证时可下载 authlib-injector。


## 连接、命令与退出

```sh
mcpm launch --server ./server --detach
mcpm attach --server ./server
mcpm attach --server ./server --history-lines 100 --input-height 5
mcpm --ssh 'ssh user@host' launch --server /srv/minecraft
mcpm launch --server ./server --communication rcon
```

- `--detach` 在 shim 接管 Java 后打印 run ID 并返回，不代表 Minecraft 已就绪。已有实例时 launch 报错，不静默忽略新的覆盖参数；attach 不启动 Java。
- 默认命令通道 `auto` 优先 RCON。`--communication rcon` 要求启动前已有配置；两种模式在连接或认证失败时均 fallback 到 Java stdin，界面显示实际通道。`stdin` 不尝试 RCON。配置从实际工作目录读取，支持应用参数 `--config` / `-c`。
- 不自动启用或修改 RCON。RCON 命令同步返回完整响应；stdin 只报告已发送，响应见 Java stdout。发送后断线/超时为 unknown，不重放同一命令。
- 只解释 RCON 所需的 properties 字段；无关字段的非法值或坏转义不影响提取。RCON 未启用时，仅设置 `rcon.port` 不会连接或自动启用它。
- 默认日志来自 Java stdout；stderr 与 RCON 响应分别标识。stdout/stderr 各打开持续 Channel，不轮询；来源关闭后 Channel 结束，但 Java 可能仍在运行。Raw stdout 保留原字节；其他来源与诊断写 stderr。日志尾部有界，落后返回 Gap。
- 日志读取块和单批正文上限均为 1 MiB，有数据即推送，不等待攒满；块按原始字节划分，不保证一块一行。stdout/stderr 各保留最多 1 MiB 尾部，超出部分通过 Gap 明确报告。Java 退出后的保留尾部以原始二进制保存，JSON 只保存状态和游标元数据，不再存字节数组；命令去重记录独立于日志尾部。
- 默认使用底部 Inline TUI，不进入全屏备用屏幕；日志进入上方终端原生 scrollback。输入框初始占 4 行，`--input-height <LINES>` 设置高度（至少 3 行）；macOS 用 Option+↑/↓，其他平台用 Alt+↑/↓。扩展后收缩会恢复已输出日志的位置，不向 scrollback 追加重复日志。标题和输入内容左右各有一个空格。左右/Home/End 编辑，Enter 发送。
- 内置命令单独占一行：`@detach`（或 stdin EOF、Ctrl+D）只退出客户端，不停止 Java；`@stop` 发送普通 stop，不会升级为强杀；`@kill` 直接请求强杀 Java，独立于普通命令队列；`@speed` 切换本地速率展示。`@@detach`、`@@stop`、`@@kill`、`@@speed` 向服务器发送对应的字面命令。已提交命令先报告结果，再结束客户端。
- 第一次 Ctrl+C 发送普通 `stop` 命令；权威状态为 Stopping 时再次 Ctrl+C 强杀 Java 进程组/Job。没有自动十秒强杀；手动输入 `stop` 也不升级为 kill。
- 多个 Listener 可同时 attach、提交命令；shim 串行执行普通命令，不再独占控制权。所有已连接 Listener 通过独立结果 Channel 接收用户命令的响应或失败结果；发送者也只从该 Channel 展示一次。自己执行的命令保留 Primary 框，其他 Listener 的命令使用绿色框，标题注明 `Executed by another Listener · <id>`。Console 同样展示边框；Raw 不着色、不画框，但保留来源标注。
- 命令结果订阅只接收连接后的结果，重新 attach 不回放旧命令；慢 Listener 落后时明确提示丢失数量，不阻塞 Java。stdin 结果只确认已发送，实际游戏输出仍来自共享 Java 日志；`@kill` 独立于普通命令队列，其请求也通知所有 Listener。
- 状态使用一个持续 Channel：订阅先收到当前快照，状态或命令通道变化时立即推送，默认每秒再发送一次当前快照。默认连续 5 秒无快照，或未收到最终退出状态就断开，attach 明确报错；不推断 Java 已退出，也不自动 stop。
- 底部状态栏分开显示 Java 生命周期、服务器通信状态和实际命令通道。`--communication rcon` 与检测到可用 RCON 配置的 `auto` 共用探测、连接复用和状态判定；认证成功显示 `Server RCON Connected`，不代表 Minecraft 已就绪。`auto` 未启用 RCON、配置缺失或无法读取时，以及显式 `stdin`，显示 `Server Unknown`。
- 使用 RCON（显式 `rcon` 或已配置 RCON 的 `auto`）时，Java 存活、尚未尝试发送 stop 且从未认证成功，后台每秒尝试连接一次；每次尝试完成后等待一秒，不堆叠超时中的尝试。探测只做连接与认证，不执行游戏命令。首次成功后保留连接供命令复用；观察到断开后改为 Unknown，不恢复首次连接探测。stop 可能发出后或 Java 退出后也为 Unknown。用户后续主动提交命令仍按原通道选择与 fallback 规则处理，失败命令不重放。
- 每次 attach 首次观察到 RCON Connected 后，自动执行一次真实 `help`，结果仅保存在当前前端会话内存中，不向其他 Listener 广播，也不输出自动 help 响应卡片；detach 后丢弃，重新 attach 会重新获取。用户手动输入 `help` 则正常广播。执行失败或未识别到命令时，游戏命令提示区显示 `Failed to fetch help`，本次会话不自动重试。内置 `@detach`、`@stop`、`@kill`、`@speed` 提示不依赖 help。
- 仅在存在非空、非纯空白输入时显示 Inline 提示框；清空或发送输入后隐藏。提示区使用灰色上边框和左右边框，下边框复用 Command Input，空白区域不会透出日志。按命令名前缀匹配，参数只作原样语法提示，不参与匹配；同屏至多五个候选，长命令和参数可自动换为多行，非历史回溯状态时上下键遍历全部匹配项并滚动，标题按候选显示当前项/总数。Tab 填入所选命令名（未选择时填第一项），保留已有参数；Enter 始终直接发送当前输入，不接受提示。命令名匹配部分用 Primary 粗体，余下部分用 Primary 常规字重，游戏命令参数用常规输出色，内置命令描述用灰色。`/wp -> waypoint` 保留箭头提示，并在找到目标时附上目标参数语法；填入仍使用别名。Console/Raw 保持按行输入，不绘制交互提示。
- 提示排序：命令名完整匹配项优先，其余前缀匹配项保持原始帮助列表中的顺序。
- Java 退出后 shim 排空管道并结束，Host 可读取有限的最终日志与退出状态；shim 不驻留成空服务。连接失败不等于 Java 已退出，不按发现记录中的 PID 杀进程。
- `.mcpm/run/` 中的发现和最终状态不保存绝对服务器路径。目录移动后用新路径 attach/launch；运行中的副本不会获得原目录的 OS 锁，拒绝当成原实例操作。活动目录能否改名仍受操作系统限制。
- 命令去重记录只保留到该次运行回收，不长期积累历史数据库。endpoint 碰撞明确拒绝启动或操作，不切换到其他实例。
- Inline TUI 使用 ratatui 标准 viewport、Block/Paragraph 和主题组件。`--log-level` 的终端诊断经有界 Channel 插入输入框上方，不绕过绘制；文件日志独立写入。`--no-ui` 使用 Console。
- 同版本 `mcpmd` 应与 mcpm 放在同一目录，或可从执行主机的 PATH 找到。

### 传输速率 / `@speed`

- Inline TUI 在状态栏下方单开速度行，复用现有的 100ms 重绘节奏。
  默认展示 100ms AVG，例如 `10.4KB/s(↑0.4KB/s ↓10.0KB/s)`。
- 输入 `@speed` 在简洁和 full 模式间切换。full 分两行，窄终端自动换行，
  不挤占配置的输入区高度；终端空间不足时受可用行数限制：

  ```text
   100ms: P 12.4KB/s(↑0.4KB/s ↓12.0KB/s) · AVG 10.4KB/s(↑0.4KB/s ↓10.0KB/s)
  1000ms: P 12.4KB/s(↑0.4KB/s ↓12.0KB/s) · AVG 10.4KB/s(↑0.4KB/s ↓10.0KB/s)
  ```

- `↑` 为客户端发送，`↓` 为客户端接收，前面的总量为二者之和。KB 按 1000 字节换算，
  保留一位小数；AVG 始终除以完整窗口长度，包括启动不足一个窗口时。
- P 是 Session 创建以来各方向的窗口平均速率峰值。P 的总量是两个独立方向峰值之和，
  不保证同时发生，不是最近 1s 内的峰值；空闲后 AVG 归零，历史峰值保留到 Session 结束。
- 前端只调用 `Executor::traffic_stats()`，由其转交客户端 Session 的快照，不自行采样、
  不发统计 RPC。统计所有 Session Channel，包含诊断、协议字段等，不等同于纯游戏流量
  或底层 SSH 线路流量。
- 本地执行没有 mcpm-exec Session，显示 `Speed unavailable (no messaging Session)`，
  不用私有 shim IPC 代替该口径。Console/Raw 没有持续状态栏，仅在输入 `@speed` 时将
  新选中模式的当前快照输出到 stderr。
- `@speed` 不提交到服务器；`@@speed` 才发送字面命令。重新 attach 默认回到简洁模式。

### 命令回溯与前导空格

- Inline 输入为空时，↑ 调出最近提交的命令，随后 ↑/↓ 浏览更早/更新的历史；
  到达最旧项后 ↑ 不再移动，越过最新项的 ↓ 恢复空输入。
- 编辑或 Tab 补全后退出历史回溯；其他非空输入仍用 ↑/↓ 选择命令提示。
  历史调出的是可编辑副本，不修改原记录，Enter 提交当前内容。
- 只保存本次 attach 提交的非空输入，包括本地内置命令和原始 `@@` 转义，保留原文与空格。
  历史只在前端内存中保存，detach 后丢弃；不从后端、日志、自动请求或其他 Listener 获取。
- 前导空格使用警告色背景标记，Command Input 标题同步提示 Minecraft 可能无法正确识别
  该命令；删除前导空格后警告消失。参数之间的空格不触发警告，不自动裁剪或改写发送内容。
- 两项交互都只属于前端 Inline 界面，Console/Raw 仍保持按行输入。

### Attach 历史与日志颜色

- 独立 `attach` 默认回放当前终端高度对应的历史行数；无法取得终端尺寸时为 24 行。`--history-lines N` 调整数量，`0` 直接跟随实时输出。
- 回放的第一行前绘制 `History` 分割线；回放到连接时的日志位置后，继续消费同一个持续 Channel，不重新订阅或重复打印。可回放范围仍受 shim 有界尾部限制。
- 仅前端识别行首的 `[时间或日期时间] [线程/级别]`，线程及其 `/` 可省略。支持 `HH:mm:ss`（含小数秒）、ISO 日期时间及 Forge 的 `ddMMMyyyy HH:mm:ss.SSS`。
- 时间戳灰色，仅第二个级别块着色：INFO 为成功绿，WARN/WARNING 为警告黄，ERROR/FATAL 为错误红，DEBUG 用 Secondary，TRACE 用弱化色。识别出的 TRACE、DEBUG、INFO、WARN/WARNING 正文灰色；ERROR/FATAL 正文和未匹配行不增加样式。样式仅作用于本行；`NO_COLOR` 禁用颜色。格式依据见 [Minecraft/Forge Log4j 示例](https://docs.spongepowered.org/stable/en/server/spongineer/logs.html)。
- RCON 响应以 Primary 色标准边框插入控制台，标题为 `RCON: 命令名`：`RCON:` 非粗体，实际提交命令的第一个词为粗体，参数不放进标题；标题左右各一个空格。Console 模式同样有边框，游戏日志不会被装进响应框。
- Raw 不进行日志着色、不绘制历史分割线或 RCON 边框，Java stdout 保留原字节；历史行数选择仍生效。


## 本次启动覆盖

覆盖值只对本次启动生效，不修改 `conf.toml`。

```sh
mcpm launch --java /opt/jdk/bin/java -m 1G:4G -G g1 --port 25566 --world another-world
mcpm launch -m 4G -Ojvm.threadStack=2M -E '-Dfile.encoding=UTF-8'
mcpm launch '-Ojvm.extraArguments:=["-Dfirst=1"]' '-Ojvm.extraArguments=-Dsecond=2'
```

| 参数 | 对应配置及行为 |
| --- | --- |
| `-O<key>=<value>` | 覆盖标量；列表追加一项或一个数组 |
| `-O<key>+<value>`、`-O<key>+=<value>` | 追加列表项或数组 |
| `-O<key>:=<value>` | 清空列表，再设置当前项或数组；省略值表示清空 |
| `--java <EXECUTABLE>` | `java.executable` |
| `-m/--memory <INITIAL>:<MAXIMUM>` | `jvm.heap.initial` 和 `jvm.heap.maximum` |
| `-m/--memory <MAXIMUM>` | 只覆盖 `jvm.heap.maximum`，保留已有 initial |
| `-G/--gc <COLLECTOR>` | `jvm.gc.collector`：serial、parallel、g1、zgc、shenandoah、epsilon |
| `--port <PORT>` | `server.port` |
| `--world <DIRECTORY>` | `server.world` |
| `-A/--auth <API_URL>` | `authlib-injector.apiUrl`；未配置认证表时，为本次启动启用最小托管配置 |
| `-E/--extra <ARGUMENT>` | 在所有 `-O` 操作之后，向 `jvm.extraArguments` 追加一项 |

- 配置键使用 `conf.toml` 的完整路径和大小写；不支持覆盖 `launch.toml`。
- 值支持 TOML 字符串、数值、布尔值、数组，也可直接写不带引号的字符串。Shell 中包含空格或数组引号时，应给整个参数加引号。
- 列表操作可重复，按 `-O` 出现顺序执行。`-Ojvm.extraArguments:=` 可清空原有普通 JVM 附加参数。
- 便捷参数均只能出现一次。同一标量键不能被多次指定，包括 `-O` 与便捷参数重叠的情况。
- 所有覆盖完成后统一检查配置结构和参数区域边界；JVM 参数的取值、重复及兼容性由实际 Java 处理。`-E` 接受普通 JVM 参数，内存/GC 扩展项使用 `-Ojvm.memory.extraArguments=...`。

## 两份配置

| 文件 | 职责 |
| --- | --- |
| `launch.toml` | 固定 JVM 参数、class/JAR/module 入口、固定应用参数、工作目录与环境覆盖 |
| `conf.toml` | Java 可执行文件、内存与 GC、authlib-injector、Minecraft 启动覆盖、用户附加参数 |
| `.mcpm/server.json` | Minecraft 和 Loader 登记身份 |

两个 TOML 始终从 `--server` 指定的根目录读取，缺失时明确报错。脚本导入时，原命令指定的设置会生效，未指定的设置保持注释；完整手动登记只生成默认 `conf.toml`，不会伪造启动入口。`conf.toml` 对全部字段附有英文说明。

```toml
# launch.toml：一个简单 JAR 服务端
[entry]
type = "jar"
path = "server.jar"
```

```toml
# conf.toml：只启用需要的设置
[java]
executable = "java"

[jvm.heap]
initial = "1G"
maximum = "4G"

[server]
nogui = true

[application]
extraArguments = []
```

## 路径和参数

- `process.workingDirectory` 相对于服务端根目录，省略时为根目录。JAR、类路径、模块路径及应用参数中的相对路径按 Java 的实际工作目录解释。
- `java.executable` 为裸名称时通过有效 `PATH` 查找；相对可执行路径按服务端根目录解释。导入会保留源命令的路径含义。
- 参数数组的每个字符串是一项 argv；不做 Shell 展开。原始数组不能创建第二个 Java 入口，也不能继续引用入口之前的 `@file`。
- 入口支持 `class`、`jar`、`module`；不支持源码模式、退出命令、未知操作数边界或其他平台的启动配置。
- 原始参数数组保留顺序和重复项，由 Java 或服务端决定它们的含义；CLI 标量覆盖仍不可重复。
- 内存或 GC 设置中的显式 `auto` 目前不受支持，会报错。省略这些字段时，由 JVM 自行决定。
- 环境变量直接传给 Java；普通启动不预先运行 Java 来验证调优参数。超长命令仅探测参数文件传输能力。

参数顺序为：固定 JVM 参数 → 结构化 JVM 设置 → 内存扩展参数 → 普通 JVM 附加参数 → 认证 agent 及其属性 → 入口 → 固定应用参数 → 服务器覆盖参数 → 应用附加参数。不能安全改变顺序的内存组合保留在同一个内存扩展数组中。

应用参数只有在选项操作数边界已知时才提取为服务器设置；遇到未知语法时整段按原顺序保留。解锁开关与固定 JVM 诊断选项的顺序无法安全表示时，导入会明确报错。

`server.port` 和 `server.world` 只生成对应启动参数，不读写 `server.properties`；是否接受这些参数及其值由目标服务端决定。`world` 选择世界目录。

## 启动进度

- 准备阶段向 stderr 提示读取配置、Java 与参数检查、认证准备及启动 Java；仅在需要时显示参数文件能力探测、发布查询、下载、验证和安装阶段。
- stderr 支持动态终端绘制且未指定 `--raw` 时，所有阶段在同一个进度区域更新，不逐行打印状态。下载显示来源、字节进度及速度；总大小已知时额外显示百分比与预计剩余时间，未知时保持不定进度。
- 来源失败后显示失败来源，下一次尝试重新计算字节进度与速度，不累计失败尝试的数据。传输结束后仍需验证及安装。
- `--raw`、stderr 重定向或不支持动态绘制的终端关闭进度展示，保留游戏输出与错误诊断。准备进度不受 `--log-level` 过滤，不写入 stdout。
- 启动 Java 前清除动态显示，随后进入 attach 或返回 run ID。准备完成或进程创建不表示 Minecraft 已就绪；mcpm 不解析游戏日志判断启动状态。
- 失败或取消时清理动态显示；错误继续通过统一错误链展示。

## authlib-injector

在 `conf.toml` 中添加最小配置即可启用：

```toml
[authlib-injector]
apiUrl = "https://example.com/api/yggdrasil/"
```

- `jar` 省略或为 `""` 时，由 mcpm 管理 `.mcpm/authlib-injector/authlib-injector.jar`；`meta.json` 保存发布号码、版本名称、上游 SHA-256 和可选发布时间。
- `jar = "libraries/custom-agent.jar"` 使用手动文件，相对路径以服务端根目录为准，与 Java 工作目录无关。只检查文件存在且可读取，不验证摘要、不读取 meta，也不自动替换。
- `--auth` / `-A` 只覆盖本次启动的 API 地址，保留其他认证设置；与 `-Oauthlib-injector.apiUrl=...` 同时指定会报重复覆盖。托管路径不写回 TOML。
- 模板列出所有支持的认证字段：日志、Mojang 命名空间与代理、旧皮肤兼容、调试、忽略包、HTTP 服务、服务器名称、anti-feature、profile key 和用户名检查。三态字段接受 `default`、`enabled`、`disabled`；布尔值为 false 或省略时不生成属性。
- 结构化配置与原始 JVM 参数重复指定同一个认证 agent 或已配置属性时会报错，不能通过追加原始参数覆盖结构化设置。

`--auth-jar-policy <auto|local|refresh>` 控制本次启动的托管 JAR：

| 策略 | 行为 |
| --- | --- |
| `auto`（默认） | 验证本地 meta 和 JAR 摘要；有效则直接使用且不联网，缺失、meta 损坏或摘要不匹配时下载最新发布 |
| `local` | 同样验证本地文件；需要修复时直接报错，不下载 |
| `refresh` | 重新下载最新发布，即使版本未变化 |

- 显式指定策略要求启用认证且使用托管 JAR；手动 JAR 或未启用认证时会报错。
- 不支持的 meta 格式版本、权限及文件 I/O 错误、目标为目录等情况直接报错，不自动覆盖。下载或校验失败阻止启动，不回退使用旧文件。
- 下载复用 BMCLAPI、官方源的默认顺序；有效本地文件不会触发更新检查。`local` 仅约束 mcpm 获取 agent，不限制 Java 或认证服务联网。
- 校验、下载和发布期间持有独立的准备锁，忙时直接报错。准备完成即释放，Minecraft 运行期间不持锁；停止准备会取消下载并清理临时文件。
- 托管目录属于服务端状态，不由 cache 命令管理。

```sh
mcpm launch -A https://example.com/api/yggdrasil/
mcpm launch --auth-jar-policy local
mcpm launch --auth-jar-policy refresh
mcpm launch -Oauthlib-injector.jar=libraries/custom-agent.jar
```

## 重新导入与退出

- 修改源脚本后，显式使用 `mcpm init ./server --start-script run.sh --force` 重新导入。
- `--force` 替换登记和启动结构，逐字节保留已有 `conf.toml`；保留配置有错误或与新结构冲突时会报错。
- 同时指定 `--force --foverride-conf` 才会重新生成 `conf.toml`，覆盖已有设置和注释。
- 中断的配置发布保留恢复日志；启动会拒绝混合状态，下一次显式初始化先恢复旧文件。工作区类型标记发布失败会明确报告已完成的文件状态。
- Unix shim 脱离原终端，Java 使用独立进程组；Windows Java 使用进程组和 Job，不接收前端的控制台 Ctrl+C 广播。
- 停止由命令通道完成；只有明确的第二次 Ctrl+C / kill 操作才强杀，不设置自动强杀超时。
- 非零退出码保持为 Java 的运行结果；启动、等待或清理失败经统一错误链输出。Unix 信号结果映射为 `128 + signal`。
- 超长命令使用临时 Java 参数文件，要求 Java 9+；执行完毕删除，不成为持久化配置。

旧版 `server.json.start` 仅供历史信息展示。它没有可靠保存实际工作目录，不能用于新启动流程；应显式重新导入脚本。
