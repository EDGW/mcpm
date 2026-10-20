# 调试调色板

```sh
mcpm debug palette
mcpm debug palette --no-ui
mcpm debug palette --raw
```

显示标准 ANSI 的 8 种普通色、8 种亮色和默认前景色，每种颜色展示 Normal、Bold、Dim
三种样式。使用终端自己的调色板和默认背景，可用于比较浅色、深色终端主题的效果。

- **TUI**（默认）：示例文字为对应的 `crossterm` 名称，例如 `Color::DarkGreen`、
  `Color::Green`，列标题标明 `Attribute::Bold` 和 `Attribute::Dim`。
  窄终端改为逐行显示。使用方向键、j/k、PageUp/PageDown、Home/End 滚动，
  q、Esc 或 Ctrl+C 退出。
- **Console**（`--no-ui` 或 `-N`）：示例文字为对应的 `owo-colors` 表达式，
  例如 `.green()`、`.green().bold()`、`.green().dimmed()`。
- **Raw**（`--raw` 或 stdout 重定向）：成功退出，不输出任何内容。

stdin 无法交互或 stderr 不是终端时，若 stdout 仍是终端则回退到 Console。
`NO_COLOR` 禁用颜色。默认前景色在 TUI 中为 `Color::Reset`，在 Console 中为
`.default_color()`，不固定为白色。
