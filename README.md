# mc package manager

[简体中文](README-zh.md)

This project is still in early development.

[Command reference](mcpm/docs/README.md) · [Developer guide](mcpm/docs/dev/README.md)

The command reference is currently difficult to read and will be improved over time.

## Installation

The commands below install pre-release [v0.1.0-dev.26w43b](https://github.com/EDGW/mcpm/releases/tag/v0.1.0-dev.26w43b). Run the install command again to replace an existing installation.

| System | Supported architectures | Install location |
| --- | --- | --- |
| Windows | x64 | `%LOCALAPPDATA%\Programs\mcpm\mcpm.exe`, automatically added to the current user's PATH |
| macOS | Intel, Apple Silicon | `/usr/local/bin/mcpm` |
| Linux | x86_64, ARM64 | `/usr/local/bin/mcpm`, using the statically linked musl build |

macOS and Linux require `curl`. The script selects the architecture automatically and may request a password through `sudo` to write to the system directory. Uninstalling preserves configuration, caches, and logs under `~/.mcpm`.

### Windows (PowerShell 5.1 or later)

Install:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.ps1))) -Version v0.1.0-dev.26w43b
```

Uninstall:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.ps1))) -Uninstall
```

Windows installation does not require administrator privileges. PATH changes apply to the current PowerShell session and newly opened terminals; uninstalling also removes this PATH entry.

### macOS (sh)

Install:

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.sh | sh -s -- v0.1.0-dev.26w43b
```

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.sh | sh -s -- --uninstall
```

### Linux (sh)

Install:

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.sh | sh -s -- v0.1.0-dev.26w43b
```

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/main/scripts/install.sh | sh -s -- --uninstall
```
