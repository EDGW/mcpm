//! Workspace type dispatch and Remote registration storage; local server metadata belongs to mcpm-backend.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use mcpm_lib::local::mc::server::MinecraftServer;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use mcpm_exec::operation::Connection;

mod resolve;
pub use resolve::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Local,
    Remote,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    #[serde(rename = "type")]
    kind: Kind,
}

/// Connection data only; no copy of the remote server's Minecraft or loader metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remote {
    version: u32,
    ssh: String,
    ssh_mcpmd: Option<String>,
    pub path: String,
}

impl Remote {
    pub fn new(connection: Connection, path: String) -> Result<Self> {
        let remote = Self {
            version: 3,
            ssh: connection.ssh,
            ssh_mcpmd: connection.mcpmd,
            path,
        };
        remote.validate()?;
        Ok(remote)
    }

    pub fn connection(&self) -> Connection {
        Connection {
            ssh: self.ssh.clone(),
            mcpmd: self.ssh_mcpmd.clone(),
        }
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 3,
            "unsupported Remote server.json version {}; expected 3",
            self.version
        );
        ensure!(
            !self.path.trim().is_empty() && !self.path.contains('\0'),
            "remote server directory must be nonempty and contain no NUL"
        );
        ensure!(
            !self.ssh.trim().is_empty() && !self.ssh.contains('\0'),
            "SSH connection command must be nonempty and contain no NUL"
        );
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
pub fn kind(root: &Path) -> Result<Kind> {
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
pub fn require_local(root: &Path) -> Result<()> {
    ensure!(
        kind(root)? == Kind::Local,
        WorkspaceError::RemoteTargetNotLocal {
            path: root.to_owned()
        }
    );
    Ok(())
}

pub fn load_local(root: PathBuf) -> Result<MinecraftServer> {
    require_local(&root)?;
    Ok(MinecraftServer::load(root)?)
}

pub fn inspect_local(root: PathBuf) -> Result<MinecraftServer> {
    require_local(&root)?;
    Ok(MinecraftServer::inspect(root)?)
}

/// Serializes mcpm registration writers across processes. Readers may observe an
/// incomplete first registration, which fails closed rather than selecting another type.
pub struct Writer {
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
        let writer = Self {
            root: root.to_owned(),
            _lock: lock,
        };
        append_gitignore(root)?;
        Ok(writer)
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

impl Drop for Writer {
    fn drop(&mut self) {
        // Closing one descriptor does not release a lock inherited by another process.
        if let Err(error) = self._lock.unlock() {
            tracing::warn!(path = %self.root.display(), %error, "failed to release workspace registration lock");
        }
    }
}

/// Appends the bundled ignore block unless the file already ends with it, ignoring blank lines.
/// The caller holds the workspace registration lock.
fn append_gitignore(root: &Path) -> Result<()> {
    const BLOCK: &str = include_str!("../../config/workspace.gitignore");
    let path = root.join(".mcpm/.gitignore");
    let mut file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(&path)
        .with_context(|| format!("failed to open workspace Git ignore file {path:?}"))?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .with_context(|| format!("failed to read workspace Git ignore file {path:?}"))?;
    let mut trailing_lines = gitignore_lines(&contents).rev();
    if gitignore_lines(BLOCK.as_bytes())
        .rev()
        .all(|line| trailing_lines.next() == Some(line))
    {
        return Ok(());
    }
    if !contents.is_empty() {
        file.write_all(b"\n").with_context(|| {
            format!("failed to append newline to workspace Git ignore file {path:?}")
        })?;
    }
    file.write_all(BLOCK.as_bytes())
        .with_context(|| format!("failed to append workspace Git ignore rules to {path:?}"))
}

/// Ignores blank lines and normalizes CRLF without changing nonblank line contents.
fn gitignore_lines(contents: &[u8]) -> impl DoubleEndedIterator<Item = &[u8]> {
    contents
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.trim_ascii().is_empty())
}

#[derive(Debug, Error)]
enum WorkspaceError {
    #[error(
        "server target {path:?} is a Remote workspace; this operation requires a Local Server (chained Remote workspaces are not supported)"
    )]
    RemoteTargetNotLocal { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_lease_ends_even_while_a_duplicate_file_description_is_alive() {
        let root = tempfile::tempdir().unwrap();
        let writer = Writer::acquire(root.path()).unwrap();
        // A concurrently spawned process can briefly inherit this open file description.
        let inherited = writer._lock.try_clone().unwrap();
        drop(writer);
        let successor = Writer::acquire(root.path()).expect("the previous writer lease has ended");
        assert!(
            Writer::acquire(root.path()).is_err(),
            "the successor must still own its lease"
        );
        drop(successor);
        let final_writer = Writer::acquire(root.path()).unwrap();
        drop(inherited);
        drop(final_writer);
    }
}
