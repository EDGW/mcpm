# `server.new`

## Inputs and results

`server.new` accepts two inputs and returns `StreamingOutput<NewEvent>`:

| Input | Result |
| --- | --- |
| `NewInput::Prepare { path, version, download_config, rcon_requested, loader }` | Validates the directory, Minecraft version, RCON options, and optional loader target; returns `Prepared`. |
| `NewInput::Install { prepared, accept_eula, no_git, git, rcon }` | Installs the prepared vanilla server (and optional loader); returns `Installed { server, git, java_major, rcon, loader_error }`. |

`download_config` uses the `DownloadConfig` codec. `Prepared` carries the resolved Minecraft artifact, optional `PreparedLoader`, and original download configuration into `Install`.

## Creation behavior

- The parent directory must exist and the target must be absent or empty; files and symbolic links are rejected.
- `Prepare` runs without writing to disk so the caller can prompt for EULA consent before installation begins.
- `Install` downloads and verifies `server.jar`, writes `eula.txt`, generates RCON settings when applicable, initializes Git, publishes launch configuration, and registers the Local workspace.
- Vanilla download or initialization failures keep completed files and report the failed stage.

## Mod loader

- When `loader` is set, `Prepare` validates installer source compatibility (including bundled mirror agent verification when needed) and freezes the canonical `PreparedLoader` version.
- After vanilla setup completes, `Install` runs the shared [`server.loader.add`](loader.md) pipeline against the new server.
- On a loader failure, `Install` still returns `Installed` carrying the vanilla `ServerInfo`, RCON details, and `loader_error: Some(...)`. Stream cancellation aborts with `Err`.

## RCON

- Versions with `releaseTime` on or after `1.9-pre4` receive default RCON settings; omitted passwords generate a UUID v4.
- Earlier releases reject explicit `--rcon-port` or `--rcon-password` (`rcon_requested: true`); `--no-rcon` (`rcon: None`) remains valid.
- Result status is `Created { port, password }`, `Disabled`, or `UnsupportedVersion`. `Created` returns the plaintext password for display and redacts it in `Debug`.

[Server operations](README.md) · [简体中文](../../docs-zh/server/new.md)
