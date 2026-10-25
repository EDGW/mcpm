# 服务端 Mod 快照

`mcpm-server stash` 保存并切换完整的 `mods/` 文件集合。`mcpm stash` 和
`mcpm-tools server stash` 使用相同实现。第一版仅支持本地服务器，拒绝 SSH 和 Remote 工作区。

## 常用流程

```sh
mcpm-server stash create "稳定版"
# 修改 mods 后保存另一个版本
mcpm-server stash create "实验版"
mcpm-server stash list
mcpm-server stash status
mcpm-server stash switch --name "稳定版"

# 显式丢弃当前未保存的修改
mcpm-server stash switch --name "稳定版" --force
```

- 使用 `--server PATH` 选择服务器，默认当前目录；可放在 stash 子命令前或后。
- 快照递归包含所有普通文件，包括禁用 Mod 和非 JAR 文件，不记录空目录。
- 不处理世界、配置、启动脚本或 Minecraft、loader 版本。
- 创建和切换期间保持 mods 稳定，停止其他修改文件的程序。
- stash 目录不加入工作区生成的 `.gitignore`。

## 命令

| 命令 | 行为 |
| --- | --- |
| `create NAME` | 保存有修改或尚未记录的工作区，并更新当前比较基准 HEAD；无修改时拒绝重复保存 |
| `switch --name NAME` / `--id UUID` | 完整验证目标，再检查工作区并完整替换 mods；不合并目录 |
| `status` | 比较工作区与 HEAD，列出新增、删除、修改的文件 |
| `list` | 按时间降序列出快照；不计算工作区 checksum 或完整校验快照 |
| `info` | 读取所选快照及 manifest，默认展示所有字段；不完整校验数据文件 |
| `rename --name OLD NEW` / `--id UUID NEW` | 修改名称，UUID、创建时间和备份引用不变 |
| `delete` | 删除所选索引及备份，保留 mods 和缓存；拒绝删除 HEAD |
| `verify` | 完整检查索引、manifest、文件集合，以及备份或服务器 stash 缓存内容的大小和 BLAKE3；不修改或修复数据 |

名称唯一、大小写敏感，允许中文和空格，不自动去除两端空格；禁止空白名称及控制字符。

## 工作区与切换

HEAD 指向工作区最近保存或切换到的 stash，不表示工作区始终无修改。

| 状态 | 含义 | switch |
| --- | --- | --- |
| `clean` | 工作区 checksum 与 HEAD 一致 | 允许；目标就是 HEAD 时无需替换 |
| `modified` | 与 HEAD 不一致 | 拒绝，除非使用 `--force` |
| `untracked` | 尚无 HEAD | 拒绝，除非使用 `--force` |

- `status` 成功得到以上任一状态都返回 0，检查错误才返回非零。
- switch 总是先完整 verify，包括 `--force` 和目标为 HEAD 的情况；清单引用的内容通过服务器自己的 stash 缓存校验，缺失或损坏时拒绝切换。
- `--force` 只允许丢弃内容，不忽略损坏目标、损坏 HEAD 或工作区读取错误。
- 强制切换到 HEAD 可以丢弃当前修改、恢复已保存版本。
- switch 不自动保存被覆盖的修改。需要保留时先 create。
- 恢复使用复制生成可写文件，完整替换 mods，删除目标中不存在的旧文件。

## 选择多个快照

info、verify、delete 接受重复的单数参数、逗号分隔的复数参数或 stdin：

```sh
mcpm-server stash info --name "稳定版" --name "实验版"
mcpm-server stash verify --ids UUID1,UUID2
printf '%s\n' '稳定版' '实验版' | mcpm-server stash info --input name
mcpm-server stash delete --id UUID1 --id UUID2
```

- 类型不自动推断，非空 stdin 必须指定 `--input name|id`。
- 单数 `--name` 的逗号是名称内容，`--names` 按逗号拆分。
- 顺序为 name、names、id、ids、stdin，各组内保留输入顺序。
- 同一 UUID 只处理一次。单项失败继续其他项，最终返回非零。
- delete 在开始前检查整个选择集合：包含 HEAD 时整批拒绝。损坏 HEAD 也会阻止删除。
- 删除无额外确认。先删 backup，再删索引；backup 已不存在时可以清理索引。

## 字段与展示

全部命令支持 Console、Raw，`--raw` 或 stdout 重定向使用 TSV，`--headers` 添加表头。

list 和 info 支持 `-C`、`--columns`、重复的 `--column`，显式选择覆盖默认字段：

| 字母 | 字段 | list | info |
| --- | --- | --- | --- |
| N | name | ✓ | ✓ |
| I | id | ✓ | ✓ |
| T | time | ✓ | ✓ |
| H | head | ✓ | ✓ |
| B | backup | ✓ | ✓ |
| P | path | | ✓ |
| C | checksum | | ✓ |
| F | files | | ✓ |
| S | size | | ✓ |

- list 默认 name、time、head；`--details` 展示全部轻量字段。Console 超过 80 项先询问。
- info 默认全部字段，`--details` 在 Console 追加逐文件详情。
- head 为比较基准标记，不是 clean 标记；无法确定 HEAD 时留空并报告错误。
- size 统计文件内容字节，不含 manifest，不代表实际占用空间。
- Raw 按所选字段顺序输出；日期为 UTC，布尔为 true/false，大小为整数字节。
- create、switch、rename、delete、verify 的 Raw 列：`operation target id name status`。
  状态为 created、switched、unchanged、renamed、deleted、valid 或 failed。
- status 的 Raw 列：`state id name expected_checksum actual_checksum change path`。
  每个变化文件一行，change 为 added、removed、modified；无变化或尚未记录时一行，变化字段为空。
- 控制字符及反斜杠按既有 TSV 规则转义。错误统一在 stderr 逐层显示。
- create、switch、status、verify 接受 `--parallelism N`，默认 10，必须为正整数。

## 文件、并发与失败

- 索引位于 `.mcpm/stash/index/<UUID>.json`，HEAD 位于 `.mcpm/stash/HEAD`。
- 备份位于 `.mcpm/data/<backup-UUID>/`，缓存根为 `.mcpm/stash/blob/`。
- 新建 stash 默认使用 cache-only：备份目录只保存 `manifest.mcbackup`，内容保存在同一服务器的 `.mcpm/stash/blob/`，不再生成 backup 的 `data/` 副本。
- verify 和 switch 使用该服务器的 stash 缓存，不使用 global cache，不初始化缺失缓存。status、list、info 不读取缓存内容。
- 移动服务器时将备份清单、stash 索引、HEAD 和 blob 一起保留，路径没有写死在清单中。
- 删除 stash 只删除其备份清单和索引，保留缓存内容供其他 stash 使用。
- 旧完整备份仍可使用；其中缺失的文件可由服务器 stash 缓存补足，已经存在但损坏的文件不会被缓存掩盖。
- 操作系统文件锁协调 stash 命令；占用时直接报错。锁文件长期保留，不要手动删除。
- 不同程序修改 mods 不受该锁约束。
- HEAD 仅在保存索引或成功应用之后更新。HEAD 更新失败会明确报告已有结果，不伪装全部成功。
- 切换依赖现有 backup 的失败回滚，文件替换与 HEAD 更新不是同一事务。
- 不提供中断恢复或断电事务保证。中断后重新执行 status 检查，不自动继续或覆盖。
- 损坏索引、HEAD、manifest 不被静默重建；部分删除可能需要按错误信息检查现场。

备份协议及完整性校验见 [Mod 文件备份](../mod/backup.md)。
