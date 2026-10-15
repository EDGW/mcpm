# mcpm client list

列出本地 Minecraft 客户端版本和声明的 ModLoader 版本。

```sh
mcpm client list [PATH]
mcpm client list "$HOME/Downloads/.minecraft"
mcpm --raw client list .minecraft > clients.tsv
```

`PATH` 是包含 `versions/` 的 Minecraft 根目录。未传参数时，选取当前路径中最后一个目录名恰好为 `.minecraft` 的祖先目录；不存在时使用当前目录。自动选择根目录时，会在标准错误输出提示所选路径，并提醒使用 `mcpm client list .` 强制列出当前目录。显式传入路径时不自动选择；`--raw` 隐藏该提示。按目录名排序加载每个版本子目录。缺少版本 JSON、JSON 损坏、继承父版本缺失或继承循环均显示为 `broken`，并保留完整错误原因，不影响其他版本列出。命令不修改文件。

`normal` 表示版本元数据成功加载。支持按 HMCL 的库、启动参数和 patches 规则识别 Fabric、Forge、NeoForge。未识别到支持的 Loader 时，根据 mainClass 判断显示 `vanilla` 或 `unrecognized`；这些标签不代表已验证版本能够启动或依赖库已下载。

终端中版本名亮白、正常状态亮绿、损坏状态亮红、错误原因亮黄，Loader 沿用 `mod` 命令的配色。`NO_COLOR` 禁用颜色。重定向输出或使用 `--raw` 时，不输出表头，依次用制表符分隔五列：序号、目录名、状态、Loader、详细信息。名称和详细信息中的控制字符会被转义。

空结果、包含损坏版本的列表返回成功；根目录、扫描、输出或 Loader 探测失败返回状态码 `1`。Loader 探测失败的条目标记为 `failed`，其他条目仍会输出。
