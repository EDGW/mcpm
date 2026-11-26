# Release packages and installation

- `package.py` reads Cargo metadata for the version and binary inventory. It verifies each executable's version and help, checks musl binaries for a dynamic interpreter, and writes an archive, its SHA-256, and `dist/manifest.json`. A supplied release tag must match the verified binary version.
- `install.sh` and `install.ps1` install ZIP releases for the current user. For previously published releases with the old archive layout, use the installer from the corresponding release tag.

## Pinned release artifacts

- Cargo builds of `mcpm` and `mcpmd` require Python 3.11+ on the build host (`python3` on Unix, `python` on Windows). The build reads `artifacts.toml` and stages its files beside the executables (`target/debug/` or `target/release/` by default). Each file is checked against its SHA-256. On a miss or mismatch, the build reuses a verified copy from `target/artifacts/<sha256>/` or downloads from the pinned URL and verifies it before publishing.
- `package.py` verifies every pinned artifact in `--binary-dir` and includes it alongside the executables in the platform ZIP.
- To update or add an artifact, edit `artifacts.toml`: the key is the distributed filename, and the entry supplies its HTTPS URL and lowercase SHA-256.

## GitHub URL-prefix proxy

Set `VERSION` (Unix) or `$Version` (PowerShell) to a release tag that ships the ZIP layout:

```sh
sh scripts/install.sh "$VERSION" --proxy https://gh-proxy.com
```

```powershell
./scripts/install.ps1 -Version $Version -Proxy https://gh-proxy.com
```

- The proxy prefix is prepended to the complete GitHub URL for both the archive and its `.sha256` file, for example `https://gh-proxy.com/https://github.com/EDGW/mcpm/releases/download/...`.
- HTTP and HTTPS prefixes are accepted, with or without a trailing slash. The Unix option may appear before or after the version.
- Omitting the option keeps the original direct URLs. Proxy failures do not silently fall back to direct downloads; checksum verification and rollback remain unchanged.
- This option does not affect the initial download of the installer itself. If necessary, also prefix that Raw URL. Use a script version containing this option; proxying an older script does not add the option.
- This is a URL-prefix service, not an HTTP/SOCKS forward proxy. Only use providers you trust: a checksum fetched from the same proxy is not independent proof of publisher authenticity.

## Package manifest

`dist/manifest.json` is also included byte-for-byte at the archive root. It describes one platform build:

- `format`: manifest schema version (`1`).
- `version`, `asset`, `target`: release version, archive basename without its extension, and Rust target triple.
- `files`: one `{ "path": "bin/…", "sha256": "…" }` record per shipped executable and pinned artifact. Paths are relative to the archive root. The manifest does not list itself; the separate `.sha256` file covers the whole archive.

After extracting a release, run `bin/mcpm-tools debug validate` (`bin\mcpm-tools.exe` on Windows). It finds `manifest.json` relative to the executable and checks each listed file's SHA-256. Pass `--exact` to also reject unlisted entries under `bin/`. Validation does not authenticate the manifest itself.

The archive's `manifest.json` is installed unchanged. The separate local `manifest` tracks which files the installer owns for uninstall, and on Windows also tracks which PATH entries it added.

## Installation transaction

1. Acquire the installation lock. If the destination already contains files, ask whether to replace and delete all its previous contents. Only `y` or `yes` confirms; declining or EOF leaves the installation unchanged.
2. Download the ZIP and verify its archive SHA-256. Stage only safe, unique file paths; run the extracted `mcpm-tools debug validate --exact` and confirm executable versions before moving any old files.
3. Move old installation contents into a backup. Publish the new `bin/` and `manifest.json`, update Unix links or Windows PATH entries, publish the daemon record, and commit the local ownership record. Restore the old installation if publication fails.
4. On success, run the newly installed `mcpm-tools debug hello` and discard the backup. The welcome guide does not control installation success.

The file set is not replaced atomically. Locks serialize cooperating installers; they do not stop users from launching programs. Unix handles signals through cleanup, but a forced termination or power loss can leave a staging backup or stale directory lock. Windows uses an exclusive file handle whose lock is released when the process exits; the empty lock file stays in place to avoid delete/open races.

Uninstall removes recorded executables and artifacts, both installed manifests, and owned links/PATH entries. It preserves untracked files, user configuration, caches, logs, and project data. Approved replacement removes all previous contents of the installation directory, including manually added files there.

### Daemon location record

- Install, upgrade, and repair write the final absolute `mcpmd` path to `~/.mcpm/mcpmd.path` on Unix or `%LOCALAPPDATA%\mcpm\mcpmd.path` on Windows. Packages must include the daemon.
- The file is UTF-8 text with one absolute executable path and a trailing LF; readers also accept CRLF or no final newline. It is not shell code and does not modify `settings.toml`.
- Publication uses a temporary file in the record directory and same-filesystem replacement. Failed installs restore the previous record, or remove the new record if none existed.
- Uninstall removes the record only when it still contains this installer's daemon path. Records pointing at another installation and all user settings remain untouched.

## Validation

```sh
shellcheck scripts/install.sh
python3 scripts/tests/test_install.py
python3 scripts/package.py --binary-dir target/debug \
  --target aarch64-apple-darwin --asset mcpm-macos-arm64
```

Adjust target and asset to the host. Packaging runs the executables, so the build must be runnable on that host.

On Windows, use PowerShell 5.1 or newer:

```powershell
./scripts/tests/install.Tests.ps1 -BinaryDir target/x86_64-pc-windows-msvc/release
```

Unix tests inject local download and filesystem failures in an isolated home. Windows tests use a locally generated archive and restore the original environment and user PATH in `finally`. They exercise repair, checksum and incomplete-package rejection, rollback, concurrent-install refusal, legacy preservation, and PATH ownership. CI runs these checks before uploading release assets.
