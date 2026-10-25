# Mod 文件备份

`mcpm-tools mod backup` 创建、校验、应用和删除完整文件夹的文件备份，也可以计算目录摘要并与备份比较。备份递归包含全部普通文件，
不限于 `.jar`，也包含禁用的 Mod；不解包归档、不记录空目录。操作仅支持本地。

## 用法

```sh
mcpm-tools mod backup create ./mods ./backups/before-update
mcpm-tools mod backup create ./mods ./backups/before-update --mode copy
mcpm-tools mod backup create ./mods ./backups/before-update --mode prefer-hard-link --parallelism 4
mcpm-tools mod backup apply ./backups/before-update ./mods
mcpm-tools mod backup apply ./backups/before-update ./mods --force
mcpm-tools mod backup apply ./backups/before-update ./mods --mode hard-link
mcpm-tools mod backup verify ./backups/before-update ./backups/another
printf '%s\n' './backups/before-update' './backups/another' | mcpm-tools mod backup verify
mcpm-tools mod backup checksum ./mods ./another-mods
printf '%s\n' './mods' './another-mods' | mcpm-tools mod backup checksum
mcpm-tools mod backup verify-with ./mods --backup ./backups/before-update
mcpm-tools mod backup verify-with ./mods --checksum <CHECKSUM>
mcpm-tools mod backup delete ./backups/before-update ./backups/another
printf '%s\n' './backups/before-update' | mcpm-tools mod backup delete
```

创建前，备份目录的父目录必须存在；备份目录本身必须不存在或为空。
应用前，目标的父目录必须存在；目标必须不存在或为空，除非指定 `--force`。

## 参数

| 参数 | create | apply |
| --- | --- | --- |
| 位置参数 | `<SOURCE> <BACKUP>` | `<BACKUP> <TARGET>` |
| `--mode MODE` | 缓存到备份，默认 `hard-link` | 备份到目标，默认 `copy` |
| `--cache-dir PATH` | 指定已初始化的内容缓存；默认使用 `~/.mcpm/cache/mods` | 不使用缓存 |
| `--parallelism COUNT` | 每阶段同时处理的文件数，默认 `10` | 同左 |
| `--force` | 不提供 | 完整替换非空目标，删除备份中不存在的旧文件 |
| `--details` | Console 展示逐文件路径、BLAKE3、大小和实际传输方式 | 同左 |
| `--headers` | Raw 输出 TSV 表头 | 同左 |
| `--raw` | 禁用终端格式、进度和颜色 | 同左 |

`--parallelism` 必须为正整数，使用 `1` 顺序处理。
`--mode` 复用缓存命令的传输策略：

- `copy`：复制，不与来源共享文件内容。
- `hard-link`：必须建立硬链接；跨文件系统等失败直接报错。
- `prefer-hard-link`：优先硬链接，仅在文件系统不支持时回退复制；权限、路径等错误不会回退。

## 创建行为

1. 递归枚举全部普通文件。符号链接、特殊文件和不合法路径会导致失败。
2. 通过 Cache 接口复制导入或复用内容，并校验源文件及缓存内容。不会计算、刷新或备份 `meta`、`ident`。
3. 全部源文件校验通过后，按 `--mode` 从缓存生成 `data/` 中的文件，保留相对路径。
4. 校验生成结果，保护文件内容和 `data/` 目录树，最后写入并保护 manifest 内容后发布备份。

`manifest.mcbackup` 与数据文件均不可原地写入。创建成功后的 Console 提示使用
`mcpm-tools mod backup delete <BACKUP>` 删除备份；Unix 默认目录权限会阻止普通递归删除。

源文件导入缓存固定使用复制，`--mode` 只决定缓存到备份的传输方式。
因此创建操作不会将原本独立的源文件变成只读文件；原先就与缓存硬链接的文件仍共享原有保护。
重复内容只占用一个缓存项，manifest 仍分别记录各文件路径。
空文件夹可以备份，但空子目录不会恢复。

创建失败时不会发布不完整备份；本次已成功导入的缓存项保留。
源目录、备份目录和缓存目录之间不得重叠。

## 应用行为

1. 校验 manifest 的格式版本、路径和语义 checksum。
2. 确认 `data/` 中全部普通文件与 manifest 一致，逐文件校验大小和 BLAKE3。
3. 在目标同级临时目录生成并校验新文件，全部通过后替换目标。

默认复制得到可写文件。硬链接与备份共享文件内容及写保护；更新时应替换文件，避免原地修改。
备份是自包含的，即使原缓存项已被删除，也可以应用。

`--force` 不跳过校验，也不合并目录。提交前保留旧目标，提交失败时尝试恢复；
恢复失败或旧目录无法清理时，错误会指出保留的恢复目录。目录切换不提供断电后的事务恢复。
备份与目标不得重叠。目标平台无法表示的路径或名称冲突会导致应用失败。

## 单独校验

```text
mcpm-tools mod backup verify [OPTIONS] [BACKUP]...
```

- 位置参数接受多个备份目录，也可以从 stdin 每行读取一个目录；两者合并，先处理命令行参数。
- 空行忽略，路径中的空格保留；stdin 中的引号按文件名内容处理，不解释为 shell 引号。
- 完全相同的路径只处理一次，保留首次出现顺序；合并后至少需要一个路径。
- 校验 manifest 格式和 checksum、`data/` 的完整文件集合，以及每个文件的大小和完整 BLAKE3。
- 不解包归档、不应用备份、不生成目标目录，也不打开或填充缓存。
- 多个备份顺序处理。`--parallelism COUNT` 限制每个备份内并行校验的文件数，默认 `10`，必须为正整数。
- 单个备份失败后继续检查其余备份；全部通过返回 `0`，任何失败返回非零退出码。
- `--details` 在 Console 中增加各文件的路径、BLAKE3 和大小；`--headers` 添加 Raw 表头。

Console 按备份展示 `Backup`、`Status`、`Files`、`Size`、`Checksum`，并复用详细校验诊断。
`Size` 包含 manifest 的原始字节数。校验不执行传输，不报告复制、硬链接或缓存占用估计。
Raw 按输入顺序每个备份输出一行，无默认表头，列为：

```text
backup  status  files  size  checksum
```

`status` 为 `valid` 或 `invalid`；校验失败时后三列为空。
成功时 `backup` 为解析后的绝对目录，失败时保留输入路径；`size` 为整数字节数。

## 计算目录摘要

```text
mcpm-tools mod backup checksum [OPTIONS] [DIRECTORY]...
```

- 每个目录独立计算一个与 v1 backup checksum 相同规则的摘要，不合并多个目录。
- 多个目录参数和 stdin 逐行输入的合并、空行、空格、去重及处理顺序与 `verify` 相同，至少提供一个目录。
- 递归包含全部普通文件，包括禁用 Mod 和其他扩展名；不解包、不识别 Mod。空目录不参与摘要。
- 相对路径按组件拆分并统一使用 `/`，因此相同名称和内容在不同操作系统下使用相同编码。
  不折叠大小写，不进行 Unicode 归一化；文件名本身包含反斜杠时拒绝处理。
- 读取完整内容计算 BLAKE3，不使用缓存中的摘要；不写入文件、不创建备份、不打开缓存或改变权限。
- 根目录或子项为符号链接、特殊文件、不合法路径时失败。读取期间检测到文件变化，或结束时检测到文件集合变化，也会失败。
  这不提供整个目录的原子快照，计算期间应保持目录稳定。
- 目录依次处理，单个失败后继续处理其余目录。`--parallelism COUNT` 限制每个目录内并发文件数，默认 `10`。
- 全部计算成功返回 `0`，任何失败返回非零退出码；错误通过 stderr 逐级展示。
- 支持 Console 和 Raw。`--details` 在 Console 展示逐文件记录；`--headers` 添加 Raw 表头。

Raw 每个成功目录只输出一个 checksum，每行一个，不附带路径、状态、文件数量或大小：

```text
<checksum>
```

失败目录不输出 stdout 行，错误写入 stderr，继续计算后续目录；任何失败仍返回非零退出码。
默认无表头，`--headers` 添加一行 `checksum` 表头。
Console 另外展示目录、状态、文件数量和大小；大小只统计文件内容字节，不包含 manifest。

## 与目录比较

```text
mcpm-tools mod backup verify-with [OPTIONS] <MODS_DIR>
```

```sh
mcpm-tools mod backup verify-with ./mods --backup ./backups/before-update
mcpm-tools mod backup verify-with ./mods --checksum <CHECKSUM>
mcpm-tools mod backup verify-with ./mods --backups ./backups/a,./backups/b --checksum <CHECKSUM>
printf '%s\n' './backups/a' './backups/b' | mcpm-tools mod backup verify-with ./mods --input backup
mcpm-tools mod backup checksum ./mods | mcpm-tools mod backup verify-with ./another-mods --input checksum
```

- `<MODS_DIR>` 使用 `checksum` 的完整目录计算规则，只计算一次，复用于全部比较目标。
- `--backup BACKUP` 指定一个备份，允许重复；`--backups BACKUPS` 指定逗号分隔的备份列表。
  单个 `--backup` 中的逗号按路径内容处理。
- `--checksum CHECKSUM` 指定一个预期摘要，允许重复；`--checksums CHECKSUMS` 指定逗号分隔的摘要列表。
  摘要必须为 64 位小写十六进制，按当前 v1 协议解释。
- 两类目标可以混合。stdin 每个非空行是一个目标，有输入时必须用 `--input backup|checksum` 指明类型，不自动猜测。
  路径空格和引号保留，空行忽略。
- 顺序依次为 `--backup`、`--backups`、`--checksum`、`--checksums`、stdin，各组内保留输入顺序。
  同一类型的相同目标只比较一次；合并后至少需要一个目标。
- 备份目标只读取并验证 manifest，不检查 `data/`，因此只有 manifest 的备份目录也可以用于比较。
  检查备份自身完整性仍使用 `backup verify`。
- Console 显示匹配结果、预期和实际摘要；manifest 不匹配时列出全部缺少、多出、大小或内容变化的文件。
  只有摘要时无法列出文件差异。`--details` 增加实际目录的逐文件记录。
- 不写入目录、备份或缓存。`--parallelism`、文件变化检测和稳定目录要求与 `checksum` 相同。
- 单个目标读取或解析失败后继续比较其余目标。全部匹配返回 `0`，任一不匹配或执行失败返回非零。

支持 Console 和 Raw；Raw 每个目标输出一行 TSV，`--headers` 添加表头：

```text
target_type  target  directory  status  expected_checksum  actual_checksum
```

`target_type` 为 `backup` 或 `checksum`；`target` 保留输入值。`status` 为 `match`、`mismatch` 或 `failed`。
不可获得的摘要留空；目录计算成功后使用其绝对路径，否则保留输入路径。
不匹配属于比较结果；读取、格式和目录计算错误通过现有逐级诊断输出到 stderr。

## 删除备份

```text
mcpm-tools mod backup delete [OPTIONS] [BACKUP]...
```

- 支持多个目录参数和 stdin 每行一个目录；合并顺序、空行、空格和去重规则与 `verify` 相同，至少提供一个路径。
- 删除整个备份目录，包括 `data/` 和 manifest；不会打开缓存，不会删除缓存项或其他硬链接。
- 删除前检查 manifest 的格式与 checksum；根目录必须是普通目录，且不能包含 `data` 和 `manifest.mcbackup` 以外的项目。
- 不要求数据文件通过完整校验，允许清理文件缺失或内容损坏的备份。`data/` 内的全部内容都会删除，包括 manifest 未记录的文件。
- 允许 `data/` 已经不存在，便于继续清理部分删除的备份；存在时必须是普通目录。拒绝符号链接形式的备份根目录或 `data/`。
- 不跟随 `data/` 内的符号链接，只删除链接自身，不影响链接目标。
- 临时解除目录的删除保护，不修改任何文件的权限或 ACL，因此缓存与其他硬链接仍保持写保护。
- 备份依次删除，失败后继续处理其余备份；全部成功返回 `0`，任何失败返回非零退出码。没有额外的 `--force` 或确认步骤。
- I/O 错误可能留下部分删除的备份；数据目录删除完成后才删除 manifest，最后删除备份根目录。

Unix 上，`data/` 的写保护阻止普通文件修改和逐项删除，也会阻止普通递归删除。
Windows 的保护禁止原地修改文件内容；如果文件自身具有 `DELETE` 权限，目录保护不能阻止删除该文件。
本命令提供跨平台的备份清理流程，不扩大共享文件的 ACL 拒绝范围。
Console 按备份显示路径和完整句子的结果提示。Raw 保留输入路径，每个备份输出一行 TSV：

```text
backup  status
```

`status` 为 `deleted` 或 `failed`；默认无表头，`--headers` 添加表头。错误通过 stderr 逐级展示原因。

## 备份格式

```text
backup/
├── manifest.mcbackup
└── data/
    ├── example.jar
    └── disabled/
        └── another.jar.disabled
```

`manifest.mcbackup` 是 JSON，版本 `1` 的字段为：

```json
{
  "version": 1,
  "files": [
    {
      "path": "example.jar",
      "blake3": "<64位小写十六进制>",
      "size": 123456
    }
  ],
  "checksum": "<64位小写十六进制>"
}
```

- `path` 是以 `/` 分隔的 UTF-8 相对文件路径。禁止空组件、`.`、`..`、反斜杠、NUL 和首组件中的冒号。
- `blake3` 同时是文件内容哈希和缓存键。`size` 使用字节数。
- 不记录目录、缓存绝对路径、传输方式、权限、修改时间或命名 metadata。
- 拒绝未知字段、重复 JSON 字段、重复文件路径，以及文件路径兼任父目录的情况。
- `data/` 中缺少文件、多出文件、存在符号链接或特殊文件均视为校验失败。额外空目录不影响文件集合。

checksum 对解析后的记录计算，与 JSON 排版、字段顺序、文件记录顺序及等价字符串转义无关。
文件按路径 UTF-8 字节升序排列，将以下字节拼接后计算 BLAKE3：

1. 固定格式标识：UTF-8 字节 `mcpm.mcbackup`，后接一个 NUL 字节。
2. `version`：大端 `u32`。
3. 文件数量：大端 `u64`。
4. 每个文件依次编码：路径 UTF-8 字节长度（大端 `u64`）、路径字节、BLAKE3 原始 32 字节、大小（大端 `u64`）。

文件名字符串不进行 Unicode 归一化；不同名称保持不同语义。
checksum 检查文件映射的完整性，不认证备份来源。

## 输出

Console 显示各阶段进度、完整句子的完成提示及最终摘要；`--details` 添加逐文件字段组。
摘要的大小按内容字节统计，不查询文件系统的实际分配空间：

- `Size`：全部文件大小，加上 manifest 文件的实际字节数（包括 JSON 排版）。
- `Actual Size`：复制文件大小，加上 manifest 文件大小；硬链接共享的内容不重复计入。
- 创建时共享部分显示为 `(… cached)`；应用时直接链接到备份，显示为 `(… shared)`。
- 应用摘要也计入读取的 manifest 大小；manifest 不会复制进目标 Mod 文件夹。
- 大小显示为 `1.50MiB (1572864 bytes)` 等形式，括号与精确字节数使用灰色。
- 存在硬链接时显示英文提示，说明复制、传输或移到无法保留硬链接的位置可能再次占用这些空间，并提示使用 `--mode copy`。

stdout 非 TTY 时自动使用 Raw；进度由 stderr 独立检测，显式 `--raw` 禁用进度。

创建和应用的 Raw 只在成功后输出一行 TSV，默认无表头，列依次为：

```text
operation  destination  files  size  actual_size  shared_size  checksum  copied  hard_linked
```

`operation` 为 `create` 或 `apply`，`destination` 为解析后的绝对目标路径。
`size`、`actual_size`、`shared_size` 均为整数字节数，其中 `shared_size` 为硬链接共享的文件内容大小，不含 manifest。
路径中的反斜杠和控制字符按现有 TSV 规则转义。`--details` 在 Raw 中不改变输出。
错误写入 stderr 并逐级展示原因，返回非零退出码。校验失败额外展示缺少及多出的文件，或内容不符文件的路径、预期值和实际值。
