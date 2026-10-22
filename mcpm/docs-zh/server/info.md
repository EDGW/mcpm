# `mcpm server info`

```sh
mcpm server info [PATH] [--full-command]
mcpm server info /Users/edgwu/Downloads/Server/Server
```

默认读取当前目录。必须存在可读且有效的 `.mcpm/server.json`，否则命令失败。
显示目录名、路径、Minecraft 版本以及 Loader 和版本。
Loader 未配置或不属于内置识别范围时，显示 `vanilla or unrecognized`。
配置中 `version` 必须为数字 `1`，表示配置格式版本；Minecraft 版本使用
`minecraft_version` 字段。不支持的配置格式版本会拒绝加载。

终端中标题亮青，名称和版本亮白，路径值和工作目录值使用普通色，标签及完整命令灰色；Loader 沿用统一配色：
Forge 亮黄、NeoForge 亮红、Fabric 亮青。`NO_COLOR` 关闭颜色。
`--raw` 或非 TTY 模式输出单行六列 TSV：名称、路径、Minecraft
版本、Loader、Loader 版本、完整命令。不识别 Loader 时 Loader 版本为空。控制字符会被转义，
错误写入 stderr。

完整命令和工作目录来自 [`server init`](init.md) 保存的快照，不会重新读取脚本。
旧配置或完整手动注册没有保存启动信息时显示 `unavailable`。
非 raw 模式默认只显示一行命令，超出的部分用 `(+N)` 表示省略的字符数；
传入 `--full-command` 可显示完整内容。raw 模式始终输出完整命令。

## SSH

```sh
mcpm --ssh 'ssh user@host' server info /path/to/server --full-command
```

服务器目录和 `.mcpm/server.json` 在 SSH 主机读取；启动参数按来源系统的规则引用。Console/Raw 输出在本地渲染，协议命令 ID 为 `server.info`。
