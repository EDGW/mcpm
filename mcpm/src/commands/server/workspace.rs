//! Workspace type dispatch and Remote registration storage; local server metadata belongs to mcpm-backend.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use mcpm_backend::local::mc::server::MinecraftServer;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ssh::Connection;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Kind {
    Local,
    Remote,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    #[serde(rename = "type")]
    kind: Kind,
}

/// Connection data only; no copy of the remote server's Minecraft or loader metadata.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Remote {
    version: u32,
    ssh: String,
    ssh_mcpm: Option<String>,
    pub path: String,
}

impl Remote {
    pub fn new(connection: Connection, path: String) -> Result<Self> {
        let remote = Self {
            version: 2,
            ssh: connection.ssh,
            ssh_mcpm: connection.ssh_mcpm,
            path,
        };
        remote.validate()?;
        Ok(remote)
    }

    pub fn connection(&self) -> Connection {
        Connection {
            ssh: self.ssh.clone(),
            ssh_mcpm: self.ssh_mcpm.clone(),
        }
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 2,
            "unsupported Remote server.json version {}; expected 2",
            self.version
        );
        ensure!(
            !self.path.trim().is_empty() && !self.path.contains('\0'),
            "remote server directory must be nonempty and contain no NUL"
        );
        // Validate with the shared SSH parser; never execute a shell to interpret settings.
        self.connection().command()?;
        Ok(())
    }

    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".mcpm/server.json");
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read Remote registration {path:?}"))?;
        let remote: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid Remote registration {path:?}"))?;
        remote.validate()?;
        Ok(remote)
    }
}

/// Missing mcpm.json is the legacy Local layout; malformed existing files never fall back.
pub(super) fn kind(root: &Path) -> Result<Kind> {
    let path = root.join(".mcpm/mcpm.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // A dangling link is an existing invalid marker, not a legacy layout.
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Kind::Local),
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to inspect workspace manifest {path:?}"));
                }
                Ok(_) => {
                    return Err(error)
                        .with_context(|| format!("failed to read workspace manifest {path:?}"));
                }
            }
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read workspace manifest {path:?}"));
        }
    };
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid workspace manifest {path:?}"))?;
    ensure!(
        manifest.version == 1,
        "unsupported mcpm.json version {}; expected 1 ({path:?})",
        manifest.version
    );
    Ok(manifest.kind)
}

/// All remote handlers and local operations use this gate; it never starts a connection.
pub(super) fn require_local(root: &Path) -> Result<()> {
    ensure!(
        kind(root)? == Kind::Local,
        WorkspaceError::RemoteTargetNotLocal {
            path: root.to_owned()
        }
    );
    Ok(())
}

pub(super) fn load_local(root: PathBuf) -> Result<MinecraftServer> {
    require_local(&root)?;
    Ok(MinecraftServer::load(root)?)
}

pub(super) fn inspect_local(root: PathBuf) -> Result<MinecraftServer> {
    require_local(&root)?;
    Ok(MinecraftServer::inspect(root)?)
}

/// Serializes mcpm registration writers across processes. Readers may observe an
/// incomplete first registration, which fails closed rather than selecting another type.
pub(super) struct Writer {
    root: PathBuf,
    _lock: File,
}

impl Writer {
    pub fn acquire(root: &Path) -> Result<Self> {
        ensure!(
            root.is_dir(),
            "workspace must be an existing directory: {root:?}"
        );
        fs::create_dir_all(root.join(".mcpm"))
            .context("failed to create workspace metadata directory")?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".mcpm/registration.lock"))
            .context("failed to open workspace registration lock")?;
        lock.try_lock()
            .context("workspace registration is already being changed or cannot be locked")?;
        Ok(Self {
            root: root.to_owned(),
            _lock: lock,
        })
    }

    pub fn mark_local(&self) -> Result<()> {
        require_local(&self.root)?;
        if !self.root.join(".mcpm/mcpm.json").exists() {
            self.mark(Kind::Local)?;
        }
        Ok(())
    }

    fn mark(&self, kind: Kind) -> Result<()> {
        self.write("mcpm.json", &Manifest { version: 1, kind }, false)
    }

    /// Type conversion is deliberately excluded, including with --force.
    pub fn remote(&self, remote: &Remote, force: bool) -> Result<()> {
        remote.validate()?;
        let marker_exists = self.root.join(".mcpm/mcpm.json").try_exists()?;
        let server = self.root.join(".mcpm/server.json");
        let server_exists = match fs::symlink_metadata(&server) {
            Ok(metadata) => {
                ensure!(
                    !metadata.is_dir(),
                    "server configuration path is a directory: {server:?}"
                );
                true
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => {
                return Err(error).context("failed to inspect existing server registration");
            }
        };
        let existing_kind = kind(&self.root)?;
        ensure!(
            !(existing_kind == Kind::Local && (marker_exists || server_exists)),
            "cannot convert a Local workspace to Remote with server init, even with --force"
        );
        ensure!(
            !server_exists || force,
            "Remote registration already exists; use --force to replace its connection settings"
        );
        self.write("server.json", remote, force)?;
        if !marker_exists && let Err(error) = self.mark(Kind::Remote) {
            // No previous registration existed, and the writer lock is still held.
            fs::remove_file(&server).context(
                "failed to remove incomplete Remote registration after mcpm.json write failure",
            )?;
            return Err(error).context(
                "failed to publish Remote workspace type; removed incomplete server.json",
            );
        }
        tracing::info!(workspace = %self.root.display(), "Remote workspace registration written");
        Ok(())
    }

    fn write(&self, name: &str, value: &impl Serialize, replace: bool) -> Result<()> {
        let directory = self.root.join(".mcpm");
        let path = directory.join(name);
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(&bytes)?;
        if replace {
            temporary.persist(&path).map_err(|error| error.error)?;
        } else {
            temporary
                .persist_noclobber(&path)
                .map_err(|error| error.error)?;
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
enum WorkspaceError {
    #[error(
        "server target {path:?} is a Remote workspace; this operation requires a Local Server (chained Remote workspaces are not supported)"
    )]
    RemoteTargetNotLocal { path: PathBuf },
}
