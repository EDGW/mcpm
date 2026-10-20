# 自定义配色

`mcpm customize` 默认打开 TUI，支持 Console 和 Raw 命令设置。配置目录为 `~/.mcpm`，缺失项使用构建时嵌入的默认值。

顶部直接列出四个设置项：TUI Dark、TUI Light、Console Dark、Console Light，分别选择各自的配色方案。运行时通过 terminal-colorsaurus 检测终端背景，自动使用对应深浅配色；检测不可用时使用深色。←/→ 即时预览，Enter 应用并保存；n 输入不可变的 ID，复制所选类别的当前方案；Name 初始与 ID 相同，随后可编辑。r 只修改 Name，d 输入 DELETE 确认删除。内置方案不能删除。e 进入 Editing ID 页面，提供 Name、Types 复选框、Primary、Secondary 四项；选中 Name 即可行内编辑，支持 ←/→、Home/End、Backspace 和 Delete；此时快捷键字母作为文本输入，Ctrl-C 请求退出。Space 切换 Types 复选框，任意设置行的 Enter 都保存全部修改。Types 决定下方展示 TUI、Console 或两种预览，颜色使用 TUI 名称并以实际颜色显示，↑/↓ 选择设置。Esc 撤销未保存的修改或取消输入，q / Ctrl-C 请求退出。有未保存修改时，状态栏提示 Y 保存并退出、N 放弃并退出、Esc 继续编辑。状态栏以绿色显示保存文件路径、黄色显示警告、红色显示错误；NO_COLOR 下关闭颜色。

所有设置显示英文名称，命令的 get/set 也接受这些名称。原有属性路径兼容使用。

```sh
mcpm customize
mcpm customize --no-ui list
mcpm customize --theme tui-dark scheme list
mcpm customize --theme tui-dark scheme create ocean --name Ocean
mcpm customize --theme tui-dark scheme use ocean
mcpm customize scheme rename ocean "Ocean Blue"
mcpm customize scheme delete ocean
mcpm customize set "TUI Dark Primary Color" Cyan
mcpm customize --raw get "Console Dark Primary Color"
```

预览区左上、右上分别展示 TUI Dark、TUI Light 的示例卡片；左下、右下分别展示 Console Dark、Console Light 的模拟输出。窄屏改为上下排列，高度不足时展示可见部分并提示拉高终端。预览保持终端背景，不强制填充深浅背景；仅与当前终端模式不匹配的预览提示切换 dark/light 模式；无法检测时保留提示。Console 预览使用 Dim 边框，并与上方留空行。配置中没有 appearance 设置。

启动时扫描 `CONFIG_DIR/styles` 下全部 `.toml` 样式集合，每个文件包含 `[[style]]` 数组。文件损坏或某条样式无效，仅记录 warning 并跳过，其他样式继续加载。已选方案不可用时回退到对应内置方案。`--log-level off` 可关闭 warning。文件顺序固定，普通集合重复 ID 的后续定义跳过，custom.toml 的覆盖项优先。

```toml
[[style]]
version = 1
id = "ocean"
name = "Ocean"
types = ["tui"]
primary = "Cyan"
secondary = "Blue"
```

样式按 ID 覆盖内置默认项，支持局部覆盖。面板新建和编辑的方案统一保存到 `styles/custom.toml`，该文件最后加载并按 ID 覆盖原样式，其他集合文件不被改写。删除导入方案时，其 ID 记录在 `styles/custom.toml` 顶层的 `deleted` 数组中，重启后仍不加载；重新创建同一 ID 会清除此记录。设置保存在 `settings.toml`，加载器颜色保存在 `loader_colors.toml`。各文件原子替换；重新序列化会移除所编辑文件的注释和原排版。

Console 颜色接受 black、red、green、yellow、blue、magenta、cyan、white，对应 bright_ 形式，以及 default_color。TUI 接受 crossterm 的 Black、DarkRed、DarkGreen、DarkYellow、DarkBlue、DarkMagenta、DarkCyan、Grey、DarkGrey、Red、Green、Yellow、Blue、Magenta、Cyan、White、Reset；首字母大写严格使用 TUI 命名，小写严格使用 Console 命名，例如 Red 与 red 表示不同亮度。设置界面统一显示 TUI 名称。`mcpm debug palette` 可预览调色板。

提供子命令时直接执行；`--no-ui` 禁用交互，省略子命令则列出设置。`--raw` 或 stdout 非 TTY 时输出英文名称和值的 TSV。TUI 使用 stderr，stdin 不参与模式检测。`NO_COLOR` 禁用样式，仍可编辑颜色。

ID 仅允许英文字母、数字、下划线和连字符，必须唯一且创建后不可修改；Name 是可修改的显示名称。命令 `scheme create ID --name NAME` 可一次设置两者。
