# `mcpm server info`

```sh
mcpm server info [PATH]
mcpm server info /Users/edgwu/Downloads/Server/Server
```

默认读取当前目录。必须存在可读且有效的 `.mcpm/server.json`，否则命令失败。
显示目录名、路径、Minecraft 版本以及 Loader 和版本。
Loader 未配置或不属于内置识别范围时，显示 `vanilla or unrecognized`。
配置中 `version` 必须为数字 `1`，表示配置格式版本；Minecraft 版本使用
`minecraft_version` 字段。不支持的配置格式版本会拒绝加载。

终端中标题亮青，名称和版本亮白，路径和标签灰色；Loader 沿用统一配色：
Forge 亮黄、NeoForge 亮红、Fabric 亮青。`NO_COLOR` 关闭颜色。
`--raw` 或非 TTY 模式输出单行五列 TSV：名称、路径、Minecraft
版本、Loader、Loader 版本。不识别 Loader 时最后一列为空。控制字符会被转义，
错误写入 stderr。
