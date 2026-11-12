# `server.new`

## 输入与结果

`server.new` 接受两类输入，均返回 `StreamingOutput<NewEvent>`：

| 输入 | 结果 |
| --- | --- |
| `NewInput::Prepare { path, version, download_config, rcon_requested }` | 校验目录和版本，返回 `Prepared`。 |
| `NewInput::Install { prepared, accept_eula, no_git, git, rcon }` | 安装已选版本，返回 `Installed { server, git, java_major, rcon }`。 |

`download_config` 使用 `DownloadConfig` codec 编码。`Prepared` 保存确定的下载对象和原下载配置，供安装阶段使用。

## 创建行为

- 父目录必须已存在，目标为新目录或空目录；文件和符号链接目标报错。
- Prepare 获取版本列表，校验版本及 RCON 参数，完成后调用方才提交 EULA 同意结果。
- Install 下载并校验 `server.jar`，写入 `eula.txt`，按需创建 RCON 设置，然后完成 Git 准备、启动配置发布和 Local 登记。
- 下载或初始化失败时保留已经完成的文件，错误说明失败阶段。

## RCON

- 按解析后的 `releaseTime` 判断，不早于 `1.9-pre4` 的版本默认生成 RCON 配置；省略密码时生成 UUID v4。
- 旧版本显式指定端口或密码时拒绝。`rcon_requested` 表示这类显式参数，`--no-rcon` 对应 `rcon: None`。
- 结果中的 RCON 状态为 `Created { port, password }`、`Disabled` 或 `UnsupportedVersion`。创建结果返回实际密码供调用方展示，Debug 格式隐藏密码以防日志泄漏。
