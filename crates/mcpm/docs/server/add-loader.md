# `mcpm add-loader`

Install `forge`, `fabric`, or `neoforge` in place on a registered v4 Vanilla server.

```text
mcpm add-loader <LOADER[:VERSION]> [OPTIONS]
```

## Arguments and options

| Argument or option | Description |
| --- | --- |
| `<LOADER[:VERSION]>` | Loader family (`forge`, `fabric`, `neoforge`), with an optional `:<version>` suffix (for example `fabric:0.16.10`). |
| `--server <PATH>` | Registered Vanilla server directory; defaults to `.`. |
| `--loader-version <VERSION>` | Exact loader version. Conflicts with inline `:<version>`. Omit to select the recommended or latest version for the server's Minecraft version. |
| `--source <SOURCE1,SOURCE2,...>` | Download sources tried in order: `bmclapi`, `official`. May be repeated or comma-separated; replaces the default `bmclapi,official`. |
| `--source-timeout <CONNECT[:READ]>` | Positive integer seconds. `CONNECT` alone sets `READ` to `3 * CONNECT`. Defaults to `10:30`. Applies to metadata and installer JAR downloads. |
| `--force` | Replace conflicting non-vanilla runtime files (such as existing library JARs or legacy Fabric launcher properties). |
| `--full-command` | Show the complete generated startup command after installation. |

## Examples

```sh
mcpm add-loader fabric
mcpm add-loader forge --server ./server --loader-version 47.3.0
mcpm add-loader neoforge:21.1.72 --server ./server --source official
mcpm --ssh 'ssh user@host' add-loader fabric --server /srv/minecraft
```

## Requirements

The target server must satisfy all of the following:

- Registered as a v4 `Vanilla` server in `.mcpm/server.json`.
- Stopped (no running server instance).
- `launch.toml` matches the Host platform (or is platform-independent), uses a `-jar` entry, and points to an existing vanilla JAR matching the registered Minecraft version.
- `process.workingDirectory` stays inside the server directory.

Servers that do not meet these conditions fail during preflight before any files are written. If an interrupted previous configuration publication is present, mcpm restores the previous configuration and exits so you can inspect the server before retrying.

## File changes and cancellation

| Scope | Behavior |
| --- | --- |
| Updated | `.mcpm/server.json`, `launch.toml`, required loader runtime files (such as `libraries/` and launcher JARs), and `fabric-server-launcher.properties` for Fabric `< 0.12.0` (preserving comments and other keys). |
| Unchanged | `conf.toml`, the original vanilla JAR, startup scripts, `eula.txt`, `server.properties`, worlds, and `mods/`. |
| Staging | Installer execution runs inside `.mcpm/loader-install/<id>/` (added to `.mcpm/.gitignore`) and is removed afterward. |

- `--force` allows overwriting conflicting non-vanilla runtime files. It does not relax target requirements and never overwrites the original vanilla JAR.
- Interrupting (`Ctrl-C` or SSH disconnect) before publication kills the installer process tree and removes the staging directory without changing server files. Once publication begins, mcpm waits for the configuration commit to finish.

## Download sources

- **Metadata and installer JAR**: tried across `--source` in order with `--source-timeout`. If none of the configured sources provides the installer's `.sha1` checksum (for example `--source bmclapi`), mcpm prints a warning on stderr and continues downloading without checksum verification.
- **Installer internal downloads**: uses the first compatible source in `--source`:
  - Mirror sources (such as `bmclapi`) inject rewrite rules using the bundled mirror agent beside `mcpm` / `mcpmd`. If the bundled agent is missing or corrupted, run `mcpm-tools debug validate` or reinstall mcpm.
  - Direct sources (`official`) run the installer without an agent.
  - Installer internal downloads do not switch to another source on failure. If a mirror lacks a required artifact, rerun with `--source official`.

## Output and execution

- **Console**: prints the updated server summary (same layout as `info`, `init`, and `new`, respecting `--full-command`). Progress, warnings, and errors go to stderr.
- **Raw** (`--raw` or redirected stdout): prints the 6-column server snapshot TSV (`name`, `path`, `mc_version`, `loader`, `loader_version`, `command`). Remote targets prepend `remote_path`, masked `remote_ssh`, and `remote_mcpmd`.

Runs locally by default, or on the remote Host via `mcpmd` for a Remote workspace or `--ssh`. See [README](../README.md) for global options and output modes.
