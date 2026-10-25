# Mod 缓存管理

`mcpm-tools cache mods` 显式管理 `~/.mcpm/cache/mods` 中的内容和具名 metadata。

- 一个条目由完整的 64 位小写十六进制 BLAKE3 标识；相同内容只保存一次。
- `meta`、`ident` 是两个 metadata 名字，分别保存 Loader 声明和来源匹配。
- `import` 只存内容，不计算、刷新或读取这两个 metadata。
- `mods info/list` 和 `mods source identify` 默认只读查询缓存；显式写入也可使用
  [`mods cache`](mod/cache.md) 或 [`server mods cache`](server/mods.md#缓存)。
- 缓存管理在本地执行。所有命令支持 Console、Raw；`--raw` 或非 TTY stdout 使用 Raw。
- Raw 的 `list`、`info` 和简单操作结果采用 TSV；refresh 成功时无输出，退出码为 0。

## 命令

```sh
mcpm-tools cache mods init --directory /path/to/cache
mcpm-tools cache mods import example.jar another.jar
mcpm-tools cache mods import example.jar --mode hard-link
mcpm-tools cache mods list
mcpm-tools cache mods list --columns blake3,size,metadatas
mcpm-tools cache mods list --details
mcpm-tools cache mods info HASH1 HASH2 --columns blake3,status,metadatas
mcpm-tools cache mods info --all
printf '%s\n' HASH1 HASH2 | mcpm-tools cache mods info
mcpm-tools cache mods refresh HASH1 HASH2 meta --loaders fabric,forge
mcpm-tools cache mods refresh HASH1 ident --source modrinth --source curseforge
mcpm-tools cache mods refresh --all meta
mcpm-tools cache mods refresh --all meta --parallelism 4
mcpm-tools cache mods refresh --all ident --details
mcpm-tools cache mods refresh HASH1 meta --short
printf '%s\n' HASH1 HASH2 | mcpm-tools cache mods refresh ident
mcpm-tools cache mods verify HASH1 HASH2
mcpm-tools cache mods verify --all
mcpm-tools cache mods verify --all --parallelism 4
mcpm-tools cache mods verify --all --details
mcpm-tools cache mods verify --all --autoremove
mcpm-tools cache mods export HASH1 restored.jar another-copy.jar
mcpm-tools cache mods remove HASH1
mcpm-tools cache mods remove --all
mcpm-tools cache mods rebuild-index
```

| 命令 | 行为 |
| --- | --- |
| `init` | 初始化不存在或空目录，也接受合法的已有缓存 |
| `import PATH...` | 按内容导入或复用文件，不计算 metadata |
| `list` | 枚举条目，包括异常内容类型和孤立 metadata；默认只有 BLAKE3 |
| `info [HASH]...` | 查询内容信息及选中的 metadata，不执行识别或刷新 |
| `refresh [HASH]... meta` | 刷新选中的 Loader 结果 |
| `refresh [HASH]... ident` | 刷新选中的来源结果 |
| `verify [HASH]...` | 检查内容的 BLAKE3，不读取或验证 metadata |
| `export HASH PATH...` | 导出到不存在的目标，父目录必须已存在 |
| `remove [HASH]...` | 删除选中内容及其受管理的 metadata；`--all` 选择整个库存 |
| `rebuild-index` | 从验证通过的内容重建可选 imohash 索引 |

`--directory PATH` 可放在 mods 子命令前后。指定目录必须已初始化；默认目录不存在或为空时初始化。
目录格式为 version 3；拒绝非空无标记目录、损坏 manifest 和不兼容版本，不自动迁移旧目录。

## Stash 缓存保护

- 服务器 `.mcpm/stash/blob` 保存快照依赖的内容，不应手动管理；删除或修改内容可能导致 stash 无法使用。
- `init`、`import`、`refresh meta/ident`、`remove`、`rebuild-index` 和 `verify --autoremove`
  在识别到 stash 缓存后于执行前拒绝，即使选择为空或本次可能只会复用已有内容。
- `list`、`info`、普通 `verify` 及导出到 stash 缓存之外仍可使用。
  导出到另一服务器 stash 缓存也会在本次任何导出前被拒绝。
- `--no-protect-stash` 显式绕过保护，可放在子命令前后；缓存自身校验和原有导出限制仍然生效。
- 同时检查传入路径和解析后的路径，覆盖相对路径、符号链接别名和尚未创建的缓存目录。
  缺失路径的大小写变体保守保留；大小写敏感文件系统上的不同已有目录通过原生文件身份区分。
  服务器的 `server.json` 条目存在即保留其固定 stash 缓存位置，即使 metadata 损坏；子路径也受保护。
  这是本地路径检查，不是文件系统访问控制，无法发现无关联的反向符号链接引用，也不阻止其他进程在检查后替换路径。

## 输入与列选择

- `info`、`verify`、`refresh`、`remove` 合并参数与重定向 stdin；每个非空行是一个完整 HASH。
- 参数在前，stdin 在后；按首次出现的顺序去重，不使用 `--all` 时合并后至少需要一个 HASH。
- `info`、`verify`、`refresh`、`remove` 支持 `--all`，按 BLAKE3 顺序选择完整缓存清单，
  包括孤立 metadata 和异常内容类型；与 HASH 参数及非空 stdin 输入互斥。
  空缓存成功结束，不处理任何条目。选取条目不解释 metadata 内容。
  info 可以展示孤立 metadata 和异常类型；refresh 遇到无法处理的条目时停止，verify 继续校验全部选中条目。
- refresh、verify 使用 `--all` 时，可用 `--parallelism COUNT` 控制同时处理的条目数量，默认 `10`。
  COUNT 必须为正整数，设为 `1` 即顺序处理；显式 HASH 输入仍顺序处理。
  输出保持选择顺序（`--all` 时为 BLAKE3 顺序），不受任务完成先后影响。info 不接受该参数。
  refresh 单个条目内的来源仍按顺序执行，避免同一条目的来源并发写入。
- `refresh` 的 HASH 放在 `meta` 或 `ident` 前面。
- 列可通过 `--columns A,B`、重复 `--column A` 和 `-C BFSHTM` 合并选择；显式选择覆盖默认列。
- `list` 默认只有 `blake3`。只有选择 `metadatas` 列时，才展示条目的 metadata。
  `--details` 在没有显式列选择时启用所有列。
- Console 单字段时每项一行，显式选择的字段标签只在顶部显示一次；默认 BLAKE3 没有标签。
  多字段时按字段组逐行显示标签和值，条目之间留一个空行。超过 80 个条目时先用 Y/n 确认，回车默认继续。
  Metadatas 列通过共享 Display 展示 metadata 状态及完整详情。
  Loader 声明沿用 `mods info` 的展示格式：明确未识别的 Loader 也会显示，未检查的 Loader 不显示。
- Raw 的 `list`、`info` 共用列定义与 TSV 渲染，每个条目一行；Metadatas 单元格只列排序后的名字，
  不解释快照。字段使用反斜杠转义控制字符及字面反斜杠。
  默认无表头，`--headers` 输出一次表头；该参数不影响 Console 和 refresh。
- `info` 默认全部列；Console 展示每个选中 metadata 的状态和完整详情，来源匹配按字段组展示。
  Raw 只列 metadata 名字。

| 列 | `-C` | 含义 |
| --- | --- | --- |
| `blake3` | B | 条目内容 ID |
| `file` | F | 缓存内容路径，不是原始导入文件名 |
| `size` | S | 实际内容文件的字节大小 |
| `sha256` | H | 有效 meta 已保存的 SHA-256；缺失时为空，不临时计算 |
| `status` | T | 内容结构状态；Console 在不可用时同时显示原因 |
| `metadatas` | M | Raw 列排序后的名字；Console 展示已知名字的状态和完整详情，其他名字只显示存在 |

内容结构的 `available` 仅表示普通内容文件存在；`not_found` 表示没有内容文件。
完整内容身份由 `verify` 检查。meta、ident 各自的状态独立，缺失或损坏不会使完整内容验证失败。
`--structure-only` 只检查普通内容文件的存在，不能发现同尺寸内容损坏。

## 校验行为

- 默认只展示校验失败的条目（`unavailable` 或 `not_found`）；`--details` 在 Console 和 Raw 中
  都包含 `available` 条目，结果保持选择顺序。
- 单个条目失败不终止校验；每个错误通过 stderr 展示完整错误链。
  所有条目处理完毕后，只要存在校验失败，退出码就非 0。
- `--autoremove` 使用与 `remove` 相同的行为，删除校验失败条目的内容及关联 metadata。
  校验通过的条目、已导出的副本及外部硬链接保留。删除失败会报告错误并继续校验，不递归删除异常目录。
- `--structure-only` 同样决定 `--autoremove` 使用的校验方式，不能发现内容损坏。
- stderr 为终端时显示进度条，按完成校验及可选删除的条目推进；`--raw` 关闭进度条。
  仅重定向 stdout 时，stderr 若仍为终端则继续显示进度。
- 最后通过 stderr 汇总已校验、通过、失败的条目数；启用 `--autoremove` 时还统计已删除、
  已不存在、删除失败的条目数。删除成功的条目仍计入校验失败。
- `--headers` 在第一条展示的结果前输出一次 Raw TSV 表头；所有结果都隐藏时不输出表头。

## 刷新行为

- Console 选中超过 3 个条目时默认 short；数量以参数与 stdin 合并去重后的 HASH 数量，
  或 `--all` 选中的缓存条目数量为准。
- short 每条一行，只展示完整 BLAKE3 和 `success` / `failed`。
  复用已有结果、查询成功但未匹配均算成功；错误详情仍输出到 stderr。
- 不超过 3 个条目时，Console 默认详细展示，先用与 info 相同的字段组展示 BLAKE3、Status、File，
  再展示 Loader 或来源详情。Status 表示缓存内容状态，与某个来源是否匹配独立。
- `--details` 强制详细展示，`--short` 强制 short，两者互斥。
  这两个参数和 `--all` 可放在 meta/ident 前后；HASH 参数须放在子命令前。
- Raw（包括 stdout 重定向）成功时无输出，退出码为 0；`--headers` 不生效。
  `--details`、`--short` 在 Raw 中也不影响输出。刷新后的缓存可用 info 或 list 查询。
- 错误统一经共享错误渲染器输出到 stderr，保留每层 `caused by:` 上下文，退出码非 0。
- meta 默认选中 forge、fabric、neoforge；`--loaders A,B` 与重复的 `--loader A` 可以组合，
  两种形式都接受逗号分隔。显式选中后只使用所选 Loader。
- 当前版本的识别和成功未识别结果会复用；`--force` 强制重新识别所选 Loader。
  收齐选中结果后一起发布，保留未选中的当前结果；失败保留原快照。
- ident 默认依次选中 modrinth、curseforge；使用 `--sources A,B` 或重复 `--source A` 指定。
  `curseforge@KEY` 使用官方 API；同一来源只能选择一次。
- 来源只保存验证通过的成功匹配；未匹配不形成负缓存。`--force` 跳过已有匹配的复用，
  查询错误或没有匹配时仍保留此前成功记录。
  多个候选均通过校验、无法唯一确定匹配时，按未匹配处理。
- Console ident 详细模式逐来源报告本次结果：`reused` 表示复用已有匹配、没有请求来源；`matched` 表示
  查询得到匹配且验证后存储；`unmatched` 表示查询成功但没有匹配，不存储该结果。
- Console ident 详细模式按来源分组列出字段，展示本次结果与已保存的匹配详情。
  所有来源均未匹配且没有此前成功记录时，不创建 ident 快照；Console 显示 `Stored matches: none`。
- Console 中未匹配的来源仍可能显示保留下来的旧匹配；本次未匹配不会删除已保存的匹配。
- 一个 HASH 的来源按选择顺序写入；后续来源失败时，前面完成的写入保留。
- 条目失败后停止启动它之后的新任务；已在执行的任务会收尾，期间仍可能完成写入。
  输出到选择顺序中的首个失败条目为止；成功写入保留，不回滚批次。

## 导入、导出及 Raw 回执

- 导入、导出按输入顺序逐项执行；每成功一项输出一条记录，首个失败后停止。
- `--mode copy` 默认复制；`hard-link` 要求硬链接；`prefer-hard-link` 只在文件系统不支持时回退复制。
- 导入已有合法对象时直接复用，不替换内容，不关联或保护另一份独立源文件。
- 硬链接共享内容与写保护。更新时写新文件再替换自己的文件名，避免原地修改。
- 导出不覆盖，不创建父目录；删除缓存不影响已导出的副本或硬链接。
- TSV 字段使用反斜杠转义控制字符及字面反斜杠。空 method 表示复用、没有发生传输。

| 操作 | TSV 字段（依次） |
| --- | --- |
| `init` | directory, version |
| `import` | blake3, file, published, method |
| `export` | blake3, destination, method |
| `verify` | blake3, status, reason |
| `remove` | blake3, removed |
| `rebuild-index` | indexed |

verify 回执只展示状态；错误详情统一经 stderr 的错误链渲染器输出。
TSV 中保留 `reason` 字段，值为空。

## 并发

- 内容发布后不可变，metadata 按完整快照原子发布。
- 并发更新可以丢失部分结果，删除后也可能被另一个发布者重新创建；不提供批次事务。
- 可复用 metadata 必须关联有效内容；孤立 metadata 可观察、删除，但不能复用。
- imohash 索引只提供候选；查询验证内容身份并在需要时回退完整 BLAKE3。索引可显式重建。

[命令参考](README.md) · [English](../docs/cache.md)
