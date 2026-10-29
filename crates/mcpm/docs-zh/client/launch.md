# mcpmc launch

准备并启动一个或多个已安装版本，不提供 `--all`。

```sh
mcpmc launch [VERSION ...] [--directory PATH] [OPTIONS]
mcpmc launch 1.18.1 --directory "$HOME/Downloads/.minecraft" --java /path/to/java
mcpmc launch "整合包 A" "整合包 B" --directory "$HOME/Downloads/.minecraft" --username Player
mcpmc launch --directory "$HOME/Downloads/.minecraft"
mcpmc launch 1.21.1 --max-memory 4096 --width 1280 --height 720
mcpmc launch 1.21.1 --jvm-arg=-XX:+UseG1GC --game-arg=--fullscreen
```

版本 ID 是 `mcpmc list` 显示的目录名，按传入顺序去重。先逐个准备并创建进程，再等待所有进程退出；某个版本失败仍继续启动其余版本。子进程退出失败也会使命令最终返回失败。

未指定 ID 时打开 TUI：上下键选择一个版本、Enter 启动、Esc 或 Ctrl-C 取消。取消发生在准备和创建进程之前，返回成功。标准输入、标准输出、标准错误中任一个不是 TTY，或指定 `--raw` 时，统一禁用交互和颜色，必须显式指定 ID，不能使用 `--all`。

`--directory` / `-d` 指定 Minecraft 根目录，默认使用 pwd 路径中最后一个目录名恰好为 `.minecraft` 的目录，不存在则使用 pwd。自动识别时提示用 `mcpmc launch --directory .` 强制使用当前目录；raw 模式隐藏提示。

默认以每个版本自己的目录作为游戏目录和工作目录，共享根目录下的 libraries/assets。`--game-directory` 自定义游戏目录；`--shared-game-directory` 改用 Minecraft 根目录，两者互斥。相对目录覆盖路径均相对于 Minecraft 根目录解析。

## Java 和身份

| 参数 | 行为 |
| --- | --- |
| `--java PATH` | Java 可执行文件，默认使用 PATH 中的 `java`，不自动搜索或安装运行时。 |
| `--java-version-check minimum\|exact\|skip` | 默认 minimum，准备时检查 manifest 声明的 Java 主版本。 |
| `--java-arch ARCH`、`--java-bits 32\|64` | 覆盖宿主平台默认值以匹配所选 JVM；已知架构自动推断位数，显式位数优先。 |
| `--username NAME` | 默认离线 `Player`。 |
| `--uuid UUID` | 覆盖离线 UUID，或传入在线账号 UUID。 |
| `--access-token TOKEN` | 使用已有在线身份，必须提供 UUID；也可从 `MCPM_ACCESS_TOKEN` 环境变量读取。 |
| `--user-type TYPE` | 覆盖离线 legacy / 在线 msa 默认值。 |
| `--xuid ID`、`--client-id ID` | 可选账号字段，需同时提供 token。 |

状态输出及帮助中的环境变量信息不打印 token。此命令不负责登录或刷新令牌。预先设置 `MCPM_ACCESS_TOKEN` 后可使用：

```sh
mcpmc launch 1.21.1 --username Player --uuid ACCOUNT_UUID
```

## 游戏和运行配置

| 参数 | 行为 |
| --- | --- |
| `--min-memory MIB`、`--max-memory MIB`、`--stack-size KIB` | 正数 JVM 内存配置，不隐式覆盖内存；最小堆不能大于最大堆。 |
| `--jvm-arg VALUE`、`--game-arg VALUE` | 可重复追加参数；含空格时加引号，带横线的 JVM 参数推荐 `--jvm-arg=-X...`。 |
| `--main-class CLASS` | 覆盖解析后的主类。 |
| `--working-directory PATH` | 仅覆盖进程工作目录，默认游戏目录。 |
| `--width N`、`--height N` | 正数窗口大小，两者必须同时传入。 |
| `--fullscreen`、`--demo` | 启用全屏或演示模式。 |
| `--server ADDRESS`、`--port PORT` | 旧式服务器连接参数；port 要求 server。 |
| `--quick-play-singleplayer WORLD`、`--quick-play-multiplayer ADDRESS`、`--quick-play-realms ID` | 三者互斥，并与旧式 server 互斥，要求版本声明对应参数。 |
| `--libraries-directory PATH`、`--assets-directory PATH`、`--client-jar PATH` | 覆盖已安装的资源位置。 |
| `--natives-directory PATH` | natives 目录，默认为 `versions/<version>/<version>-natives`，退出后保留。 |
| `--use-existing-natives` | 需指定 natives 目录，跳过解压。 |
| `--no-logging-config` | 不传入 manifest 的日志配置参数。 |
| `--verify none\|default\|strict\|shallow` | 可选启动前完整性检查；默认 none；strict 对四类资源全部 hash，shallow 全部 size。 |

现有完整性检查使用标准缓存/JAR 路径，因此非 none 的 `--verify` 不能与自定义 libraries/assets/JAR 路径组合。未启用完整检查时，仍要求启动所需的本地文件存在。不下载资源、不安装 Loader、不自动修复依赖。

## 进程输出和生命周期

| 参数 | 行为 |
| --- | --- |
| `--game-output inherit\|null` | 默认继承游戏 stdout/stderr；null 隐藏游戏日志，游戏 stdin 固定为 null。 |
| `--clear-env` | 清空继承的子进程环境，再应用覆盖。 |
| `--env KEY=VALUE` | 可重复设置子进程环境变量。 |
| `--unset-env KEY` | 可重复移除变量，优先于设置。 |

Java 版本探测也使用配置后的环境。清空环境时需提供可用的 PATH 或绝对 Java 路径。

终端中，准备状态使用亮青色，启动和成功退出亮绿，退出失败亮红，版本名和 PID 亮白。`NO_COLOR` 关闭颜色。raw 或重定向状态输出使用三列 TSV：版本、事件（preparing / started / exited）、详情。继承的游戏日志共享命令输出流，采集状态行时可用 `--game-output null`。诊断写入 stderr，解压后的 natives 保留在版本目录下的 `<version>-natives` 中，后续启动继续向该目录解压并覆盖同名文件。

全部进程成功退出或取消选择返回 `0`；准备、启动、等待、输出失败或子进程退出失败返回 `1`；参数错误返回 `2`。完整参数见 `mcpmc launch --help`。

## 启动诊断日志

全局 `--log-level info` 显示计划构建、资源验证、Java 版本检查、native 解压阶段、
进程创建、PID 和退出状态。`debug` 增加普通完成提示、每个 native 压缩包的解压、平台、路径、库选择及最终脱敏的
可执行文件/参数列表；`trace` 增加每个 native 条目、排除项、大小和必需文件检查。
默认级别为 `warn`。日志写入 stderr，游戏输出仍由 `--game-output` 决定。
启动日志对 token 和常见凭据参数脱敏，不输出环境变量的值。

```sh
mcpmc launch 1.18.1 --no-logging-config --log-level debug
```
