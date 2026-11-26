# `mcpm new`

Create a Minecraft server in a new or empty directory, optionally installing a mod loader right after vanilla setup.

```text
mcpm new [OPTIONS] [DIR_PATH]
```

## Arguments and options

| Argument or option | Description |
| --- | --- |
| `[DIR_PATH]` | Target directory; defaults to `.`. Must be absent or empty, with an existing parent directory. Files and symbolic links are rejected. |
| `--version <VERSION>` | Minecraft version ID, `latest` (latest release), or `latest-snapshot`. Defaults to `latest`. |
| `--loader <LOADER[:VERSION]>` | Mod loader to install after vanilla setup: `forge`, `fabric`, or `neoforge`, with an optional `:<version>` suffix. |
| `--loader-version <VERSION>` | Exact loader version. Requires `--loader` and conflicts with inline `:<version>`. |
| `--source <SOURCE1,SOURCE2,...>` | Download sources tried in order: `bmclapi`, `official`. Replaces the default `bmclapi,official`. |
| `--source-timeout <CONNECT[:READ]>` | Positive integer seconds. `CONNECT` alone sets `READ` to `3 * CONNECT`. Defaults to `10:30`. `READ` applies per response read, not to the whole download. |
| `--eula` | Fetch the current Minecraft EULA and prompt for confirmation. Defaults to decline. |
| `--rcon-port <PORT>` | RCON port in generated `server.properties`; defaults to `25575`. |
| `--rcon-password <PASSWORD>` | RCON password. Defaults to a random UUID/GUID generated on the server Host and printed after creation. |
| `--no-rcon` | Skip generating `server.properties`. Conflicts with `--rcon-port` and `--rcon-password`. |
| `--no-git` | Skip Git discovery and repository creation on the server Host. |
| `--git` | Ensure the server directory itself has a Git repository. Conflicts with `--no-git`. |
| `--full-command` | Show the complete generated startup command after installation. |

## Examples

```sh
mcpm new ./server --version 1.20.1
mcpm new ./server --version 1.21.1 --loader fabric
mcpm new ./server --version 1.20.1 --loader forge:47.3.0 --eula
mcpm new ./server --version 1.20.1 --source official --source-timeout 5:20
mcpm --ssh 'ssh user@host' new /srv/minecraft --version 1.21.1 --loader neoforge
```

## Generated files

```text
server/
├── server.jar          # Vanilla server
├── eula.txt            # EULA status
├── server.properties   # RCON settings (1.9-pre4+, unless --no-rcon)
├── launch.toml         # Startup entry
├── conf.toml           # User configuration (nogui enabled by default)
└── .mcpm/
    ├── server.json     # Server identity
    └── mcpm.json       # Local workspace registration
```

When `--loader` is specified, mcpm installs the loader onto this vanilla server, adds its runtime files (such as `libraries/` and launcher JARs), and updates `.mcpm/server.json` and `launch.toml`.

If vanilla download or initialization fails, completed files remain in the target directory.

## Mod loader

- With `--loader`, mcpm validates the loader version and download source before prompting for `--eula`, without writing to disk.
- Loader installation runs after vanilla setup finishes and follows the same staging, mirror, and cancellation behavior as [`mcpm add-loader`](add-loader.md).
- If vanilla setup succeeds and loader installation fails, the vanilla server stays registered and usable. mcpm prints the error and a `mcpm add-loader` retry command to stderr, then exits non-zero.

## RCON

Versions released on or after `1.9-pre4` receive default RCON settings in `server.properties`:

```properties
enable-rcon=true
rcon.port=25575
rcon.password=<generated UUID>
```

- On earlier versions, explicit `--rcon-port` or `--rcon-password` fails during version validation; `--no-rcon` is still accepted.
- On Unix, generated `server.properties` is readable and writable only by its owner.
- Creation output displays the full RCON password. Protect terminal recordings and redirected output.

## EULA

- mcpm validates the Minecraft version (and `--loader`, if set) before fetching <https://www.minecraft.net/en-us/eula>.
- EULA retrieval connects directly to `www.minecraft.net` using `--source-timeout`, regardless of `--source`.
- Enter `y` or `yes` (case-insensitive) to write `eula=true`; any other input cancels creation.
- Without `--eula`, mcpm writes `eula=false`.
- `--eula` requires an interactive terminal and conflicts with `--raw`.

## Execution and Git

Creation runs locally by default. With `--ssh`, directory checks, version resolution, downloads, loader installation, Git setup, and registration run on the remote Host using the invocation's `--source` and `--source-timeout`; EULA confirmation remains local.

By default, mcpm reuses an existing Git repository in the target or a parent directory, or initializes one in the server directory. See [`init`](basics.md#git) for Git details and [README](../README.md) for global options and output modes.
