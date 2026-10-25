# Filter 类型

`mcpm-tools mods list` 和 `mcpm mods list` 均支持 `-F/--filter <FILTER>`，适用于 TUI、Console、Raw。TUI Filter 卡片可编辑同一表达式。

| 语法 | 含义 |
| --- | --- |
| `Fabric`、`'Fabric API'`、`"Fabric API"` | 不区分大小写的包含匹配。 |
| `=fabric`、`='Fabric API'` | 不区分大小写的完整值精确匹配。 |
| `~'^Fabric.*API$'` | Rust 正则，默认不区分大小写。 |
| `@name`、`@modid`、`@file`、`@loaders` | 限定紧随其后的一个值或括号组。 |
| `A B`、`A & B` | AND，优先级高于 OR。 |
| `A \| B` | OR。 |
| `(A \| B) & C` | 括号分组。 |

未限定字段时搜索 Name、Mod ID、File、Loaders，包括隐藏子项；Index 不参与搜索。精确匹配比较完整字段；Loaders 则逐个比较不带显示方括号的 Loader 名称。File 使用 `--path-style` 选定的显示归档路径。空 Filter 匹配全部，`=""` 匹配空字段。

括号组继承字段作用域，组内显式字段覆盖继承字段：

```text
@loaders (Forge | Fabric)
@loaders (=Forge | =Fabric)
@name (Fabric | @modid =sodium)
```

第一句也会匹配 NeoForge，因为 Forge 是其子串；第二句只匹配两个完整 Loader 名称。作用域在组结束后终止；`@loaders Forge | Fabric` 的 Fabric 不限定字段。短语需要引号，字段不会持续作用于后面的空格条件。

引号外，反斜杠可转义 `@`、`(`、`)`、`&`、`|`、`=`、`~`、两种引号、反斜杠和空白。引号内只转义对应引号和反斜杠，其他反斜杠原样保留：

```text
@file user\@host.jar
@file a\(b\)\&c\|d.jar
@name "He said \"hello\""
@name 'Mod (client | server)'
@modid ~'^\d+$'
```

正则默认搜索子串，`^`、`$` 限定整个值，`(?-i)` 切换为区分大小写。不支持环视和反向引用。零宽匹配会选择行，但不高亮文字。未知字段、缺少值、非法正则、未闭合引号或括号都会提示字符列号；TUI 标出错误，在表达式恢复合法前不匹配任何行。

命令行中需要用 shell 引号包住完整表达式：

```sh
mcpm-tools mods list mods/ -F "@modid 'Fabric'"
mcpm mods list --all -F "@loaders (=Forge | =Fabric)"
mcpm --raw mods list --file example.jar -F "@name ~'API$'" --expanded
```

服务器的 `--file`、`--modid`、`--name`、`--all` 先解析输入文件，Filter 只在这些输入中筛选；两者独立，`--all` 可以与 `-F` 同用。Console 和 Raw 搜索全部声明并输出匹配行，未指定 `--expanded` 时每个输入文件只输出首个匹配。TUI 保留祖先行作为上下文，清空或修改初始 Filter 不会丢失输入选择。
