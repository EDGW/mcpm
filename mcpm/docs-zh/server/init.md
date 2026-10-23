# `mcpm server init`

```sh
mcpm server init [PATH] [OPTIONS]
mcpm server init /path/to/server
mcpm server init --start-script ./start.sh
mcpm server init --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
mcpm server init --minecraft-version 1.21.1 --loader vanilla
```

默认读取服务端目录下的启动脚本：Windows 使用 `run.bat`，Unix 使用 `run.sh`。
`--start-script FILE` 相对于服务端目录解析，也支持绝对路径。它与手动字段
`--minecraft-version`（别名 `--mc-version`）、`--loader`、`--loader-version` 互斥。
`--loader` 支持 `forge`、`neoforge`、`fabric`、`vanilla`。手动信息完整时直接注册；
缺少字段时从默认脚本识别，并以手动信息筛选已验证候选，无法匹配则失败。
原版没有 Loader 版本，不能同时指定 `--loader vanilla` 与 `--loader-version`。

已经存在 `.mcpm/server.json` 时，默认阻止初始化，包括内容损坏的配置。
`--force` 允许覆盖；仅在识别、选择和校验成功后原子替换，失败或取消选择保留旧配置。
这些选项及 `--full-command`、全局选项可与 `--start-script` 搭配。

识别只做静态读取，不运行脚本、Java 或安装器，不下载文件。
支持明确赋值的变量、引号、续行、`cd`/`pushd`/`popd`、shell 的 `source`/`.`、
`sh`/`bash`/直接脚本引用，以及 BAT 的 `call`、`%~dp0` 和简单 `setlocal`。
shell 的 `$(dirname "$0")` 是唯一支持的命令替换模式；改变工作目录后使用 `$0`
会保守失败。脚本参数按空处理：忽略 shell 常规尾部 `"$@"`，BAT 的 `%*`
无论位于何处都展开为空，并保留周围的文本和引号语义。
环境变量不从当前进程补全；子脚本中的本地变量和导出变量遵守所支持的静态范围。
条件、循环、管道、重定向、任意命令替换、延迟 BAT 展开、脚本循环引用或
无法确定的变量都会失败。

Java 的 `@参数文件` 被展平，已知的 classpath/module-path、主类、`-jar` 和
相关属性用于识别。未知 JVM option 原样保留并忽略，不验证 JVM 参数合法性。
不认识的带独立操作数选项无法猜测其参数数量；若影响入口解析，仍可能识别失败。
Loader 验证使用实际引用的 Maven 坐标、JAR 中的身份标记和游戏版本元数据，
避免从目录中残留的旧版本猜测。当前支持常见 Forge/NeoForge 参数文件、
Forge shim、Fabric 普通安装器与自动安装启动器，以及具有版本元数据的原版 JAR。
旧版无版本信息的 JAR、第三方启动包装器和未支持的布局会失败。
识别成功表示本地身份元数据一致，不保证文件真实可信、Java 参数有效或能够启动。

只有一个可靠候选时直接初始化；多个候选在 TUI 中使用上下键选择、Enter 确认、
Esc 或 Ctrl-C 取消。非 TTY 或 `--raw` 下出现多个候选会失败，可以通过更具体的
入口脚本或手动版本条件消除歧义。任何 Java 启动项无法识别时，整个识别失败，
不会只取其余可识别项。没有 Loader 证据不会直接视为原版。识别失败会明确提示未识别，
需要使用完整的版本与 Loader 参数手动初始化；已知原版必须显式指定 `--loader vanilla`。

成功后输出与 [`server info`](info.md) 相同的信息。配置保存选定启动项的脚本、
工作目录、Java 可执行文件和展平参数，作为初始化时的快照。
非 raw 模式下 Full command 默认只占一行，超出部分显示 `(+N)`（省略字符数）；
`--full-command` 显示完整命令。命令使用灰色，路径值使用普通色。
