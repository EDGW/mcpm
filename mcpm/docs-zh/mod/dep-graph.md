# `mcpm-tools mod dep-graph`

```text
mcpm-tools mod dep-graph build [OPTIONS] <DIR>
```

扫描指定目录中的 `.jar` 和 `.jar.disabled` 文件，并递归识别各 Loader 声明的内嵌 JAR。不会递归扫描文件系统子目录。未被所选 Loader 识别的有效归档被跳过。所有 Loader 识别失败（包括归档读取或元数据解析失败）统一使用 Skip：继续尝试其他 Loader 和后续文件，保留成功记录并导出图。目录扫描和输出错误仍会导致命令失败。

```sh
mcpm-tools mod dep-graph build mods/
mcpm-tools mod dep-graph build mods/ -o graph.json
mcpm-tools mod dep-graph build mods/ --loader fabric --external fabric:minecraft --external fabric:java --output graph.json
```

默认向 stdout 输出格式化 JSON，不包含颜色或提示文字。使用 `-o <FILE>` 或 `--output <FILE>` 写入文件，已有文件会被覆盖。错误写入 stderr。支持 `--loader` 和 `--loaders`，选择方式见 [Loader 选择](README.md#loader-选择)。

`--external <LOADER:MODID>` 添加一个已知的外部组件，可重复指定多个，冒号两侧均不能为空。相同标识重复传入只添加一个 External 候选。目标节点已经存在时，复用该节点并保留其入边和其他候选；不存在时创建节点。External 没有出边或版本信息。显式 External 独立于 `--loader`／`--loaders` 的识别筛选，也可以使用自定义 Loader 命名空间。不指定参数时不添加 External。

JSON 顶层包含 `format_version: 1`、`identification_failures` 和按 Loader、Mod ID 排序的 `nodes` 数组。识别失败列表记录文件路径、Loader 和错误原因链；非空表示识别不完整。失败也会报告到 stderr，但不使构建返回失败状态。每个节点包含 `identifier`（`loader` 和 `id`）及 `candidates`。候选 `kind` 为 `primary_mod`、`nested_mod` 或 `external`；仅通过显式 `--external LOADER:MODID` 参数添加 external。未找到候选的目标保留为空节点，不推断 Minecraft、Java 或 Loader 的版本。

Mod 候选包含 `modinfo`（名称、Mod ID 和版本）及 `outgoing`。每条边直接保存 `target` 和完整的 `dependency`：`kind`、`id`、`version_requirement`、`side`、`ordering` 和 `reason`。版本要求仍保留同一目标版本的 `all` 和 `any_of` 条件。边和依赖均无序，不记录声明序号或条件路径。显式 `ordering` 表示 Loader 的加载顺序约束，与 JSON 数组位置无关。例如：

```json
{
  "target": { "loader": "forge", "id": "library_b" },
  "dependency": {
    "kind": "required",
    "id": "library_b",
    "version_requirement": { "type": "any" },
    "side": "both",
    "ordering": "after_target",
    "reason": null
  }
}
```

此格式尚未发布，因此继续使用格式版本 1。

反向索引和运行时 Loader 实例不写入文件。输出表示声明关系，不判断版本是否满足、不选择实际加载候选，也不将依赖环认定为加载顺序错误。已禁用文件仍会产生候选，当前 JSON 不记录文件启用状态或来源路径。读取和校验图文件使用下述 `test` 子命令。

## 校验并恢复快照

```sh
mcpm-tools mod dep-graph test graph.json
```

`test <FILE>` 使用内置 Loader 注册表调用库中的 JSON 读取函数，恢复全部候选、完整依赖、`ModInfo.dependencies` 及反向关系，成功后仅向 stdout 输出节点数、各类候选数、边数、空节点数及识别失败数，不输出图的 JSON。它不会读取 Mod 归档或判断版本兼容性。空节点和 External 候选无需 Loader 实例；Mod 候选必须在注册表中找到对应 Loader。数组位置不表示依赖或加载顺序。

库接口为 `storage::read_json(input, &loaders)`，接收实现 `std::io::Read` 的输入和 `HashMap<LoaderId, ModLoaderMeta>`，返回 `Result<DependencyGraph, storage::error::ReadGraphError>`。抽象层不引用内置 Loader 实现。

读取错误区分 I/O、JSON 语法或编码、结构字段、不支持的格式版本及图关系校验。图关系错误包括重复节点、未知 Mod Loader、注册表 ID 不一致、候选与节点 ID 不一致、跨 Loader 的边、依赖 ID 与边目标不一致，以及目标节点缺失。未知 JSON 字段和不合法的候选、依赖类型及版本条件会被拒绝。循环、重复候选和重复依赖仍然允许。导入失败时不输出统计结果，CLI 统一通过现有诊断模块展示完整错误原因链。

## 查看一个 Mod 的依赖

```sh
mcpm-tools mod dep-graph view -i graph.json fabric:examplemod
mcpm-tools mod dep-graph build mods/ | mcpm-tools mod dep-graph view fabric:examplemod
mcpm-tools --raw mod dep-graph view -i graph.json fabric:examplemod
```

`view [-i FILE] <LOADER:MODID>` 从指定文件读取 Graph JSON；省略 `-i` 或指定 `-i -` 时从 stdin 读取。选择器按第一个冒号拆分。不认识的节点和无效输入统一通过现有错误原因链诊断输出。

全部依赖节点从下一行开始，以竖向树形分叉展示，每层使用固定宽度缩进。节点显示 Mod ID 和候选版本，边保留关系类别、版本要求、side、ordering 和 reason。关系颜色沿用 `deps`，节点 ID 使用 Loader 颜色。重定向输出、`--raw` 和 `NO_COLOR` 遵循通用终端策略。分支缩进不依赖节点标签长度，非 ASCII 元数据不会影响树形结构对齐。

正向模式下，每个节点在最浅依赖层级展开，较深位置标注 `[shared]` 并折叠，即使较浅位置还未打印。相同层级的重复节点只展开一次。指向当前祖先的引用标注 `[cycle]`，仅表示遍历遇到循环，不判定为加载顺序错误。多个候选分别展示 primary、nested 或 external 分支及其各自声明，不静默选择或合并。`[no candidate]` 表示图中该节点没有已知候选，`[external]` 表示显式提供的外部组件；这些标记不判断版本兼容性、端限制是否适用或实际加载选择。快照包含识别失败时，会显示识别不完整的摘要。

### 忽略目标和筛选关系类型

```sh
mcpm-tools mod dep-graph view -i graph.json fabric:examplemod --ignores java,fabric:helper
mcpm-tools mod dep-graph view -i graph.json fabric:examplemod --ignore java --ignore :helper
mcpm-tools mod dep-graph view -i graph.json fabric:examplemod --no-default-ignores
mcpm-tools mod dep-graph view -i graph.json fabric:examplemod --type required --types optional,recommended
```

| 参数 | 行为 |
| --- | --- |
| `--ignore <MOD>` | 追加一个忽略目标，可重复指定。 |
| `--ignores <MODS>` | 追加逗号分隔的目标，可重复或与 `--ignore` 组合。 |
| `--no-default-ignores` | 关闭默认的 `minecraft` 忽略项，显式忽略项仍然生效。 |
| `--type <TYPE>` | 选择一个关系类型，可重复指定。 |
| `--types <TYPES>` | 选择逗号分隔的类型，可重复或与 `--type` 组合。 |

忽略选择器区分大小写：`loader:name` 精确匹配 Loader 和 Mod ID；`:name` 或 `name` 匹配所有 Loader 下的该 ID。显式忽略项在默认项基础上追加。忽略作用于树中展示的相邻节点：正向为依赖目标，反向为声明关系的来源 Mod。显式查询的根节点始终保留。

未指定类型参数时，只展示 `required`、`breaks`、`conflicts`。提供任意 `--type` 或 `--types` 后，使用全部显式选择的并集替换默认类型。可选值为 `required`、`optional`、`recommended`、`suggested`、`breaks`、`conflicts`，也接受单数别名 `break` 和 `conflict`。显示全部关系并关闭默认忽略项：

```sh
mcpm-tools mod dep-graph view -i graph.json fabric:examplemod --no-default-ignores --types required,optional,recommended,suggested,breaks,conflicts
```

过滤对全部依赖层级生效，不再沿隐藏的边继续遍历；同一节点仍可能通过其他可见路径出现。正向模式的最浅展开层级及 `[shared]` 标记依据过滤后的图计算，隐藏的关系不会影响折叠位置；反向模式逐路径展开，不折叠共享节点。过滤不修改存储的 Graph 或识别失败记录。

### 被依赖树

```sh
mcpm-tools mod dep-graph view -i graph.json fabric:library_b --reverse
mcpm-tools mod dep-graph view -i graph.json fabric:library_b -R --types required,optional
```

`--reverse`／`-R` 沿入边展示哪些 Mod 直接或间接引用当前节点。标注 `(required by)` 的子节点声明了对父节点的必需依赖；其他关系保留原始类别，包括冲突和不兼容关系。`parent:` 后保留原始的版本要求和限定字段，针对父节点，而不是对子节点自身版本的要求。

每个反向子节点显示实际来源候选的版本及 primary／nested 身份，不会替换成同一节点下其他候选。之后继续沿来源节点的入边查找，不会沿来源候选的出边展开。由于声明指向 Node，此视图不决定多候选目标中哪一个实际满足要求。没有候选的目标节点和 External 也可以展示被依赖关系。

类型过滤依据原始声明类别，忽略规则匹配展示出的来源 Mod。默认忽略不会隐藏显式查询的根节点，例如 `fabric:minecraft`。反向不折叠共享节点，每条可见路径均完整展开；仅在引用当前路径的祖先时停止该环分支并标注 `[cycle]`，防止无限输出。颜色和识别不完整摘要在两个方向均生效。

[mod 概览](README.md) · [English](../../docs/mod/dep-graph.md)
