# mc package manager

[English](README.md)

该项目仍在早期开发中。

[命令手册](mcpm/docs-zh/README.md) · [开发手册](mcpm/docs-zh/dev/README.md)

命令手册目前可读性较差，会择时完善。

## 安装

安装预发布版本 [v0.1.0-dev.26w43c](https://github.com/EDGW/mcpm/releases/tag/v0.1.0-dev.26w43c)。
安装脚本仅支持 **v0.1.0-dev.26w43c 及更新版本**，旧版本会在下载前被拒绝。
五个程序整套安装：`mcpm`、`mcpm-server`、`mcpmc`、`mcpm-client`、`mcpm-tools`。

| 系统 | 架构 | 程序目录 | PATH 入口 |
| --- | --- | --- | --- |
| Windows | x64 | `%LOCALAPPDATA%\Programs\mcpm\bin` | 加入用户 PATH 和当前 PowerShell 会话 |
| macOS | Intel、Apple Silicon | `~/.local/share/mcpm/bin` | 在 `~/.local/bin` 创建链接 |
| Linux | x86_64、ARM64 | `~/.local/share/mcpm/bin` | 在 `~/.local/bin` 创建链接，使用静态链接的 musl 程序 |

默认安装不需要管理员权限。配置、缓存和日志继续保存在 `~/.mcpm`；项目目录中的 `.mcpm` 与程序安装独立。

### Windows（PowerShell 5.1 或更高版本）

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-dev.26w43c/scripts/install.ps1))) -Version v0.1.0-dev.26w43c
```

卸载：

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-dev.26w43c/scripts/install.ps1))) -Uninstall
```

PATH 对当前 PowerShell 和之后新开的终端生效，其他已打开的终端需要重新打开。升级时如提示文件被占用，请先退出正在运行的 mcpm 进程。

### macOS 和 Linux（sh）

需要 `curl`、`tar`，以及 `sha256sum` 或 `shasum`。

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-dev.26w43c/scripts/install.sh | sh -s -- v0.1.0-dev.26w43c
```

如果 `~/.local/bin` 尚未加入 PATH，在 shell 配置文件中添加下面一行：zsh 使用 `~/.zshrc`，bash 使用 `~/.bashrc`。

```sh
export PATH="$HOME/.local/bin:$PATH"
```

重新加载配置或打开新终端。安装器会在需要时提示，不自动修改 shell 配置文件。

卸载：

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-dev.26w43c/scripts/install.sh | sh -s -- --uninstall
```

### 升级、修复与卸载行为

- 使用支持的版本标签重新安装，即可整套升级或降级；重复安装同一版本可修复丢失的程序。
- 改动安装前，先验证安装包 SHA-256、清单和每个程序的版本。
- 文件先暂存并备份，替换失败时尝试回滚。如果回滚未完成，会保留备份并报告位置，请先恢复安装再清理备份。
- 同一时间只允许一个安装器运行。Unix 安装器被中断后，确认没有安装器运行才可删除提示的 `.install-lock` 目录；残留 `.stage.*` 目录可能包含恢复备份。
- 安装记录统一标记受管理文件。卸载删除这些程序、属于该安装的 Unix 链接，以及安装器添加的 Windows PATH 项；保留其他文件和原先就有的 PATH 项。
- 卸载保留配置、缓存、日志和项目数据。

### 已有安装与 SSH

- 保留旧 Unix 安装 `/usr/local/bin/mcpm`。若它优先于新程序被找到，请检查 PATH，或确认来源后显式删除旧程序。
- 保留旧 Windows 程序 `%LOCALAPPDATA%\Programs\mcpm\mcpm.exe` 及其 PATH 项。如果旧程序仍优先被找到，安装器会提示；验证新程序后再显式移除旧程序和旧 PATH 项。
- 没有安装记录的 `bin` 目录不会被自动接管或覆盖，需要先处理冲突。
- 新脚本只管理新目录布局，不负责卸载旧版单文件安装。
- 非交互 SSH 会话可能不会加载 shell 配置。远端 PATH 中没有 `mcpm-tools` 时，可用 `--ssh-mcpm` 指定绝对路径，例如 `/home/alice/.local/share/mcpm/bin/mcpm-tools`。

### 手动安装

[发布附件](https://github.com/EDGW/mcpm/releases/tag/v0.1.0-dev.26w43c) 中提供完整的 `bin/` 目录和 `manifest`：

- macOS/Linux：`mcpm-PLATFORM-ARCH[-LIBC].tar.gz`。
- Windows：`mcpm-windows-x86_64.zip`。
- 每个安装包附带对应的 `.sha256` 文件。Linux 提供 glibc 和 musl 两类产物，安装脚本选择 musl。

解压到自选目录并将其中的 `bin` 加入 PATH 即可。手动安装由使用者管理，安装器不会自动接管。
