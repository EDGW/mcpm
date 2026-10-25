# 开发原则

## 可执行文件入口

- `src/bin/` 提供五个薄入口：`mcpm`、`mcpm-server`、`mcpmc`、`mcpm-client`、`mcpm-tools`。
- `program.rs` 统一定义程序名称和进程只读的 Server、Client、Tools 命令根；命令提示和示例使用这些命令根。
- `cli/` 将公共选项与命令族已有参数结构组合；客户端入口不提供 SSH 和远端日志选项。
- `app.rs` 负责公共初始化和分发。二进制显式传入程序身份，文件改名不改变命令族。
- SSH 默认启动 `mcpm-tools remote`；远端可执行文件覆盖配置属于连接设置，与本地命令根独立。

## 构建身份

- `build.rs` 统一生成编译期构建身份；`build_info.rs` 提供版本元数据，`display/build_info.rs` 渲染启动提示。
- Commit ID 和工作区状态在构建时从 Git 读取，仅受跟踪文件的已暂存或未暂存变更计为 `modified`；监视仓库受跟踪文件及 Git 索引、引用以刷新构建缓存。
- 非正式版本号由 `build.rs` 在 Cargo 包版本后追加六位 commit ID 和可选的 `.modified`；无 Git 信息时使用 `.unknown`。打包脚本校验整套二进制版本一致，并将实际版本写入清单；提供 Tag 时必须与实际版本一致。
- 正式构建要求同时满足：release 配置、`MCPM_OFFICIAL_RELEASE=1`、`GITHUB_ACTIONS=true`、仓库为 `EDGW/mcpm`、事件为 `push` 或 `workflow_dispatch`、引用类型为 Tag，且 Tag 为 `v<Cargo 包版本>`。
- 发布工作流在 Tag 构建中注入标记；分支、PR、本地和 Fork 构建默认非正式。环境变量改变时 Cargo 重新生成身份。
- `official` 表示正式发布流程构建，也包含官方预发布版本，不表示稳定版。该标记不是来源签名。
- 五个入口共享版本详情和非正式 release 启动提示；提示仅写 stderr，不受日志级别影响。

## 服务端 stash

- `mcpm-backend::local::mc::server::stash` 负责快照业务、索引、HEAD 和锁；前端 `commands/server/stash` 只负责参数、路由和展示协调，`display/stash` 统一呈现。
- 前端每个子命令独立拥有 Clap 参数、输入校验与执行入口，并显式选择共享或独占会话；命令族仅负责路由和公共设置。list/info 分别声明可选列，列合并与校验逻辑共用。
- 后端 `stash/create.rs` 负责快照发布；发布失败且清理失败时，专用错误保留发布错误主链、独立清理错误链和残留备份路径。前端通过既有诊断入口分别报告两条错误链。
- 每个 `.mcpm/stash/index/<UUID>.json` 保存 version=1、id、name、created_at（UTC）和 backup UUID；`.mcpm/stash/HEAD` 保存 version=1 与 stash UUID。两者的 JSON 编解码与验证只在 stash/index.rs 实现。
- backup 引用解析为 `.mcpm/data/<backup-UUID>`，每条记录独立拥有 backup 清单；新建 stash 使用 CacheOnly，文件内容位于服务器 stash blob；文件清单和摘要仅由现有 backup manifest 提供。数据损坏不按缓存派生索引的规则静默重建。
- 服务器 RuntimeEnvironment 的 stash 角色延迟指向 `.mcpm/stash/blob`。backup 的显式缓存创建入口共享原有创建实现，不知道 stash 概念。
- 独占锁覆盖所有修改操作；共享锁保护数据读取和校验。锁位于 `.mcpm/stash/lock`，文件永久保留。首次只读访问缺失目录或尚无锁的空目录时，固定返回空视图，不在无锁状态下继续读取后来发布的数据。
- switch 先完整 verify，再比较工作区；force 不能绕过验证或检查错误。只有 apply 成功后才更新 HEAD；文件发布与 HEAD 更新不是跨文件事务，不提供中断恢复。
- 新工作区的 `.gitignore` 不忽略 stash；data 与 cache 仍按生成模板处理。

- cache 管理命令在分发执行前检查写操作，包含 verify --autoremove 和 export 目标；stash 拥有路径识别，通用 cache 层不依赖 server。server metadata 条目存在即保守保留 stash 缓存位置；--no-protect-stash 绕过 CLI 保护，只读命令的默认路径别名只打开已有缓存，不初始化。

## Backup 缓存回退

- backup 的 v1 manifest 是文件集合及摘要的唯一真相，cache-only 与完整备份共用格式；不持久化缓存路径或存在性副本。
- `backup/create.rs` 共用内容导入与源验证，通过 CreateStorage 明确选择完整导出或仅清单发布；stash 显式选择 CacheOnly。
- `backup/sources.rs` 是 verify/apply 共用的来源选择、完整内容验证和数据集合复查入口；verify_with_cache 不写入或生成目标。
- `backup/apply.rs` 共用严格与可回退应用流程。严格 apply 不访问缓存；apply_with_cache 仅在清单文件实际缺失时调用延迟缓存入口。损坏、非法类型、不可读和额外文件不得触发回退。
- cache 模块负责完整内容哈希和目录协议，backup 负责对照 manifest 的大小验证、来源选择和目标发布；前端不解释缓存布局或清单。
- apply 使用已有缓存，不初始化；完整备份和空清单不打开缓存。全部来源和暂存结果验证通过后才能替换目标。
- display/backup 统一英文风险提示、逐文件来源及统计；cache-only 的 Raw 格式保持原列，警告及缓存依赖提示写 stderr，不受普通日志级别过滤。
- stash 使用 CacheOnly 创建，verify_with_cache 与 apply_with_cache 明确传入服务器 stash 缓存的只读打开入口，绝不回退 global；switch 在独占锁下先完整验证再检查工作区。删除 stash 保留 blob，不做 GC。

## 1. 命令

命令主要按照可复用性分为 **单一命令** 和 **通用命令**。

### 1.1. 单一命令

单一命令归属于一个命令族。比如 `mcpm init`、`mcpm-server init` 和 `mcpm-tools server init` 共享 `server` 命令族中的同一份定义与实现，放在 `src/commands/server/init/mod.rs` 下。可执行文件入口不重复定义命令。

如果这个命令同时支持 `Remote`，则 `Remote` 的入口和 `Remote` 的定义也要放在命令入口旁边。

### 1.2. 通用命令

通用命令有多个入口，比如说 `mcpm-tools mods list`，这个命令可以直接调用，也可以在 `mcpm mods list` 里使用，两者同时还有一些行为区别。

这种命令一般成套出现，比如说 `mods` 命令族对应 `mcpm mods` 和 `mcpm-tools mods`，则这个命令族应该放在 `src/commands/supports/mods/` 下，内部路径也要直接体现命令路径，`Remote` 的入口同上。

值得注意的是，`Remote` 的调用口和命令行的调用口不同。`Remote` 一般直接针对底层做接口，比如说 `mcpm-tools mods list` 和 `mcpm mods list` 都对应一个 `mod.list` 的 `Remote` 命令，`Remote` 模式下两个调用分别对这一个 `Remote` 命令传参数。

所以此时 `Remote` 不该放在命令行的入口上，而是在 `support` 层的统一调用入口上。这类命令的所有调用都要走一个统一的 `support` 入口，不能有各自的实现。

## 2. 参数

命令的参数可以比较复杂，值得注意的是每个命令一般都有个主参数。

### 2.1. 主参数的约定

查询类命令语义上应该支持多个输入，并且应该支持从 `stdin` 按行输入项目。

比如说 `mcpm-tools cache mods info <KEYS>`，应该支持从 `<KEYS>` 和 `stdin` 接收多个参数。

#### 2.1.1. 主参数类型

当 `KEY` 可以是多个类型的时候，比如说 `mcpm-tools mods info` 支持 `modid`、`filename` 等多种类型作为 `key` 时：

- 此时应该用 `--input <type>` 来解释 `stdin` 的输入。
- 此时应该用 `--<specific-type> <value>` 来指明输入。

每个命令**在设计的时候就应该明确是否支持自动推断类型**。

- 如果支持，则 `type` 必须包含 `unspecified` 作为默认项。
- 如果不支持，则 `stdin` 不为空的时候，`--input` 是必须参数，或者要用明确的、和开发者约定的默认值；同时，若有默认值，`<KEYS>` 要按照默认值解释；若无默认值，则不接受直接传入 `<KEYS>`，必须用 `--<specific-type> <value>` 来指明输入。

**示例：支持自动推断类型**

```plaintext
> mcpm-tools mods info --help
...
      --file <FILE>          Inspect a file without falling back to modid or name; repeat to select more
      --modid <MODID>        Match an exact modid; repeat to select more
      --name <NAME>          Match an exact mod display name; repeat to select more
      --input <TYPE>         Interpret stdin lines as names, files, modids, or automatic filename/modid/name queries [default: unspecified] [possible values: name, file, modid, unspecified]
...
```

当主参数只是单个类型（比如说 `cache` 系统的 `BLAKE3 key`）的时候，不需要做这些。

### 2.2. 其余参数约定

#### 2.2.1. 列表参数

当参数是一个列表的时候，要用 `--<plural> A,B,C` + `--<singular> A --<singular> B` 来指定。

注意，有时候参数是一个**选择**而非列表，此时就不适用于这些规定。

比如说在 `mcpm-tools mods source search --source` 里，`--source` 是一个选择，只能有一个值；但是在 `cache mods refresh <KEYS> ident --source --sources` 里，`source` 就是一个列表，适用于本节规定。

**标准示例：**

```plaintext
  -l, --loader <LOADER>      Select loaders, optionally comma-separated; repeat to select more [possible values: forge, neoforge, fabric]
  -L, --loaders <LOADERS>    Select comma-separated loaders; combined with all --loader options [possible values: forge, neoforge, fabric]
```

最终的 `Loader` 集合是合并两类参数后的结果。

**默认值处理规则：**

- **默认值是全集**：此时传入任意一种指定表示彻底放弃所有默认值，只使用指定值。
- **默认值是空集**：此时非常自然地以传入值为准。
- **默认值包含一部分有效值**：此时视情况而定。如果你是 AI，并且你遇到这个问题，你**必须和开发者讨论这个参数的语义**。

当默认值不为空集，但是语义接受空集输入的时候，应该加入 `--no-<plural>` 参数。

值得注意的是，`--no-<plural>` 和 `--<plural>`、`--<singular>` 都是冲突的。

若默认值包含一部分有效值，且你与开发者讨论后决定在用户手动传入参数后执行*追加*到集合的话，要添加 `--no-default-<plural>`，它与 `--<plural>`、`--<singular>` 是不冲突的。

当允许空集的时候，除了 `--no-<plural>` 外，`--<plural> ,` 也是一个输入空集的合法方式。

#### 2.2.2. Loader 列表

`Loader` 列表的全集为 `neoforge,fabric,forge`，一般而言默认为满集且不接受空集。

#### 2.2.3. Source 列表

内置来源为 `modrinth,curseforge`。来源列表一般默认为全集，不接受空集；显式选择覆盖默认值。

#### 2.2.4. Columns 列表

这个列表很特殊，它用来控制输出项，并且默认可能不是满集。

凡是 `Columns` 都要这么实现：

- 默认值包含一部分有效值，但是如果有出现，则是**覆盖语义**而非追加语义。
- 不得为空。
- 有简化命令格式，且简化命令格式的参数往往不一样。

**示例：标准 Columns 格式**

```plaintext
  -C <INMFL>          Select columns by letters: I=index, N=name, M=modid, F=filename, L=loaders

  --columns <COLUMNS> Select comma-separated columns, combined with -C and --column
                      [possible values: index, name, modid, filename, loaders]

  --column <COLUMN>   Select one column; repeat to select multiple columns
                      [possible values: index, name, modid, filename, loaders]
```

主要是 `-C` 简写。这个 `-C` 简写后面的参数都是单字母简写，且无需逗号。

## 3. 输出

输出分为 `Raw`、`Console`、`TUI` 三种模式。

`mcpm` 的最基本形式是命令，每个命令可以提供多种模式，`--help` 中要指出支持的模式。

**所有命令都要支持 `Console` 和 `Raw` 模式。**

用 `--raw` 指定 `Raw` 模式，有 `TUI` 模式的命令可以用 `--no-ui` 来指定 `Console` 模式，`--no-ui --raw` 的作用与 `--raw` 相同。

默认使用顺序最靠前的模式打开，当检测到 `stdout` 不是 `TTY` 的时候自动使用 `Raw` 模式。

进度条等输出的显示用 `stderr` 检测，进度条默认输出到 `stderr`。

`stdin` 不参与检测。

### 3.1. Raw 模式输出规范

`Raw` 模式一般设计为输出 `TSV` 格式，便于机器自动处理。每一行一个数据。

当数据层次较为复杂，不是简单的表，或者数据本身可能含有 `Tab`、换行等符号的时候，可以考虑输出为 `JSON` 格式。但是如果你是 AI，此时要和开发者协调，不得擅自决定。

`Raw` 模式不输出表头，但是可以用 `--headers` 让它输出表头。`--headers` 是仅 `Raw` 模式生效的参数。

`Raw` 模式一般仅仅输出本职数据。

比如说 `mcpm-tools cache mods list`，这个命令的本职数据就只包含 `cache` 的 `BLAKE3 HASH`、`FileName`、`Size`、`Sha256`、`Metadata Names` 等数据，可以输出为这种形式：

```plaintext
<blake3>\t<filename>\t<sha256>\tident,meta
```

### 3.2. Console 模式输出规范

`Console` 模式要用 `Primary Color`、`Secondary Color` 等等呈现，主要要求可读性。

不必列成 `TSV`，而是列成最 user-friendly 的方式。

`Console` 模式可以列出附加数据，来避免用户执行更多命令。

还是比如说 `mcpm-tools cache mods list`，虽然这个命令的本职数据就只包含 `cache` 的 `BLAKE3 HASH`、`FileName`、`Size`、`Sha256`、`Metadata Names` 等数据，但是建议这么输出：

```plaintext
<BLAKE3>  available
  File: <file>
  Size: xxx bytes
  SHA-256: <sha256>
  2 Metas:
    ident:
    ...
    meta:
    ...
```

此时就建议把 `Meta` 列出，让用户不用多次查询。

### 3.3. TUI 模式输出规范

TODO.

### 3.4. 数据输出规范

在 `Raw` 和 `Console` 模式下，每个数据类型一般要有一个统一的输出格式。

比如说 `ModMeta` 和 `CachedModMeta`，在不同地方都要有统一的输出格式，保持一致性。

这种统一格式的输出代码统一放到 `src/display/xxx` 下，这样才能保持各种输出的一致性。

错误统一经 `diagnostics::error` 报告；有进度条时使用 `Progress::report`。诊断入口负责日志记录、
远端错误标记和进度协调，`src/display/error.rs` 负责 `error:` / `remote error:` 与逐层
`caused by:` 的显示。保留错误上下文，不要直接打印错误或将错误链拼成单行。

Console 渲染使用 `src/display/console` 的 `ConsoleWriter`：

- 在 Console 输出入口用 `ConsoleWriter::new` 包装输出流，子渲染器接收已有 Writer。
- 通过 `subwriter()` 创建下一级 Writer，每级缩进由唯一的 `INDENT` 常量定义，为两个空格；渲染器不手写缩进。
- 子 Writer 借用父 Writer，可继续创建子 Writer；借用结束后，父 Writer 可继续输出。
- 使用标准 `write!`、`writeln!` 或 `std::io::Write` 写入；跨多次写入保留行状态，空行不添加空格。
- Raw 直接使用原始输出流。

```rust
let mut console = ConsoleWriter::new(output);
writeln!(console, "Metadata:")?;
{
    let mut metadata = console.subwriter();
    writeln!(metadata, "meta:")?;
    let mut details = metadata.subwriter();
    writeln!(details, "Status: available")?;
}
writeln!(console, "Next entry")?;
```

## 4. 常见命令类型规范

### 4.1. `info`

`info` 类命令一般要支持多个主参数，同时兼容 `stdin` 和命令行输入，参照上面的规范。

然后输出比较复杂的 `info` 类命令要支持 `columns`。

### 4.2. `list`

`list` 命令与 `info` 命令不同的是，`list` 一般不接受主参数。

`list` 命令一般要支持 `columns`，且 `Raw` 模式下要对自动化处理友好。

如果 `list` 是树形数据，比如说 `Mod` 和 `Nested Mods`，一般而言可以以某种方式展示这些子数据的存在（如 `mods list` 的 `(+x mods)`），但是默认不列出，用 `--expanded` 等参数控制列出。没有折叠数据的命令不提供 `--expanded`；普通详情使用 `--details`。

**Console 模式下，List 的语义一般如下：**

- **默认**：只列出最重要的几个 `Column`，比如说 `cache` 的 `BLAKE3`、`mods` 的 `ModName + ModId`。当列出的 `column` 数量很少、且 `expanded` 未打开时，可以在一行输出多个（此时不显示 `Header`），参考 Linux `ls` 的行为。
- **`--details`**：默认等于列出所有 `column`，但是不默认启用 `expanded`。
- **`--columns`**：只要 `--columns` 被明确指定，就不能一行输出多个，并且必须显示 `Header`。
- Console 模式下不一定要显示成表，Columns 可以指广义的字段名，也可以显示成 `XX: XX` 的格式，只要有助于阅读。如果不列成表而列成字段组，每一个项目之间要有空行。默认模式下若选择字段组且只列出一个字段的时候，此时和列表其实没有差别，所以 Header（若有）放在最上面而不是每个前面，也不需要有空行。
- 超过 80 项的时候要在控制台提醒用户是否真的需要阅读，用 `Y/n` 决定。
- 没有折叠的数据的命令不用加上 `--expanded`。

例子：

```plaintext
# 列表-详细
Name    Id      File
<name>  <id>    <file>
<name>  <id>    <file>
<name>  <id>    <file>

# 字段组-详细

Name: <name>
Id: <id>
File: <file>

Name: <name>
Id: <id>
File: <file>

Name: <name>
Id: <id>
File: <file>

# 列表/字段组 只有单个输出 A 情况（适用于数据较短的命令）
<name>  <name>  <name>  <name>
<name>  <name>  <name>

# 列表/字段组 只有单个输出 B 情况（适用于数据较长的命令）
Header
<name>
<name>
<name>
<name>
<name>
```
