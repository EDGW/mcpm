# mc package manager

[English](README.md)

该项目仍在早期开发中。

[命令手册](mcpm/docs-zh/README.md) · [开发手册](mcpm/docs-zh/dev/README.md)

命令手册目前可读性较差，会择时完善。

## 安装

以下命令安装预发布版本 [v0.1.0-dev.26w43b](https://github.com/EDGW/mcpm/releases/tag/v0.1.0-dev.26w43b)。再次运行安装命令可覆盖安装。

| 系统 | 支持的架构 | 安装位置 |
| --- | --- | --- |
| Windows | x64 | `%LOCALAPPDATA%\Programs\mcpm\mcpm.exe`，自动加入当前用户的 PATH |
| macOS | Intel、Apple Silicon | `/usr/local/bin/mcpm` |
| Linux | x86_64、ARM64 | `/usr/local/bin/mcpm`，使用静态链接的 musl 版本 |

macOS 和 Linux 需要 `curl`；安装脚本自动选择架构，写入系统目录时可能通过 `sudo` 请求密码。卸载命令保留 `~/.mcpm` 中的配置、缓存和日志。

### Windows（PowerShell 5.1 或更高版本）

安装：

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.ps1))) -Version v0.1.0-dev.26w43b
```

卸载：

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.ps1))) -Uninstall
```

Windows 安装无需管理员权限，PATH 对当前 PowerShell 和以后新开的终端生效；卸载时一并移除该 PATH 项。

### macOS（sh）

安装：

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.sh | sh -s -- v0.1.0-dev.26w43b
```

卸载：

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.sh | sh -s -- --uninstall
```

### Linux（sh）

安装：

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.sh | sh -s -- v0.1.0-dev.26w43b
```

卸载：

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.sh | sh -s -- --uninstall
```
