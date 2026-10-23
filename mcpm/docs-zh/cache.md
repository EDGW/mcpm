# Mod 缓存

`mcpm cache mods` 将文件显式保存到 `~/.mcpm/cache/mods`。
`mod info/list` 默认查询已有缓存，`--cache-dir` 指定其他目录，`--no-cache` 关闭查询；不导入文件、不保存识别结果。
下载和来源搜索不自动使用缓存。所有命令支持 Console 和 Raw；stdout 重定向或使用
`--raw` 时每行输出一个 JSON 对象，空缓存的 `list` 不输出记录。缓存管理命令仅在本地执行，不支持 SSH。

使用 [`mcpm mod cache`](mod/cache.md) 可按文件名、ID、名称或 stdin 选择文件，并一次存储文件及其识别摘要。 已登记服务器可用 [`server mods cache`](server/mods.md#缓存) 继承服务器范围及 loader，并支持 SSH、Remote 工作区；server mods list/info 同样默认查询文件主机的缓存。

## 用法

```sh
mcpm cache mods init --directory /path/to/cache
mcpm cache mods import example.jar another.jar
mcpm cache mods import example.jar --mode hard-link
mcpm cache mods list
mcpm cache mods info <BLAKE3>
mcpm cache mods refresh <BLAKE3> --loader fabric
mcpm cache mods refresh <BLAKE3> --force
mcpm cache mods verify <BLAKE3>
mcpm cache mods export <BLAKE3> restored.jar another-copy.jar
mcpm cache mods export <BLAKE3> linked.jar --mode hard-link
mcpm cache mods remove <BLAKE3>
mcpm cache mods rebuild-index
```

在 mods 子命令前后使用 `--directory PATH` 选择其他缓存根目录。
默认目录不存在或为空时自动初始化；指定目录必须已初始化。非空无标记目录、manifest
损坏或不支持的版本均拒绝打开。目录格式为 version 2，不自动接管或迁移 version 1 GUID
缓存；可选择新目录以保留旧缓存。

对象 ID 是完整的 64 位小写十六进制 BLAKE3。相同 BLAKE3 视为相同内容，只保存一个对象。
默认导入复制文件，不识别 Mod。新对象初始为 `missing_digest`，文件已经可以导出。

import 接受多个文件；export 接受一个 BLAKE3，后跟一个或多个目标路径。
两者按输入顺序处理，每成功一项输出一条记录，首个失败时停止；此前成功项保留，批次不回滚。

| 命令 | 行为 |
| --- | --- |
| `init` | 初始化不存在或空目录；已有合法缓存也可成功 |
| `list` | 列出完整和不可用对象，不解析 JAR |
| `info BLAKE3` | Console 展示 File、Size、SHA-256，再按 `mod info` 格式展示已识别 Loader 声明；Raw 保留完整缓存记录 |
| `refresh BLAKE3` | 识别缺失或过期结果，默认使用所有内置 Loader |
| `refresh BLAKE3 --force` | 强制重新解析选中的 Loader |
| `verify BLAKE3` | 检查对象内容及摘要；`--structure-only` 跳过完整哈希 |
| `export BLAKE3 PATH...` | 导出到缓存外的不存在目标，父目录必须已存在 |
| `remove BLAKE3` | 只删除缓存名字，报告是否实际删除 |
| `rebuild-index` | 从已有对象重建可选的 imohash 候选索引 |

`refresh` 复用 Mod 检查的 `--loader` 和逗号分隔的 `--loaders`。
Loader 失败时保留原描述。Raw 的 `digest.loaders` 区分未检查（缺少键）、已检查但
未识别（`null`）、已识别（ModMeta 快照对象）。未识别结果可以复用，`--force` 可重新检查。

## 导入与硬链接

- 导入先按内容查找。合法对象已存在时直接复用，不替换已有文件、不准备 staging、不尝试硬链接、不改源权限。
  重复导入独立副本不会替换源文件，也不会将其连接到已有缓存对象。
- mode 仅在需要新增对象时生效：`--mode copy` 为默认；`hard-link` 要求硬链接；
  `prefer-hard-link` 仅在文件系统能力不支持时回退复制。即使源文件跨文件系统，`hard-link` 也能复用已有对象。
- Raw 导入记录的 `published` 表示是否由本次新发布；复用为 false。
  新发布时 `method` 为 `copy` 或 `hard_link`，复用时为 `null`。
  Console 展示 added/reused，以及实际发布方式。
- 硬链接导入、导出共享内容及写保护；硬链接导入也保护源文件。
  更新时写入新文件再替换自己的文件名，不原地覆写、追加或截断。
- 导出不覆盖目标、不创建父目录。删除缓存对象不影响已导出副本或硬链接。
  没有自动清理或备份引用管理。

## 可用状态与并发

- `available`：识别摘要有效，包括明确未识别；至少检查过一个 Loader。
- `not_found`：数据和描述均不存在。
- `unavailable`：描述缺失、数据无效或不兼容、读取异常。
- `info` 将这些状态作为正常查询结果，Console 解释原因，Raw 保留结构化状态。
  `verify` 对不可用或不存在对象返回失败。
- `refresh` 可以创建缺失摘要；无效内容描述会拒绝，解析版本过期则可刷新。
- 完整校验和导出按 BLAKE3 ID 检查内容，包括没有识别摘要的对象；结构检查不能发现
  同大小的内容变化。没有摘要的状态仍为 `missing_digest`。
- 缓存允许缺失：并发写入可以丢失，已写入数据也可能消失；接受复用的结果必须正确。
  数据发布后不可变，metadata 使用完整快照原子发布。
- 没有全局读写锁。并发删除可能导致未命中，并发发布可以在删除后重新产生对象。
  导入成功描述本次操作，不承诺之后持续存在。
- imohash 索引可重建，缺项或陈旧候选不能证明内容相同；查询验证候选并回退完整 BLAKE3。
  索引恢复有单独的非阻塞协调锁；SQLite 事务与初始化也可以内部同步。
  索引忙时可跳过写入，不阻塞对象或 metadata 操作。

[命令参考](README.md) · [English](../docs/cache.md)
