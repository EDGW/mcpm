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
语法错误会在搜索框标出列号，并将光标移到对应位置，保留原结果。
取消请求或请求失败也会保留原结果及筛选条件。进入 Files 时保留版本和 Loader 的候选值，
文件列表先筛选再分页。
