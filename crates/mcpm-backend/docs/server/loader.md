# `server.loader.add`

Host-side streaming operation that installs `forge`, `fabric`, or `neoforge` in place on a registered v4 Vanilla server.

## Inputs and results

| Field | Type | Description |
| --- | --- | --- |
| `path` | `PathBuf` | Server directory on the owning Host. |
| `family` | `LoaderFamily` | `forge`, `fabric`, or `neoforge`. |
| `loader_version` | `Option<String>` | Exact loader version; `None` selects the recommended or latest version for the server's Minecraft version. |
| `download_config` | `Vec<u8>` | `DownloadConfig` encoded by its library codec. |
| `force` | `bool` | Allows overwriting conflicting non-vanilla runtime files. |

Returns `StreamingOutput<AddLoaderEvent>`:

| Event | Meaning |
| --- | --- |
| `Stage { stage, message }` | Stage transition (`preflight`, `resolve`, `download`, `install`, `verify`, `publish`). |
| `DownloadProgress { message, received, total }` | Transfer progress for metadata or installer JAR downloads. |
| `InstallerAttempt { source, attempt }` | Download source and attempt index for installer execution. |
| `Notice(String)` | Warning or informational notice. |
| `Completed(Box<ServerInfo>)` | Post-commit server summary (inspected without creating `mods/`). |

## Pipeline

1. **Preflight**: validates the download source (verifying the bundled `mcloader-injector-agent.jar` beside the Host executable when a mirror table is needed), checks `workspace::require_local`, confirms no active `running.lock`, and snapshots `.mcpm/server.json`, `launch.toml`, `conf.toml`, and the vanilla JAR. If a pending v1 publication journal exists, acquires `.mcpm/server.lock`, restores the previous configuration, and returns an error.
2. **Resolve**: resolves and freezes the installer artifact via `download::loader`. Stages 1–2 perform no disk writes (except pending-journal recovery).
3. **Download and install**: ensures `/loader-install` in `.mcpm/.gitignore`, creates `.mcpm/loader-install/<id>/`, downloads the installer JAR, stages the verified agent JAR for mirror sources, and runs the installer inside the isolated staging tree while watching for stream cancellation.
4. **Verify**: expands platform argument files when present, merges `launch.toml` without touching `conf.toml`, and verifies the resulting loader identity through `BUILTIN_LOADERS`.
5. **Publish**: enters a non-interruptible window holding `.mcpm/server.lock` after confirming no active `running.lock`, re-verifies the snapshot and file conflicts, writes allowlisted runtime files, and commits `.mcpm/server.json` + `launch.toml` atomically via `config::commit`.
6. **Cleanup**: removes the staging directory and emits `Completed(ServerInfo)`.

## Download sources and errors

- **Missing `.sha1`**: if all configured sources fail to supply the installer's `.sha1` file (and the stream was not cancelled), emits `Notice` and downloads the installer JAR without checksum verification. When a checksum is obtained, mismatch fails strictly.
- **Mirror injection**: the first source exporting a static `MappingTable` drives installer execution. Non-empty tables attach `-javaagent:<staged-agent>` and `-Dmcloader.injector.table=...`; empty tables (`official`) run directly.
- **Installer failure**: installer execution runs once against the selected source without automatic source switching; non-zero exit returns `InstallerFailed` with the source ID and log tails.

[Server operations](README.md) · [简体中文](../../docs-zh/server/loader.md)
