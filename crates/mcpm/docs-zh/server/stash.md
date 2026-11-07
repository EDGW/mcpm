# Stash 命令

保存并恢复服务端 `mods/` 目录的快照。目标选择见 [README](README.md)。

快照有名称、UUID 和创建时间，记住 `mods/` 下每个文件的内容。保存之后可以拿当前的 `mods/` 与快照比较、检查快照是否完好，或者恢复到某个快照。快照属于服务端本身，支持 Local Server、显式 SSH 目标和 Remote workspace。

## 命令族选项

```text
mcpm stash [OPTIONS] <COMMAND>
```

| 选项 | 说明 |
| --- | --- |
| `--server <PATH>` | 服务端目录，或已登记的 Remote workspace。默认 `.`，可放在子命令前后。 |
| `--headers` | Raw TSV 输出表头；Console 下忽略。 |

## 通用规则

### 保存内容

- 快照包含 `mods/` 下所有常规文件，包括被禁用的 Mod 和非 JAR 文件；空目录不保存。
- 世界、配置、启动脚本、Minecraft 版本和 Loader 声明不属于快照，切换时不会改变。
- 创建和切换期间不要修改 `mods/`。
- cache 管理命令修改服务端 stash 缓存时默认被拒绝；`--no-protect-stash` 可绕过该保护，见 [stash 缓存保护](../legacy/cache.md)。

### HEAD 与工作区状态

**HEAD** 是最近一次保存或切换到的快照，`status` 和 `switch` 都以它为基准。HEAD 只表示工作区从哪个快照出发，不表示工作区没有被修改。

`status` 把 `mods/` 与 HEAD 比较，得到三种状态：

| 状态 | 含义 | `switch` 是否需要 `--force` |
| --- | --- | --- |
| `clean` | 与 HEAD 完全一致 | 不需要；切换到 HEAD 本身也无需替换 |
| `modified` | 与 HEAD 不同 | 需要 |
| `untracked` | 还没有 HEAD（从未保存过） | 需要 |

三种状态都是成功的结果，命令返回 0；只有检查本身出错才返回非零。

### 指定快照

`switch`、`rename` 需要指定恰好一个快照，`info`、`delete`、`verify` 可以指定多个：

| 选项 | 含义 |
| --- | --- |
| `--name <名称>` | 精确名称；可重复。名称里的逗号就是名称的一部分 |
| `--names <名称,名称>` | 逗号分隔的多个名称 |
| `--id <UUID>` | 完整 UUID；可重复 |
| `--ids <UUID,UUID>` | 逗号分隔的多个 UUID |
| `--input name\|id` | 把 stdin 里每一个非空行当作名称或 UUID。stdin 有非空行时必须指定，类型不会自动推断 |

`switch` 和 `rename` 只接受 `--name` 或 `--id` 之一。

```sh
mcpm stash info --name 稳定版 --name 实验版
mcpm stash verify --ids UUID1,UUID2
printf '%s\n' 稳定版 实验版 | mcpm stash info --input name
mcpm stash delete --id UUID1 --id UUID2
```

指定多个快照时：

- 处理顺序为 `--name`、`--names`、`--id`、`--ids`、stdin，各组内保持输入顺序。
- 同一个快照被指定多次（例如先按名称、再按 UUID）只处理一次。
- 某一项失败不会阻止后面的项，但只要有失败，命令最终返回非零。

名称唯一且区分大小写。允许空格和 Unicode；只含空白的名称和控制字符会被拒绝。

### 输出模式

所有命令都支持 Console 和 Raw。`--raw` 或 stdout 被重定向时使用 Raw（TSV）；`--headers` 输出表头。诊断和进度使用 stderr。

### 字段

`list`、`info` 按字母选择字段：

| 字母 | 字段 | 含义 | list | info |
| --- | --- | --- | --- | --- |
| N | name | 名称 | ✓ | ✓ |
| I | id | UUID | ✓ | ✓ |
| T | time | 创建时间（UTC） | ✓ | ✓ |
| H | head | 是否为 HEAD（true/false） | ✓ | ✓ |
| B | backup | 备份 UUID | ✓ | ✓ |
| P | path | 备份所在路径 | | ✓ |
| C | checksum | 快照整体校验和 | | ✓ |
| F | files | 文件数 | | ✓ |
| S | size | 文件内容总字节数，不含清单，不代表磁盘占用 | | ✓ |

- `list` 接受 `NITHB`，`info` 接受 `NITHBPCFS`。`--columns` 用逗号分隔字段名，`--column` 一次一个字段。
- `list` 默认显示 name、time、head；`info` 默认显示全部字段。显式选择字段会替换默认字段。
- HEAD 无法读取时，`head` 字段留空，并报告错误。

### 结果行

`create`、`switch`、`rename`、`delete`、`verify` 在 Raw 下每个结果输出一行：

```text
operation target id name status
```

| 命令 | `target` | `status` |
| --- | --- | --- |
| `create` | 快照 UUID | `created` |
| `switch` | 快照 UUID | `switched` 或 `unchanged` |
| `rename` | 快照 UUID | `renamed` |
| `delete` | 你指定的名称或 UUID | `deleted` 或 `failed` |
| `verify` | 你指定的名称或 UUID | `valid` 或 `failed` |

失败的行 `id` 和 `name` 为空。Console 的显示见各命令的 `### 输出`。

### 数据位置与安全

- 快照数据保存在服务端目录的 `.mcpm/` 下：`.mcpm/stash/` 保存快照记录、HEAD 和内容缓存，`.mcpm/data/<备份 UUID>/` 保存该快照的备份清单。
- 移动或备份服务端时，要把 `.mcpm/stash/` 和 `.mcpm/data/` 一起带走。
- 同一服务端上，修改快照的命令（`create`、`switch`、`rename`、`delete`）不能与其他 stash 命令同时运行；遇到冲突立即报错，不会等待。`.mcpm/stash/` 下的锁文件需要长期保留，不要手动删除。
- 其他程序修改 `mods/` 不受这个限制，创建和切换期间请避免。
- 校验和切换只使用该服务端自己的 stash 缓存，不使用 `~/.mcpm/cache/mods` 下的全局缓存，也不会创建缺失的缓存。

## `mcpm stash create`

```text
mcpm stash create [OPTIONS] <NAME>
```

保存当前 `mods/`，并把它设为 HEAD。

### 参数

| 参数 | 说明 |
| --- | --- |
| `<NAME>` | 唯一、精确、区分大小写的快照名称。 |

### 选项

| 选项 | 说明 |
| --- | --- |
| `--parallelism <N>` | 同时处理的文件数上限，正整数，默认 10。 |

### 输出

Console 显示操作、快照名称和结果，下一行是快照 UUID。Raw 输出一行结果，`status` 为 `created`。

与 HEAD 完全相同的工作区会被拒绝，因为再存一份相同的快照没有意义；还没有 HEAD 时可以直接创建。

### 示例

```sh
mcpm stash create "稳定版"
mcpm stash create "实验版" --parallelism 4
```

## `mcpm stash switch`

```text
mcpm stash switch [OPTIONS] <--name <名称>|--id <UUID>>
```

把 `mods/` 恢复成所选快照，并把它设为 HEAD。

### 选项

| 选项 | 说明 |
| --- | --- |
| `--name <名称>`、`--id <UUID>` | 指定一个快照，二者择一。 |
| `--force` | 允许丢弃 `modified` 或 `untracked` 状态下的修改。 |
| `--parallelism <N>` | 同时处理的文件数上限，正整数，默认 10。 |

### 输出

Console 显示操作、快照名称和结果，下一行是快照 UUID。Raw 输出一行结果，`status` 为 `switched`。工作区与 HEAD 一致且所选快照就是 HEAD 时，不改动文件，`status` 为 `unchanged`。

### 行为

- 切换前总是先完整校验该快照，包括带 `--force` 和目标就是 HEAD 的情况。快照内容缺失或损坏时拒绝切换，不会破坏现有的 `mods/`。
- 切换是整体替换：不在快照里的文件会被删除。
- `--force` 只表示允许丢弃 `modified` 或 `untracked` 状态下的修改，不会跳过校验，也不会忽略损坏的快照、无效的 HEAD 或读取工作区时的错误。
- 对 HEAD 使用 `--force` 会丢弃修改，恢复到已保存的版本。
- 不会自动保存被覆盖的修改。想保留，先 `create`。
- HEAD 只在切换成功之后才更新。切换成功但 HEAD 更新失败时，错误会说明文件已经切换。

### 示例

```sh
mcpm stash switch --name 稳定版
mcpm stash switch --id UUID1
# 显式丢弃未保存的修改
mcpm stash switch --name 稳定版 --force
```

## `mcpm stash status`

```text
mcpm stash status [OPTIONS]
```

显示 `mods/` 与 HEAD 的差异：新增、删除、修改的文件。

### 选项

| 选项 | 说明 |
| --- | --- |
| `--parallelism <N>` | 同时处理的文件数上限，正整数，默认 10。 |

### 输出

Console 先显示 `Status:` 和工作区状态；有 HEAD 时再显示它的名称与 UUID，然后逐个列出 `Missing`（缺失）、`Unexpected`（多出）、`Changed`（已修改）的文件；已修改的文件只显示确实不同的大小或 BLAKE3。

Raw 的列：

```text
state id name expected_checksum actual_checksum change path
```

`modified` 时每个变化的文件一行，`change` 为 `added`、`removed` 或 `modified`；`clean` 和 `untracked` 各输出一行，`change` 与 `path` 为空。

### 示例

```sh
mcpm stash status
```

## `mcpm stash list`

```text
mcpm stash list [OPTIONS]
```

按时间从新到旧列出所有快照。不检查 `mods/`，也不打开任何备份，速度快；HEAD 只作为标记显示。

### 选项

| 选项 | 说明 |
| --- | --- |
| `-C <字母>`、`--columns <列表>`、`--column <字段>` | 选择输出的字段；显式选择会替换默认字段。 |
| `--details` | 显示 list 支持的全部轻量字段。 |

### 输出

按[字段](#字段)表的顺序输出所选字段。Console 默认每个快照一行，显示名称和创建时间，HEAD 快照追加 `HEAD` 标记；指定字段或 `--details` 后改为带标签的字段。超过 80 项时先询问 Y/n；stdout 被重定向时不询问。

### 示例

```sh
mcpm stash list
mcpm stash list --columns name,time,head
mcpm stash list --details
```

## `mcpm stash info`

```text
mcpm stash info [OPTIONS]
```

显示选中快照的详细信息。部分字段（校验和、文件数、大小）需要读取快照的文件清单，这不等于校验文件内容。

### 选项

| 选项 | 说明 |
| --- | --- |
| `--name`、`--names`、`--id`、`--ids`、`--input` | 指定一个或多个快照，见[指定快照](#指定快照)。 |
| `-C <字母>`、`--columns <列表>`、`--column <字段>` | 选择输出的字段；显式选择会替换默认字段。 |
| `--details` | 在 Console 追加每个文件的名称、BLAKE3 和大小。 |

### 输出

按[字段](#字段)表的顺序输出所选字段。Console 显示带标签的字段；`--details` 只在 Console 生效，即使同时指定了字段也一样。

### 示例

```sh
mcpm stash info --name 稳定版
mcpm stash info --name 稳定版 --details
```

## `mcpm stash rename`

```text
mcpm stash rename [OPTIONS] <--name <名称>|--id <UUID>> <NEW_NAME>
```

修改一个快照的名称。只改名称；UUID、创建时间、内容和是否为 HEAD 都不变。

### 参数

| 参数 | 说明 |
| --- | --- |
| `<NEW_NAME>` | 新的唯一名称，规则同快照名称。 |

### 选项

| 选项 | 说明 |
| --- | --- |
| `--name <名称>`、`--id <UUID>` | 指定一个快照，二者择一。 |

### 输出

Console 显示操作、快照名称和结果，下一行是快照 UUID。Raw 输出一行结果，`status` 为 `renamed`。改成该快照已有的同一个名称时直接成功，不写入。

### 示例

```sh
mcpm stash rename --name 稳定版 "长期稳定版"
```

## `mcpm stash delete`

```text
mcpm stash delete [OPTIONS]
```

删除选中的快照，`mods/` 不受影响，被其他快照共用的内容也保留。

### 选项

| 选项 | 说明 |
| --- | --- |
| `--name`、`--names`、`--id`、`--ids`、`--input` | 指定一个或多个快照，见[指定快照](#指定快照)。 |

### 输出

Console 每个快照显示操作、快照名称和结果，下一行是快照 UUID；失败时显示你指定的名称或 UUID，没有 UUID 行。Raw 每个快照输出一行，`status` 为 `deleted` 或 `failed`。

### 行为

- 没有额外确认，也没有 `--force`。
- 选中的快照里包含 HEAD，或 HEAD 无法读取时，整批都不会删除。
- 否则每个快照各自删除，单个失败不影响其余，可以对失败的项重试。

### 示例

```sh
mcpm stash delete --name 实验版
mcpm stash delete --ids UUID1,UUID2
```

## `mcpm stash verify`

```text
mcpm stash verify [OPTIONS]
```

检查选中的快照是否完好：备份格式、文件清单、每个文件的大小和完整的 BLAKE3 校验和。只检查已保存的内容，不与当前 `mods/` 比较，也不修改任何文件。

### 选项

| 选项 | 说明 |
| --- | --- |
| `--name`、`--names`、`--id`、`--ids`、`--input` | 指定一个或多个快照，见[指定快照](#指定快照)。 |
| `--parallelism <N>` | 同时处理的文件数上限，正整数，默认 10。 |

### 输出

Console 每个快照显示操作、快照名称和结果，下一行是快照 UUID；失败时显示你指定的名称或 UUID，没有 UUID 行。进度显示在 stderr。Raw 每个快照输出一行，`status` 为 `valid` 或 `failed`。

单项失败不中断后续检查；HEAD 无法读取时报告错误，仍继续检查选中的快照。

### 示例

```sh
mcpm stash verify --name 稳定版
mcpm stash verify --ids UUID1,UUID2 --parallelism 4
```

## 失败与中断

- 不提供中断恢复，也不保证断电时的一致性。命令被中断后先运行 `status` 检查现状；命令不会自动继续上一次的切换。
- `create` 发布失败且清理也失败时，错误会同时报告两者，并指出残留的未引用备份目录。
- 已经提交的文件和 HEAD 变更不会因连接中断而回滚。
- 损坏的快照记录、HEAD 或清单会作为错误报告；部分删除后可能需要按错误提示检查现场。

备份格式与完整性校验见 [Mod 文件备份](../legacy/mod/backup.md)。
