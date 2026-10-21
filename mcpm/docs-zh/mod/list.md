# `mcpm mod list`

```text
mcpm mod list [OPTIONS] [PATH]...
```

列出 Mod 文件，可混合传入多个文件和目录。不传路径时使用当前目录（`.`）。

终端中默认打开 TUI：上方是可滚动的 Mod 树，下方是紧凑的 Details 卡片。每个 Mod 占两行，File 移到第二行并与 Mod ID 左对齐，第一行不再单列 File；选中项的两行以背景色标识；`>` 表示可展开分支，`v` 表示已展开，叶子不显示箭头。→ 展开一级，← 收起当前分支或移至父项，Space 切换当前分支，`e` 展开全部。`--expanded` 会在启动时展开全部分支。

仅 TUI 会自动按列表顺序识别外层 JAR：每个文件查询 Modrinth，并通过 Curse.tools 查询 CurseForge。默认最多同时执行 10 个平台反查请求，期间仍可操作列表；`--identify-parallelism <COUNT>` 指定正整数并行度，设为 `1` 则顺序识别。Index 下方的第一位对应 Modrinth，第二位对应 CurseForge；识别时显示加载动画，验证匹配后显示平台色勾选。Details 显示各平台状态，成功时包括 Project ID、File ID、文件名、匹配类型和大小，支持选择和复制。单次请求失败后继续处理其他文件，退出 TUI 会取消剩余请求。重复文件路径共享识别结果。嵌套 JAR 不参与识别，也不会继承外层文件的成功标记。Console 和 Raw 不执行远程识别。

在 Details 中选中已识别成功的 Modrinth 或 CurseForge 字段，按 Enter 进入该项目的 files，自动将识别到的版本置顶为 `Selected`。命令行的 `--loader/--loaders` 筛选会一并传递，多选按任意匹配；未指定时不限制 loader。Esc 或 q 返回原列表，保留选中行、Details 字段和筛选条件。加载时可按 Esc 取消；请求失败会返回列表并显示错误。尚未匹配的来源和嵌套 JAR 不提供此入口。

平台颜色从 `~/.mcpm/source_colors.toml` 读取，缺失项使用 `mcpm/config/source_colors.toml` 默认值。`source_colors.tui` 和 `source_colors.console` 表包含 `modrinth`、`curseforge` 的颜色；这些颜色不属于 `mcpm customize` 编辑项。

↑/↓、PageUp/PageDown、Home/End 和触控板用于导航。`F` 在 Mods 和 Details 之间打开 Filter 卡片。普通文本默认搜索 Name、Mod ID、File、Loaders，包括隐藏子项；Index 不参与筛选。输入 `@name Fabric API @file fabric`，表示名称包含 `Fabric API` 且文件名包含 `fabric`。支持 `@name`、`@modid`、`@file`、`@loaders`，各字段的内容持续到下一个 `@`，多个条件必须同时满足，匹配不区分大小写。字段标记使用主色，后续带特殊含义的内容使用附色。用 `\@` 表示字面量 `@`，例如 `@file user\@host.jar`，其他反斜杠保持原样；不支持 `@ name`，未知标记（包括 `@index`）会提示警告且不匹配任何行。

搜索只自动展开通往匹配项的祖先路径；仅父项自身匹配时，不会连带自动展开其子项。普通列表原有的展开、折叠状态始终保留，分支不再处于匹配路径时立即恢复，无需清空筛选。搜索中仍可用 ←/→、Space、鼠标箭头逐级折叠/展开，`e` 展开全部；这些手动操作优先于自动展开，在分支退出搜索结果后失效。清空筛选后恢复普通列表状态。

匹配内容使用附色，并反转原有粗体：普通文字变粗体，已有粗体变普通字形。如果命中会被截断隐藏，单元格会保留开头，并用省略号连接第一个命中附近的上下文；优先完整显示命中，只有命中本身超过可用宽度时才截断。表格、File 第二行和 Details 使用相同规则，复制仍保留完整原文。

筛选实时生效。Enter 应用并返回 Mods 焦点；Esc 或大写 `Q` 关闭筛选卡片并保留条件，小写 `q` 作为普通筛选文本输入。在筛选框外按 `r` 清空条件。Tab/Shift-Tab 在可见的 Mods、Filter、Details 卡片间切换；Details 内用 ↑/↓ 选择字段，`c` 复制完整字段值，即使显示内容已被 `...` 截断。状态栏反馈复制成功或失败。除已匹配的来源字段外，Enter 打开/关闭完整 Details，`i` 查看跳过的文件；筛选框外按 `q`、Esc 或 Ctrl-C 退出。鼠标点击可选择行和字段，点击箭头可切换分支。

`--no-ui` 使用 Console 列表；`--raw` 或 stdout 重定向使用 TSV 原始输出，`--no-ui --raw` 同样使用 Raw。TUI 和 Console 分别使用与当前终端深浅模式对应的配色，NO_COLOR 关闭颜色。

路径按参数顺序处理。目录只提供其直接包含的 `.jar` 和 `.jar.disabled` 文件，并按路径排序；跳过子目录和其他文件，不进行递归扫描。直接传入的文件也必须使用上述后缀，后缀区分大小写。支持跟随指向普通文件和目录的符号链接。

重复路径会重复处理。例如，一个文件同时作为独立参数传入并通过目录扫描发现时，会出现两次。文件编号按合并后的显示顺序分配。

```sh
mcpm mod list first.jar mods/ second.jar other-mods/
mcpm mod list --expanded mods/ --loaders forge,neoforge
mcpm mod list --no-ui --width 100 mods/
mcpm mod list -- -leading-dash.jar
```

| 选项 | 说明 |
| --- | --- |
| `--no-ui` | 使用 Console 列表，不打开 TUI。 |
| `--identify-parallelism <COUNT>` | TUI 平台反查并行度，默认 10，必须为正整数。Console 和 Raw 忽略此项。 |
| `--expanded` | 显示全部 Mod 声明，包括已识别的嵌套 Mod。在终端输出中还列出因 Loader 筛选而跳过的文件名。 |
| `--index-type <TYPE>` | `flat`（默认）显示完整且左对齐的编号；`tree` 对齐文件编号后的第一项，其余层级紧凑缩进，并缩进嵌套 Mod 名称。 |
| `--width <WIDTH>` | 覆盖 Console 排版宽度，范围为 1–65535 列。TUI、原始输出或重定向时忽略此选项。 |
| `--path-style <STYLE>` | 使用 `inner`、`full`（默认）或 `outer` 格式化嵌套归档路径，详见[归档路径样式](README.md#归档路径样式)。 |
| `-C <LETTERS>` | 使用大写字母选择列，可重复指定或与长选项组合。 |
| `--columns <COLUMNS>` | 使用逗号分隔的列名选择多列。 |
| `--column <COLUMN>` | 选择一列，可重复指定。 |

## 列

| 字母 | 名称 | 内容 |
| --- | --- | --- |
| `I` | `index` | 根据文件、Loader、原生 Mod 和嵌套 JAR 位置生成的层级编号。 |
| `N` | `name` | Mod 显示名称；无法识别时为去除扩展名的文件名。 |
| `M` | `modid` | Mod 标识符。 |
| `F` | `filename` | 外层文件名；嵌套归档默认显示完整的 `outer-jar-path!inner-jar-path` 链。 |
| `L` | `loaders` | 识别成功的 Loader 及识别失败标记。 |

默认显示全部列。显式选择取并集，并始终按 `I`、`N`、`M`、`F`、`L` 的固定顺序显示，与参数选择顺序无关。

```sh
mcpm mod list mods/ -C NML
mcpm mod list mods/ --columns name,filename --column loaders
mcpm --raw mod list mods/ -C MF
```

默认视图使用第一个已识别声明作为文件摘要，并附带其他声明数量和 Loader 名称。`--expanded` 分别显示每个声明，并为失败的 Loader 尝试添加行。

## 声明编号

所有数字位置均从 **1** 开始，编号在当前显示的文件序列中标识声明：

1. 按显示顺序分配文件编号 `X`。
2. 成功的 Loader 元数据记录与失败的 Loader 尝试一起计数。总数为一时，成功元数据的前缀为 `X`；大于一时附加 Loader ID，例如 `X-forge` 或 `X-neoforge`，包括仅一个成功、另一个失败的情况。未识别文件且没有报错的 Loader 不计入。
3. 对于元数据前缀 `Y`，只有一个直接声明的 Mod 时使用 `Y`；多个直接声明的 Mod 分别使用 `Y-1`、`Y-2` 等。
4. 子 JAR 元数据使用 `Y+1`、`Y+2` 等，与原生 Mod 编号独立。对子 JAR 递归应用相同规则。

例如，`1-neoforge+2-3` 表示文件 1 的 NeoForge 记录中，第二个子 JAR 直接声明的第三个 Mod。`1-fabric+1+1` 表示孙级 JAR 中唯一的 Mod。文件 144 只有一个 Loader 记录且其中只有一个 Mod 时，编号就是 `144`。

子 JAR 位置按声明顺序编号，包括保留在结构中但不产生 Mod 输出行的纯库 JAR。`--path-style` 不影响编号。

摘要视图保留第一个显示声明的编号。展开视图中每个 Mod 行都有独立编号；失败的 Loader 行使用 `X-loader`。

`--index-type flat` 保留完整编号并左对齐。两种编号格式中的 Loader 名称均使用与 Loaders 列相同的颜色，周围的数字编号不着色。

`--index-type tree` 用 `[X]` 表示文件，多个 Loader 记录时用 `[loader]`，一个 JAR 直接声明多个 Mod 时用 `[N]`，子 JAR 位置用 `<N>`。文件编号后的第一项始终从同一列开始，无论它是 Loader 标签、原生 Mod 编号还是嵌套 JAR 编号。为此会补齐文件标签宽度，其他标签不补齐到相同宽度。更深的重复祖先位置每层仅占两格，不保留完整标签的宽度。每个文件开始新的层级。单个 Loader 或单个直接声明的 Mod 无需额外标签，省略规则与完整编号一致。

重复祖先的位置用灰色 `·` 标记，保持原有对齐和紧凑间距。原始输出和无颜色输出仍保留这些引导字符。

例如，Forge 和 NeoForge 记录可以显示为：

```text
[1][forge][1]
·  · [2]
·  · <1>[1]
·  · · [2]
·  [neoforge]
```

只有一个 Loader 且直接声明多个 Mod 时，同样适用：

```text
[144][1]
·    [2]
·    <1>[1]
·    · [2]
```

使用 `--expanded` 显示完整层级，仅更改编号格式不会展开摘要。树形模式下，外层声明的名称从 Name 列正常位置开始，每嵌套一层 JAR，名称前增加两格。换行后的续行保持此缩进。Loader 和原生 Mod 编号不增加名称缩进。

两种编号格式都支持列选择、路径样式和原始输出。原始输出保留编号字段内的首项对齐和紧凑层级，以及 Name 字段内的嵌套缩进，制表符分隔的列之间不添加终端排版的填充。

```sh
mcpm mod list mods/ --expanded --index-type flat
mcpm mod list mods/ --expanded --index-type tree
```

未指定 Loader 筛选时，所有 Loader 都无法识别的归档显示为 `[unrecognized]`。指定筛选时，没有已识别声明且没有识别错误的文件会被跳过。识别失败会报告错误，不视为被筛选掉的文件。

输入路径无效或不可读时报告错误，并继续处理其他路径。目录扫描失败时，不使用该目录的任何条目。只要发生输入路径错误或 Loader 识别错误，即使已经显示其他结果，命令仍返回失败状态。

通用选项见 [Loader 选择](README.md#loader-选择)，忽略文件提示及 `--raw` 行为见[终端输出](../README.md#终端输出)。

[命令参考](../README.md) · [mod 概览](README.md) · [English](../../docs/mod/list.md)
