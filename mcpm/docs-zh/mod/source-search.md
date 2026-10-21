# 远端 Mod 搜索表达式

`mcpm mod source search` 和 `mcpm server mods source search` 共用以下语法，适用于
CurseForge 和 Modrinth，也适用于 TUI、Console、Raw。关键词放在前面，筛选字段放在后面。

```sh
mcpm mod source search 'create @version 1.20.1 @loader fabric' --source modrinth
mcpm mod source search 'create @version (1.20.1 | 1.21.1) @loader (forge | neoforge)' --source curseforge
mcpm server mods source search '@version * @loader *' --source modrinth
```

| 写法 | 含义 |
| --- | --- |
| `create railways` | 整段关键词交给平台搜索，不解释为布尔 AND。 |
| `@version 1.20.1` | 按完整 Minecraft 版本匹配。 |
| `@loader fabric` | 按完整 Loader 名称匹配，不区分大小写。 |
| `@version (1.20.1 \| 1.21.1)` | 匹配任意一个列出的版本。 |
| `@loader (forge \| neoforge)` | 匹配任意一个列出的 Loader。 |
| `@version *` / `@loader *` | 清除对应限制，包括命令行和服务端默认条件。 |

不同字段之间 AND，同一括号内 OR；重复的候选值会去重。`forge` 不会匹配 `neoforge`。
每个字段只能声明一次；需要多个值时使用括号。关键词可省略，空表达式使用默认条件浏览。

优先级：表达式字段 > `--game-version` / `--loader` > 服务端默认值。
删除某字段会恢复对应默认值；要清除默认限制，显式填写 `*`。
`--all-versions` 清除服务端默认条件，仍可用表达式指定筛选。

引号用于保护包含 `@`、`(`、`)`、`|`、`&` 的关键词，也可以用反斜杠转义字符。
单引号或双引号都可使用，引号本身不传给平台；反斜杠本身写成 `\\`。
例如在搜索框输入 `"Steam 'n' Rails" @loader forge`，会保留名字里的单引号。
Shell 命令还需要外层引用或转义：

```sh
mcpm mod source search "\"Steam 'n' Rails\" @loader forge" --source curseforge
```

不支持正则、否定、版本范围或任意布尔嵌套。未知字段、重复字段、缺少值、未闭合引号或括号
均报错，并给出从 1 开始的字符列号。两家的关键词匹配和排序由各自平台决定。
CurseForge 每次搜索最多支持 4 个版本、5 个 Loader；来源不支持的 Loader 会明确报错。

TUI 中按 `/` 编辑顶部表达式，回车才搜索并回到第一页；Esc 放弃编辑。
编辑器沿用 mod list 的语法配色：字段名和转义符用主色、值用附色、运算符用重要文本样式、
括号和引号用 dimmed。编辑时即时用红色下划线标出语法错误，不会触发网络请求。
语法错误会在搜索框标出列号，并将光标移到对应位置，保留原结果。
取消请求或请求失败也会保留原结果及筛选条件。进入 Files 时保留版本和 Loader 的候选值，
文件列表先筛选再分页。

光标位于 `@loader` 或 `@version` 的值区域时，搜索框下方会弹出候选列表，也支持
`(A | B)` 中的候选值。Loader 沿用现有 Forge、NeoForge、Fabric 三个选项；
Version 在搜索界面打开时从当前来源的 API 后台获取，并在本次搜索会话内缓存，加载期间仍可输入。
输入前缀筛选候选，↑↓ 或 PageUp/PageDown 选择，Enter/Tab 或鼠标点击填入。
补全只修改草稿，再次 Enter 才搜索。Esc 先关闭候选框，再按一次才放弃编辑。
版本加载失败时仍可手动输入，Ctrl-R 重试。两个候选框均使用附色边框，版本条目使用普通样式。

Results 和 Details 复用 mod list / Files 的共享浏览组件。Tab 切换搜索框、结果列表
和 Details，支持鼠标点击与滚轮导航。结果第一行显示项目名称和加载器，第二行使用整行宽度
显示来源报告的 Minecraft 版本。Details 包含加载器、版本、分类和 slug；加载器使用对应
配置色；列表和 Details 的 Loaders 仅显示 forge、neoforge、fabric。
支持的版本使用 dimmed，逗号使用灰色。版本系列按前两段数字降序，组内数字补丁升序，
可缩写为 `1.20.1/3/5~8/10/6-snapshot`，非数字后缀放在组末。覆盖目录中的全部 `1.20.*`
时显示为 `1.20.x`，还支持 `1.20` 时显示为 `1.20(.x)`。非 `1.` 前缀覆盖目录中的
全部 `22.*` 时显示为 `22.x`，不限段数。列表最多显示三个缩写项，后接 `(all N versions)`，
N 按去重后的实际版本数计算，而非缩写项数。无法解析的版本名保留原样列在最后；目录未加载时
不推断通配符。Details 的 Minecraft 字段展示排序后的完整版本名，自动换行，复制仍为完整字段。
信息来自来源响应，CurseForge 可能仅报告近期文件；空值表示未提供，
版本与加载器列表不保证任意组合都兼容。在 Details 中，↑↓ 或 j/k 逐行选择（包括 Minecraft 的换行），
PageUp/PageDown 移动一屏，Home/End 跳到首尾字段。长内容使用省略号，c 可复制
当前字段的完整值。Enter 展开/收起 Details，Esc 先收起完整详情再退出。
在结果列表中按 Enter 进入 Files。
