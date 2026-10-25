# Release packages and installation

- `package.py` reads Cargo metadata for the version and binary inventory. It verifies each executable's version and help, checks musl binaries for a dynamic interpreter, and writes a complete archive plus SHA-256. A supplied release tag must match the Cargo version.
- `install.sh` and `install.ps1` are standalone bootstrap scripts. Each owns manifest interpretation and serialization on its platform; manifests are data and are never evaluated as shell code.
- Both installers support SemVer tags starting at `v0.1.0-dev.26w43c`. Older releases use a different distribution layout and are rejected before download.

## Package and installation manifests

UTF-8 text, one `key=value` record per line:

```text
format=1
version=v0.1.0-dev.26w43c
asset=mcpm-macos-arm64
file=mcpm
file=mcpm-client
file=mcpm-server
file=mcpm-tools
file=mcpmc
```

- `format`, `version`, and `asset` occur exactly once. `file` repeats for each executable basename under `bin/`; no paths, duplicates, or undeclared archive files are accepted.
- Windows executable basenames include `.exe`. Windows installation records additionally contain `path_user=0|1` and `path_process=0|1`, identifying PATH entries introduced by the installer.
- Installed manifests are the ownership record used by upgrades and uninstall. A missing manifest prevents automatic adoption of an existing `bin` directory. Corrupt manifests fail rather than falling back to inferred ownership.
- The package producer defines archive contents. Installers consume the manifest inventory rather than maintaining another list of executable names.

## Installation transaction

1. Acquire the installation lock, read the existing ownership record, and check for unmanaged files.
2. Download into a staging directory on the installation filesystem; verify the checksum, archive layout, manifest, and executable versions.
3. Move managed `bin` and manifest into the staging backup. Install the new `bin`, Unix links or Windows PATH entries, and commit the new manifest.
4. Clean up after success. On failure, restore the previous suite and PATH. If restoration fails, retain the backup and report its location.

The file set is not replaced atomically. Locks serialize cooperating installers; they do not stop users from launching programs. Unix handles signals through cleanup, but a forced termination or power loss can leave a staging backup or stale directory lock. Windows uses an exclusive file handle whose lock is released when the process exits; the empty lock file stays in place to avoid delete/open races.

Uninstall removes only recorded executables and owned links/PATH entries. Unknown files, legacy installations, and user data are preserved. Upgrade refuses unmanaged files in `bin` rather than moving or deleting them as part of the suite.

## Validation

```sh
shellcheck scripts/install.sh
python3 scripts/tests/test_install.py
python3 scripts/package.py --binary-dir mcpm/target/debug \
  --target aarch64-apple-darwin --asset mcpm-macos-arm64 \
  --tag v0.1.0-dev.26w43c
```

Adjust target and asset to the host. Packaging runs the executables, so the build must be runnable on that host.

On Windows, use PowerShell 5.1 or newer:

```powershell
./scripts/tests/install.Tests.ps1 -BinaryDir mcpm/target/x86_64-pc-windows-msvc/release
```

Unix tests inject local download and filesystem failures in an isolated home. Windows tests use a locally generated archive and restore the original environment and user PATH in `finally`. They exercise repair, checksum and incomplete-package rejection, rollback, concurrent-install refusal, legacy preservation, and PATH ownership. CI runs these checks before uploading release assets.
