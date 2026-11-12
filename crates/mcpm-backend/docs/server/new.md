# `server.new`

## Inputs and results

`server.new` accepts two inputs, both returning `StreamingOutput<NewEvent>`:

| Input | Result |
| --- | --- |
| `NewInput::Prepare { path, version, download_config, rcon_requested }` | Validate the directory and version, then return `Prepared`. |
| `NewInput::Install { prepared, accept_eula, no_git, git, rcon }` | Install the selected version and return `Installed { server, git, java_major, rcon }`. |

`download_config` is encoded by the `DownloadConfig` codec. `Prepared` retains the resolved download artifact and original configuration for installation.

## Creation behavior

- The parent must exist and the target must be absent or empty. File and symbolic-link targets fail.
- Prepare fetches the version catalog and validates the version and RCON options before the caller submits EULA consent.
- Install downloads and verifies `server.jar`, writes `eula.txt`, creates RCON settings when applicable, then prepares Git, publishes launch configuration, and registers the Local workspace.
- Download or initialization failures preserve completed files and identify the failed stage.

## RCON

- Parsed `releaseTime` determines eligibility: versions released at or after `1.9-pre4` receive RCON configuration by default. Missing passwords generate UUID v4.
- Explicit port or password options on an older release fail. `rcon_requested` represents these overrides; `--no-rcon` corresponds to `rcon: None`.
- The returned RCON status is `Created { port, password }`, `Disabled`, or `UnsupportedVersion`. Creation returns the actual password for display; its Debug representation redacts the password to keep it out of logs.
