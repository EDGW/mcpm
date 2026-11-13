# `debug all-help`

递归列出当前可执行文件的全部命令帮助，适合集中查阅或导出命令用法。

```text
mcpm debug all-help [OPTIONS]
mcpm-tools debug all-help [OPTIONS]
```

## 选项

| 选项 | 说明 |
| --- | --- |
| `--raw` | 以纯文本输出，适合保存到文件。 |

## 示例

```sh
mcpm debug all-help
mcpm-tools debug all-help --raw > all-help.txt
mcpm-server debug all-help --raw > server-help.txt
```

## 输出范围

- 包含根命令、所有父命令组和叶子命令，先输出父命令，再递归输出子命令。
- 每段标注完整命令路径，包含参数、选项、默认值和补充帮助。
- `mcpm`、`mcpm-server` 导出服务端命令树；`mcpm-tools` 导出完整 Tools 命令树，包括服务端、客户端、Mod、缓存和调试命令。
- 命令别名不重复输出。

输出到 stdout。Console 和 Raw 均可使用，通用选项见 [README](../README.md)。
