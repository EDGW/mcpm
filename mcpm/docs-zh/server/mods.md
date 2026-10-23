# 服务器 Mod 检查

`server mods info` 和 `server mods deps` 使用已登记服务器的 loader 和 mods 目录，支持 Console、Raw、stdin 输入，以及 `--file`、`--modid`、`--name` 和 `--all`。

```sh
mcpm --ssh 'ssh user@host' server mods info --server /srv/minecraft --all --parallelism 4
mcpm --ssh 'ssh user@host' server mods deps --server /srv/minecraft --modid create
```

SSH 模式下，`.mcpm/server.json`、mods 目录和选择输入均在远端解析。文件名相对于远端 mods 目录，接受 `mods/` 前缀，拒绝父路径穿越及绝对文件输入。Minecraft 和 loader 版本仍来自服务器元数据。

`--parallelism` 默认 10，约束完整扫描和 modid/name 的延迟索引；多个独立文件输入仍顺序处理。进度及诊断实时回传，输出在本地渲染。远端诊断遵循全局 `--remote-logs` 选项。

协议 ID 为 `server.mods.info` 和 `server.mods.deps`。请求接受 `server`（默认 `.`）、`selection` 和正整数 `parallelism`（默认 10），复用 [mod info/deps](../remote.md) 的选择及检查事件格式。

## 列表

```sh
mcpm --ssh 'ssh user@host' server mods list --server /srv/minecraft --parallelism 4
mcpm --ssh 'ssh user@host' server mods list --server /srv/minecraft --modid create --expanded
```

列表支持 TUI、Console（`--no-ui`）和 Raw；没有选择输入时列出全部文件。过滤、列配置和布局在本地处理，TUI 复用同一 SSH 会话读取远端文件摘要，再在本地查询来源。嵌套声明选择保留其原始归档身份。

`server.mods.list` 请求接受 `server`、`selection` 和 `parallelism`。事件依次包含 `scope {loaders}`、`scan {completed,total}`、`error {error}`、有序的 `item {index,record}`，以 `done {files,input_errors,failed}` 结束。记录复用 mod list 表示，可附带选择键 `records`，其中每个键具有 `metadata` 和 `record` 索引。完成事件的失败数以扫描缓存中的唯一失败文件为准，包含未被选中的失败文件，重复输出同一选择不会重复计数。
