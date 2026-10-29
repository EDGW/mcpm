//! Cross-process workspace registration writer and its bundled Git ignore block.
//!
//! Readers may observe an incomplete first registration, which fails closed rather than
//! selecting another workspace kind.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{
    manifest::{self, Kind, marker_path},
    remote::RemoteRegistration,
};

const IGNORE_BLOCK: &str = include_str!("../../../config/workspace.gitignore");

/// Serializes registration writers across processes.
pub struct RegistrationWriter {
    root: PathBuf,
    _lock: File,
}

impl RegistrationWriter {
    /// Acquires the workspace lock and installs the bundled ignore block.
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
        append_gitignore(root)?;
        Ok(Self {
            root: root.to_owned(),
            _lock: lock,
        })
    }

    /// Marks a workspace Local unless a marker already exists.
    pub fn mark_local(&self) -> Result<()> {
        manifest::require_local(&self.root)?;
        if !marker_path(&self.root).exists() {
            self.write("mcpm.json", &manifest::marker(Kind::Local), false)?;
        }
        Ok(())
    }

    /// Writes a Remote registration; type conversion is deliberately excluded, even with force.
    pub fn remote(&self, registration: &RemoteRegistration, force: bool) -> Result<()> {
        registration.validate()?;
        let marker_exists = marker_path(&self.root).try_exists()?;
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
        let existing_kind = manifest::kind(&self.root)?;
        ensure!(
            !(existing_kind == Kind::Local && (marker_exists || server_exists)),
            "cannot convert a Local workspace to Remote with server init, even with --force"
        );
        ensure!(
            !server_exists || force,
            "Remote registration already exists; use --force to replace its connection settings"
        );
        self.write("server.json", registration, force)?;
        if !marker_exists && let Err(error) = self.write("mcpm.json", &manifest::marker(Kind::Remote), false) {
            // No previous registration existed, and the writer lock is still held.
            fs::remove_file(&server).context(
                "failed to remove incomplete Remote registration after mcpm.json write failure",
            )?;
            return Err(error)
                .context("failed to publish Remote workspace type; removed incomplete server.json");
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

/// Appends the bundled ignore block unless the file already ends with it, ignoring blank lines.
fn append_gitignore(root: &Path) -> Result<()> {
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
    if gitignore_lines(IGNORE_BLOCK.as_bytes())
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
    file.write_all(IGNORE_BLOCK.as_bytes())
        .with_context(|| format!("failed to append workspace Git ignore rules to {path:?}"))
}

/// Ignores blank lines and normalizes CRLF without changing nonblank line contents.
fn gitignore_lines(contents: &[u8]) -> impl DoubleEndedIterator<Item = &[u8]> {
    contents
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.trim_ascii().is_empty())
}
