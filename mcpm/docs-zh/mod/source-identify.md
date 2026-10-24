# `mcpm mod source identify`

按文件内容在指定平台反查来源，支持 Console、Raw 和 SSH Remote：

```sh
mcpm mod source identify --source modrinth --parallelism 4 a.jar b.jar
mcpm --raw mod source identify --source curseforge a.jar b.jar
mcpm --ssh 'ssh user@host' mod source identify --source modrinth /srv/mods/a.jar /srv/mods/b.jar
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm.exe' mod source identify --source curseforge 'D:\mods\a.jar'
```

`--source` 接受 `modrinth`、`curseforge`（Curse.tools）或 `curseforge@KEY`（官方 API）。路径是字面文件路径；stdin 每行追加一个路径。SSH 下路径属于远端，文件读取与摘要计算由远端执行，传回文件大小、SHA-1、所需的 MurmurHash2 指纹、校验后的缓存命中及读取错误。本地完成平台 HTTP 请求和匹配验证，沿用本地代理与 API key，不传输 JAR 内容。

`-p/--parallelism <COUNT>` 必须为正整数，默认 `10`，与 `mod list --identify-parallelism` 共用默认值。并发任务完成后立即推进 stderr 进度条，样式沿用 mod list；结果及逐项错误始终按输入顺序输出。`--parallelism 1` 顺序检查。重复物理路径共享查询，但每个原始输入仍保留自己的输出位置。`--raw` 关闭进度条，stdout 重定向时结果使用 TSV，进度条是否显示由 stderr 的终端状态决定。

未匹配输出 `unmatched`，匹配成功输出 `matched`。单项读取或请求失败不会阻止后续输入，错误保留逐级 `caused by:`，最终退出码为非零；远端读取错误始终标记为 `remote error:`；本地平台查询错误始终使用 `error:` 输出，均不受 `--remote-logs` 和 `--log-level` 影响。

默认列为 index、path、source、project、id、filename、size、role、status；使用 `--columns` 或 `-C` 选择列。展示选项留在本地，不作为远端命令参数。

Remote command ID 是 `mod.source.identify`。它与 `mod.list` 使用相同的共享识别调度；SSH 列表的 TUI 在现有会话上获取摘要，本地通过同一识别调度查询平台；`--identify-parallelism` 控制两个平台合计的并发度。每个文件的摘要只读取一次，两个平台共用，退出面板时取消本地查询和未完成的远端读取。详见[远程协议](../remote.md)。

`mcpm server mods source identify` 也支持同样的本地并发参数和有序输出，并支持 SSH 和已登记 Remote 路由。

## 只读缓存

- 默认查询文件主机的 `~/.mcpm/cache/mods`，命中则跳过平台请求。
- `--cache-dir PATH` 指定该主机上已初始化的缓存；`--no-cache` 关闭查询，二者互斥。
- 命中必须有 `.dat`、合法 `.identification.json`，并且来源结果与查询文件摘要一致；Loader metadata 缺失或损坏不影响来源缓存。
- 未命中则查平台，绝不写缓存。显式填充使用 `mod cache --contents ident` 或 `server mods cache --contents ident`。
- 官方 API／不同 key／Curse.tools 共用 `curseforge` 结果；凭据不持久化，也不跨 SSH 传输。
- 独立／服务器 Identify 和列表 TUI 共用此行为；列表 TUI 继承列表命令的缓存选择。
