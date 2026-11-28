# mc package manager

[English](README.md)

该项目仍在早期开发中。

[命令手册](crates/mcpm/docs-zh/README.md) · [尚未迁移的开发手册](crates/mcpm/docs-zh/legacy/dev/README.md) · [Backend operation 文档](crates/mcpm-backend/docs-zh/README.md)

命令手册目前可读性较差，会择时完善。

## 安装

安装预发布版本 [v0.1.0-rc2](https://github.com/hitszosa/mcpm/releases/tag/v0.1.0-rc2)。
安装脚本仅支持 **v0.1.0-rc1 及更新版本**，不支持 `v0.1.0-rc1` 之前的版本（旧版本会在下载前被拒绝）。
六个程序及随包产物整套安装：`mcpm`、`mcpm-server`、`mcpmc`、`mcpm-client`、`mcpm-tools`、`mcpmd`。

| 系统 | 架构 | 程序目录 | PATH 入口 |
| --- | --- | --- | --- |
| Windows | x64 | `%LOCALAPPDATA%\Programs\mcpm\bin` | 加入用户 PATH 和当前 PowerShell 会话 |
| macOS | Intel、Apple Silicon | `~/.local/share/mcpm/bin` | 在 `~/.local/bin` 创建链接 |
| Linux | x86_64、ARM64 | `~/.local/share/mcpm/bin` | 在 `~/.local/bin` 创建链接，使用静态链接的 musl 程序 |

默认安装不需要管理员权限。配置、缓存和日志继续保存在 `~/.mcpm`；项目目录中的 `.mcpm` 与程序安装独立。

### Windows（PowerShell 5.1 或更高版本）

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/hitszosa/mcpm/v0.1.0-rc2/scripts/install.ps1))) -Version v0.1.0-rc2
```

卸载：

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/hitszosa/mcpm/v0.1.0-rc2/scripts/install.ps1))) -Uninstall
```

PATH 对当前 PowerShell 和之后新开的终端生效，其他已打开的终端需要重新打开。升级时如提示文件被占用，请先退出正在运行的 mcpm 进程。

### macOS 和 Linux（sh）

需要 `curl`、`unzip`，以及 `sha256sum` 或 `shasum`。

```sh
curl -fsSL https://raw.githubusercontent.com/hitszosa/mcpm/v0.1.0-rc2/scripts/install.sh | sh -s -- v0.1.0-rc2
```

如果 `~/.local/bin` 尚未加入 PATH，在 shell 配置文件中添加下面一行：zsh 使用 `~/.zshrc`，bash 使用 `~/.bashrc`。

```sh
export PATH="$HOME/.local/bin:$PATH"
```

重新加载配置或打开新终端。安装器会在需要时提示，不自动修改 shell 配置文件。

卸载：

```sh
curl -fsSL https://raw.githubusercontent.com/hitszosa/mcpm/v0.1.0-rc2/scripts/install.sh | sh -s -- --uninstall
```

如需通过 GitHub URL 前缀代理下载（`--proxy` / `-Proxy`），见 [GitHub URL 前缀代理参数](scripts/README.md#github-url-prefix-proxy)。

### 升级、修复与卸载行为

- 重新安装即可升级、降级或修复。安装目录已有内容时，须确认替换并删除该目录原有的全部内容；拒绝则不下载、不替换。
- 安装器验证 ZIP 的 SHA-256，执行解包后的 `mcpm-tools debug validate --exact`，并检查程序版本。安装成功后运行 `mcpm-tools debug hello`。
- 文件先暂存并备份，替换失败时尝试回滚。如果回滚未完成，会保留备份并报告位置，请先恢复安装再清理备份。
- 同一时间只允许一个安装器运行。Unix 安装器被中断后，确认没有安装器运行才可删除提示的 `.install-lock` 目录；残留 `.stage.*` 目录可能包含恢复备份。
- 卸载删除登记的程序和附带产物、属于该安装的 Unix 链接，以及安装器添加的 Windows PATH 项；保留其他文件和原先已有的 PATH 项。
- 卸载保留配置、缓存、日志和项目数据。

### 已有安装与 SSH

- 保留旧 Unix 安装 `/usr/local/bin/mcpm`。若它优先于新程序被找到，请检查 PATH，或确认来源后显式删除旧程序。
- 确认替换后，位于 `%LOCALAPPDATA%\Programs\mcpm` 内的旧版程序也会删除。安装目录之外的程序和原先已有的 PATH 项保持不变。
- 非交互 SSH 会话可能不会加载 shell 配置。安装器会将 `mcpmd` 位置记录到 Unix 的 `~/.mcpm/mcpmd.path` 或 Windows 的 `%LOCALAPPDATA%\mcpm\mcpmd.path`。未指定 `--ssh-mcpmd` 时，SSH 读取远端主机的记录；文件不存在仅发出 warning 并尝试远端 PATH，fallback 也失败才报错。详见[远端程序查找](crates/mcpmd/docs-zh/README.md#远端程序查找)。未迁移的 mods/cache 命令仍要求远端安装 `mcpm-tools`。

### 手动安装

[发布附件](https://github.com/hitszosa/mcpm/releases)的 ZIP 安装包包含 `bin/` 和 `manifest.json`：

- macOS/Linux：`mcpm-PLATFORM-ARCH[-LIBC].zip`。
- Windows：`mcpm-windows-x86_64.zip`。
- 每个安装包附带对应的 `.sha256` 文件。Linux 提供 glibc 和 musl 两类产物，安装脚本选择 musl。

解压后将 `bin` 加入 PATH，并运行 `mcpm-tools debug validate` 检查解包文件。
