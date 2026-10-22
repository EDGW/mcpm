# 服务端 Mod 来源

搜索远端项目、浏览发布文件，或根据本地文件内容反查来源：

```sh
mcpm server mods source search create --server ./my-server --source modrinth
mcpm server mods source files create --server ./my-server --source curseforge
mcpm server mods source identify --server ./my-server --source modrinth --all
```

三个命令均支持 `--server PATH`，默认当前目录，要求服务端已注册且声明了受支持的
mod loader。`--source` 支持 `modrinth`、`curseforge`（Curse.tools）和
`curseforge@KEY`（官方 API）。

## Search 和 files

默认使用服务端元数据中的 Minecraft 版本和 loader 过滤。
显式的 `--game-version`、`--loader` 分别覆盖对应默认值。
`--all-versions` 同时取消这两个过滤，不能与显式过滤同时使用。
这些条件只筛选声明的兼容性，不检查 Mod 依赖。

Search 接受可选的[搜索表达式](../mod/source-search.md)：关键词放在前面，
后接 `@version VALUE`、`@loader VALUE`；括号内用 `|` 表示候选值，`*` 清除对应限制。
表达式字段覆盖显式命令行选项及服务端默认条件。Shell 中应引用整条表达式。
Files 接受远端 project ID 或精确
slug，也会追加读取 stdin，每行一个项目。这些参数不是本地 modid。

```sh
mcpm server mods source search "Steam 'n' Rails" --source modrinth
mcpm server mods source files 688231 --source curseforge --all-versions
mcpm server mods source files create --source modrinth --game-version 1.21.1
printf '%s\n' create ZzjhlDgM | mcpm server mods source files --source modrinth
```

终端中 search 和单项目 files 默认使用 TUI。搜索界面顶部始终显示搜索框，按 `/`
进入编辑，修改后按 Enter 才重新搜索，并回到第一页；Esc 放弃修改。Tab / Shift+Tab
切换搜索框、结果列表和底部 Details。结果列表复用 mod list 的布局，第一行显示
Project Name（Primary Color）和加载器，第二行使用整行宽度显示来源报告的 Minecraft 版本（dimmed，逗号为灰色）。
版本最多显示三个[基于完整目录的缩写项](../mod/source-search.md)，后接 `(all N versions)` 表示实际版本总数，
Details 中使用排序后的完整版本名并自动换行。
Details 随选中项目显示加载器、Minecraft 版本、分类、简介、来源、ID、slug 和项目地址；
加载器使用各自的配置色。兼容性信息来自来源响应，CurseForge 可能仅报告近期文件，空值表示未提供。
名称和 slug 沿用主色和附色，来源使用对应
CurseForge / Modrinth 色，ID 使用 dimmed。Details 复用共享字段列表：↑↓ 或 j/k
选择字段，PageUp/PageDown 移动一屏，Home/End 跳至首尾字段。长内容使用省略号，
c 复制字段完整值；Enter 展开/收起详情，Esc 先收起完整详情再退出。支持鼠标点击与滚轮导航。
选中结果时按 Enter 进入项目的 files，保留兼容性过滤；Esc 返回搜索。
多项目、`-N/--no-ui`、指定列或流重定向时使用列表。
排序、翻页和发布类型过滤沿用 `mcpm mod source`。

Search 同样支持[字段值补全](../mod/source-search.md)：Loader 使用 Forge、NeoForge、Fabric，
Version 从当前来源 API 获取并在会话内缓存。↑↓ 选择、Enter/Tab 填入，Esc 关闭候选框。
补全只修改草稿，再次 Enter 才提交搜索。

Files 复用 mod list 的紧凑列表和底部 Details 布局，不提供交互式 Filter。
表头和每个文件均占两行：第一行是序号、Name、Minecraft、Loaders，第二行是 Version、Release；
第二行的值分别与表头中的 Version、Release 对齐，数据行不重复字段标签；
filename 仅在 Details 中查看。Tab / Shift+Tab 切换列表和 Details；↑↓ 或 j/k 选择
文件或详情字段，PageUp/PageDown 在当前页移动一屏，←→ 或 p/n 请求前后远程页。
Home/End 跳至首尾文件或字段；Enter 展开/收起 Details，c 复制当前详情字段。
Esc 先收起完整详情，再返回上一级；q 直接返回。支持鼠标点击选择和滚轮导航。

单项目可用 `--selected FILE_ID` 按 ID 直接获取文件，并在顶部的 `Selected` 区域显示，
不受当前页和兼容性过滤限制。`--loader`（别名 `--loaders`）支持逗号分隔和重复指定，匹配任意一个 loader。翻页时保持显示。TUI 列表保留同一发布文件的原始位置，其 Name 与置顶文件一样使用 primary，
其他文件的 Name 使用 secondary；未传入 selected 时 Name 仍使用 primary。Console 和 Raw 输出继续去重。
分页和总数仍对应原始筛选结果，置顶文件不占用每页数量；文件不存在或不属于指定项目时
报错。独立的 `mcpm mod source files` 同样支持此选项。

```sh
mcpm mod source files sodium --source modrinth --selected yaoBL9D9
mcpm --raw mod source files sodium --source modrinth --selected yaoBL9D9 --columns selected,id,filename
```

Raw 输出先列出选中文件，默认 TSV 列不变。可用 `--columns selected`（或 `-C S`）
添加 `true`/`false` 选中标记；选中文件不参与分页，因此其 `index` 为空。

## Identify

沿用现有服务端 Mod 选择规则：依次匹配文件名、精确 modid、精确显示名称。
`--file`、`--modid`、`--name` 可以明确指定解释方式。
路径相对于服务端 mods 目录，也接受 `mods/` 前缀；不接受绝对路径或向上跳转。
识别任意本地路径请使用独立的 `mcpm mod source identify`。

```sh
mcpm server mods source identify create.jar --source curseforge
mcpm server mods source identify --modid create --source modrinth
printf '%s\n' create | mcpm server mods source identify --source modrinth --input modid
```

`--all` 选择 mods 目录内全部 `.jar` 和 `.jar.disabled` 文件。
无选择条件时不发起请求，在终端 stderr 显示提示。
stdin 每行追加一个选择条件，`--input` 控制其解释方式；同一文件重复选中只识别一次。

识别会计算完整本地文件的哈希，不按 Minecraft 版本或 loader 过滤。
文件选择和 `--all` 无需成功解析 Mod 元数据；名称和 modid 选择需要解析元数据以定位文件，
嵌套声明会选择其所在的最外层归档。

未匹配文件输出 `unmatched`。选择、读取或请求失败会写入 stderr，继续处理其他输入，
最终以非零状态退出。

`--parallelism <COUNT>` 默认 10，与 mod list 共用默认值。并发检查时，结果和逐项错误仍按选择顺序输出，stderr 进度条沿用 mod list 样式。SSH 参数支持独立的 `mod source identify`、`mod list`、`mod info` 和 `mod deps`。

## 输出

三个列表均支持 `-C` 列字母和 `--columns` 列名，具体列见各命令 `--help`。
Raw 或 stdout 重定向时输出转义后的 TSV，不含表头；`--raw` 同时关闭进度提示。

```sh
mcpm server mods source search create --source modrinth -N --columns name,id,url
mcpm server mods source files create --source modrinth --columns id,filename,size
mcpm --raw server mods source identify --source modrinth --all --columns path,project,id,status
```
