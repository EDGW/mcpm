# mc package manager

[简体中文](README-zh.md)

This project is still in early development.

[Command reference](crates/mcpm/docs/README.md) · [Developer guide](crates/mcpm/docs/dev/README.md)

The command reference is currently difficult to read and will be improved over time.

## Installation

Install pre-release [v0.1.0-dev.26w43d](https://github.com/EDGW/mcpm/releases/tag/v0.1.0-dev.26w43d).
The installer supports **v0.1.0-dev.26w43c and newer**; older tags are rejected before downloading.
All five executables are installed together: `mcpm`, `mcpm-server`, `mcpmc`, `mcpm-client`, and `mcpm-tools`.

| System | Architectures | Program location | PATH entry |
| --- | --- | --- | --- |
| Windows | x64 | `%LOCALAPPDATA%\Programs\mcpm\bin` | Added to the current user's PATH and current PowerShell session |
| macOS | Intel, Apple Silicon | `~/.local/share/mcpm/bin` | Links in `~/.local/bin` |
| Linux | x86_64, ARM64 | `~/.local/share/mcpm/bin` | Links in `~/.local/bin`; statically linked musl executables |

Installation does not require administrator privileges. Configuration, caches, and logs remain under `~/.mcpm`; project-local `.mcpm` directories are independent of the program installation.

### Windows (PowerShell 5.1 or later)

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-dev.26w43d/scripts/install.ps1))) -Version v0.1.0-dev.26w43d
```

Uninstall:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-dev.26w43d/scripts/install.ps1))) -Uninstall
```

PATH changes apply to the current PowerShell session and newly opened terminals. Other existing terminals need to be reopened. Close running mcpm processes before upgrading if Windows reports files in use.

### macOS and Linux (sh)

Requires `curl`, `tar`, and either `sha256sum` or `shasum`.

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-dev.26w43d/scripts/install.sh | sh -s -- v0.1.0-dev.26w43d
```

If `~/.local/bin` is not already on PATH, add the following line to your shell configuration (`~/.zshrc` for zsh or `~/.bashrc` for bash):

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Reload that configuration or open a new terminal. The installer prints this instruction when necessary and does not edit shell configuration files.

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-dev.26w43d/scripts/install.sh | sh -s -- --uninstall
```

### Upgrade, repair and uninstall behavior

- Run the installation command with a supported tag to upgrade or downgrade the complete suite. Repeat the same version to repair missing executables.
- The installer verifies the archive's SHA-256, manifest, and every executable's version before changing the installation.
- File replacement is staged, with backup and rollback on failure. If rollback cannot finish, the installer retains the backup and reports its location. Do not delete it before recovering the installation.
- Concurrent installers are refused. After an interrupted Unix installer, remove the reported `.install-lock` directory only after checking that no installer is running; retained `.stage.*` directories may contain a recovery backup.
- Installation records identify managed files. Uninstall removes those executables, their owned Unix links, and Windows PATH entries added by the installer. Other files and preexisting PATH entries are retained.
- Uninstall preserves configuration, caches, logs, and project data.

### Existing installations and SSH

- Previous Unix installations in `/usr/local/bin/mcpm` are kept. If one shadows the new command, review PATH or remove that old executable explicitly after confirming its origin.
- Previous Windows installations at `%LOCALAPPDATA%\Programs\mcpm\mcpm.exe` and their PATH entries are kept. The installer warns when this executable still takes precedence; remove the old executable and old PATH entry explicitly after checking the new suite.
- An untracked `bin` directory is never silently adopted or replaced. Resolve a reported conflict before installing.
- These scripts manage the new layout only; they do not uninstall the old single-file layout.
- Noninteractive SSH sessions may not load your shell configuration. If the remote `mcpm-tools` is unavailable on PATH, use `--ssh-mcpm` with its absolute path, for example `/home/alice/.local/share/mcpm/bin/mcpm-tools`.

### Manual installation

[Release assets](https://github.com/EDGW/mcpm/releases/tag/v0.1.0-dev.26w43d) contain a complete `bin/` directory and a `manifest`:

- macOS/Linux: `mcpm-PLATFORM-ARCH[-LIBC].tar.gz`.
- Windows: `mcpm-windows-x86_64.zip`.
- Each archive has a matching `.sha256` file. Linux provides glibc and musl variants; the install script selects musl.

Extract the suite to a directory of your choice and add its `bin` directory to PATH. Manually installed copies remain your responsibility; the installer does not manage them automatically.
