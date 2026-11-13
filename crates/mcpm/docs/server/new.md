# `mcpm new`

Install a vanilla Minecraft server in a new or existing empty directory and generate its mcpm registration and launch configuration.

```text
mcpm new [OPTIONS] [DIR_PATH]
```

## Arguments and options

| Argument or option | Description |
| --- | --- |
| `[DIR_PATH]` | Defaults to `.`. The target must be absent or an empty directory, and its parent must already exist. Files and symbolic links are rejected. |
| `--version <VERSION>` | Exact Minecraft version ID, `latest` (latest release), or `latest-snapshot`. Defaults to `latest`. |
| `--source <SOURCE1,SOURCE2,...>` | Download sources tried in order: `bmclapi`, `official`. An explicit list completely replaces the default `bmclapi,official`. |
| `--source-timeout <CONNECT[:READ]>` | Positive integer seconds. CONNECT alone sets READ to three times CONNECT. Defaults to 10 seconds for connecting and 30 seconds for reading. The read timeout applies to each response read, not the entire download. Can be used independently of `--source`. |
| `--eula` | Fetch and confirm the current Minecraft EULA. Defaults to no. |
| `--rcon-port <PORT>` | Generated RCON port; defaults to `25575`. |
| `--rcon-password <PASSWORD>` | RCON password. When omitted, a random UUID/GUID is generated on the server Host. The actual password is displayed after creation. |
| `--no-rcon` | Skip automatic `server.properties` creation. Conflicts with `--rcon-port` and `--rcon-password`. |
| `--no-git` | Skip Git discovery and creation on the Host that owns the server. |
| `--git` | Ensure the server directory itself has a Git repository. Conflicts with `--no-git`. |
| `--full-command` | Show the complete startup command after installation. |

## Examples

```sh
mcpm new ./server --version 1.20.1
mcpm new ./server --version 1.20.1 --source official --source-timeout 5:20 --eula
mcpm new ./server --version latest-snapshot --source bmclapi,official --source-timeout 5 --no-git
mcpm --ssh 'ssh user@host' new /srv/minecraft --version 1.20.1 --source official
```

## Installation result

```text
server/
├── server.jar          # Vanilla server
├── eula.txt            # EULA consent status
├── server.properties   # RCON settings generated according to version and options
├── launch.toml         # Startup entry for server.jar
├── conf.toml           # User settings; nogui enabled by default
└── .mcpm/
    ├── server.json     # Vanilla identity registration
    └── mcpm.json       # Local workspace registration
```

If downloading or initialization fails, the directory and completed files remain. Inspect their contents before proceeding.

## Automatic RCON configuration

Versions released at or after `1.9-pre4` receive these settings by default:

```properties
enable-rcon=true
rcon.port=25575
rcon.password=<a random UUID generated for this installation>
```

- Explicit `--rcon-port` or `--rcon-password` options on an earlier release fail during version validation; `--no-rcon` remains accepted.
- The generated password is stored in `server.properties`. On Unix, the file is readable and writable only by its owner.
- Creation results display the full RCON password. Protect terminal recordings and redirected output.
- RCON becomes active when the server starts. Protect the password and restrict port access as needed.

## EULA confirmation

The selected Minecraft version is validated before fetching the current EULA from <https://www.minecraft.net/en-us/eula>. `latest` uses the version selected during validation.

Enter `y` or `yes`, case-insensitively, to accept and write `eula=true`; any other answer cancels installation. Version validation or EULA retrieval failure stops creation.

Without `--eula`, installation writes `eula=false`. Review the agreement and edit `eula.txt` before launching.

`--eula` requires an interactive terminal and conflicts with `--raw`. EULA retrieval follows the selected download sources; include `official` when using this option.

## Execution and Git

Installation is local by default. With explicit `--ssh`, directory checks, version resolution, server downloads, Git, and registration run remotely using the source order and timeouts supplied for this invocation. EULA confirmation remains local.

By default, reuse the target or parent Git repository, or create one in the server directory when none exists. See the [`init` documentation](basics.md#git) for shared Git behavior and [README](../README.md) for global options and output modes.
