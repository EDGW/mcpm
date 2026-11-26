# mc package manager

[简体中文](README-zh.md)

This project is still in early development.

[Command reference](crates/mcpm/docs/README.md) · [Legacy developer guide — not yet migrated](crates/mcpm/docs/legacy/dev/README.md) · [Backend operations](crates/mcpm-backend/docs/README.md)

The command reference is currently difficult to read and will be improved over time.

## Installation

Install pre-release [v0.1.0-rc1](https://github.com/EDGW/mcpm/releases/tag/v0.1.0-rc1).
The installer supports **v0.1.0-rc1 and newer**; versions earlier than `v0.1.0-rc1` are not supported and are rejected before downloading.
All six executables and bundled artifacts are installed together: `mcpm`, `mcpm-server`, `mcpmc`, `mcpm-client`, `mcpm-tools`, and `mcpmd`.

| System | Architectures | Program location | PATH entry |
| --- | --- | --- | --- |
| Windows | x64 | `%LOCALAPPDATA%\Programs\mcpm\bin` | Added to the current user's PATH and current PowerShell session |
| macOS | Intel, Apple Silicon | `~/.local/share/mcpm/bin` | Links in `~/.local/bin` |
| Linux | x86_64, ARM64 | `~/.local/share/mcpm/bin` | Links in `~/.local/bin`; statically linked musl executables |

Installation does not require administrator privileges. Configuration, caches, and logs remain under `~/.mcpm`; project-local `.mcpm` directories are independent of the program installation.

### Windows (PowerShell 5.1 or later)

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-rc1/scripts/install.ps1))) -Version v0.1.0-rc1
```

Uninstall:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-rc1/scripts/install.ps1))) -Uninstall
```

PATH changes apply to the current PowerShell session and newly opened terminals. Other existing terminals need to be reopened. Close running mcpm processes before upgrading if Windows reports files in use.

### macOS and Linux (sh)

Requires `curl`, `unzip`, and either `sha256sum` or `shasum`.

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-rc1/scripts/install.sh | sh -s -- v0.1.0-rc1
```

If `~/.local/bin` is not already on PATH, add the following line to your shell configuration (`~/.zshrc` for zsh or `~/.bashrc` for bash):

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Reload that configuration or open a new terminal. The installer prints this instruction when necessary and does not edit shell configuration files.

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/EDGW/mcpm/v0.1.0-rc1/scripts/install.sh | sh -s -- --uninstall
```

To use a GitHub URL-prefix proxy (`--proxy` / `-Proxy`), see [proxy options](scripts/README.md#github-url-prefix-proxy).

### Upgrade, repair and uninstall behavior

- Reinstall to upgrade, downgrade, or repair. If the installation directory already contains files, confirm replacing and deleting all its previous contents. Declining does not download or replace anything.
- The installer verifies the ZIP's SHA-256, runs `mcpm-tools debug validate --exact` on the extracted suite, and checks executable versions. It runs `mcpm-tools debug hello` after setup succeeds.
- File replacement is staged, with backup and rollback on failure. If rollback cannot finish, the installer retains the backup and reports its location. Do not delete it before recovering the installation.
- Concurrent installers are refused. After an interrupted Unix installer, remove the reported `.install-lock` directory only after checking that no installer is running; retained `.stage.*` directories may contain a recovery backup.
- Uninstall removes recorded executables and artifacts, owned Unix links, and Windows PATH entries added by the installer. Other files and preexisting PATH entries are retained.
- Uninstall preserves configuration, caches, logs, and project data.

### Existing installations and SSH

- Previous Unix installations in `/usr/local/bin/mcpm` are kept. If one shadows the new command, review PATH or remove that old executable explicitly after confirming its origin.
- A previous Windows executable inside `%LOCALAPPDATA%\Programs\mcpm` is removed on approved replacement. Preexisting PATH entries and executables outside the installation directory are preserved.
- Noninteractive SSH sessions may not load your shell configuration. Installers record `mcpmd` in `~/.mcpm/mcpmd.path` on Unix or `%LOCALAPPDATA%\mcpm\mcpmd.path` on Windows. Without an explicit `--ssh-mcpmd`, SSH reads the remote host's record; a missing file warns and falls back to remote PATH. If that fallback also fails, the command reports an error. See [remote discovery](crates/mcpmd/docs/README.md#remote-executable-discovery). Unmigrated mods/cache commands still require remote `mcpm-tools`.

### Manual installation

[Release assets](https://github.com/EDGW/mcpm/releases) for the ZIP layout include `bin/` and `manifest.json`:

- macOS/Linux: `mcpm-PLATFORM-ARCH[-LIBC].zip`.
- Windows: `mcpm-windows-x86_64.zip`.
- Each archive has a matching `.sha256` file. Linux provides glibc and musl variants; the install script selects musl.

Extract the suite and add its `bin` directory to PATH. Run `mcpm-tools debug validate` to check the extracted files.
