# `mcpm launch`

从服务端根目录中的配置启动 Java，继承终端输入输出，在前台等待退出并传递 Java 的退出码。

```sh
mcpm init ./server --start-script run.sh
mcpm launch --server ./server
mcpm-server launch --server ./server
mcpm-tools server launch --server ./server
```

- `--server <PATH>`：已登记的本地服务端目录，默认 `.`；不接受路径位置参数。
- 支持 Console、Raw；`--raw` 或 stdout 重定向不改变 Minecraft 控制台内容。
- 不支持 SSH、Remote 工作区、后台守护或自动重启。
- 不执行原始脚本，不重新读取原始 Java 参数文件，不安装服务端依赖或接受 EULA；启用托管认证时可下载 authlib-injector。

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
- 启动 Java 前清除动态显示，随后继承游戏标准流。准备完成或进程创建不表示 Minecraft 已就绪；mcpm 不解析游戏日志判断启动状态。
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
- Unix 将控制终端交给 Java 进程组，正常退出后恢复；Windows 与 Java 共用控制台，避免重复发送 Ctrl+C。
- 显式停止请求等待最多十秒，再强制结束；再次请求会立即强制结束。Unix 终端 Ctrl+C 直接交给 Java 处理。
- 非零退出码保持为 Java 的运行结果；启动、等待或清理失败经统一错误链输出。Unix 信号结果映射为 `128 + signal`。
- 超长命令使用临时 Java 参数文件，要求 Java 9+；执行完毕删除，不成为持久化配置。

旧版 `server.json.start` 仅供历史信息展示。它没有可靠保存实际工作目录，不能用于新启动流程；应显式重新导入脚本。
