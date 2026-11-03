//! Pinned regular-file readers, hashing and copies with change detection.

use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read, Seek},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::FileIdentity;
use super::hash::{self, Blake3Hash, ImoHash};

/// Keeps native identity alive while content is verified and published.
#[derive(Debug)]
pub(crate) struct Content {
    file: File,
    path: PathBuf,
    identity: FileIdentity,
    observed: Metadata,
}

impl Content {
    pub fn open(path: &Path) -> Result<Option<Self>> {
        Self::open_inner(path, true)
    }

    /// Opens one metadata snapshot. An atomic replacement of its pathname does
    /// not invalidate a complete pinned inode that still matches its observation.
    pub fn open_snapshot(path: &Path) -> Result<Option<Self>> {
        Self::open_inner(path, false)
    }

    fn open_inner(path: &Path, validate_name: bool) -> Result<Option<Self>> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("failed to inspect content {path:?}"));
            }
            Ok(metadata) => ensure!(metadata.is_file(), "expected a regular file: {path:?}"),
        }
        let file = File::open(path).with_context(|| format!("failed to open content {path:?}"))?;
        let observed = file
            .metadata()
            .with_context(|| format!("failed to inspect content handle {path:?}"))?;
        ensure!(observed.is_file(), "expected a regular file: {path:?}");
        let identity = FileIdentity::from_file(&file)
            .with_context(|| format!("failed to identify content {path:?}"))?;
        let content = Self {
            file,
            path: path.to_owned(),
            identity,
            observed,
        };
        if validate_name {
            content.validate()?;
        } else {
            content.validate_handle()?;
        }
        Ok(Some(content))
    }

    pub fn hash(&mut self) -> Result<Blake3Hash> {
        self.file
            .rewind()
            .with_context(|| format!("failed to rewind content {:?}", self.path))?;
        let hash = hash::blake3_reader(&mut self.file)
            .with_context(|| format!("failed to hash content {:?}", self.path))?;
        self.validate()?;
        self.file
            .rewind()
            .with_context(|| format!("failed to rewind verified content {:?}", self.path))?;
        Ok(hash)
    }

    pub fn quick_hash(&mut self) -> Result<ImoHash> {
        let hash = hash::imohash_reader(&mut self.file)
            .with_context(|| format!("failed to sample content {:?}", self.path))?;
        self.validate()?;
        Ok(hash)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn file(&self) -> &File {
        &self.file
    }
    pub fn size(&self) -> u64 {
        self.observed.len()
    }
    pub fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub fn copy_to(&mut self, destination: &Path) -> Result<()> {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .with_context(|| format!("failed to create staged copy {destination:?}"))?;
        self.file
            .rewind()
            .with_context(|| format!("failed to rewind source {:?}", self.path))?;
        io::copy(&mut self.file, &mut output)
            .with_context(|| format!("failed to copy {:?} to {destination:?}", self.path))?;
        output
            .sync_all()
            .with_context(|| format!("failed to flush staged copy {destination:?}"))?;
        self.validate()
    }

    pub fn read(&mut self) -> Result<Vec<u8>> {
        self.file
            .rewind()
            .with_context(|| format!("failed to rewind metadata {:?}", self.path))?;
        let mut bytes = Vec::new();
        self.file
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read metadata {:?}", self.path))?;
        // A complete old snapshot remains valid after atomic replacement of its name.
        self.validate_handle()?;
        Ok(bytes)
    }

    fn validate_handle(&self) -> Result<()> {
        let current = self
            .file
            .metadata()
            .with_context(|| format!("failed to inspect content handle {:?}", self.path))?;
        let modified = current.modified().with_context(|| {
            format!(
                "failed to inspect content modification time {:?}",
                self.path
            )
        })?;
        let original = self.observed.modified().with_context(|| {
            format!(
                "failed to inspect original modification time {:?}",
                self.path
            )
        })?;
        ensure!(
            current.len() == self.observed.len() && modified == original,
            "file changed during operation: {:?}",
            self.path
        );
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_handle()?;
        let named = fs::symlink_metadata(&self.path)
            .with_context(|| format!("failed to inspect content name {:?}", self.path))?;
        let identity = FileIdentity::read(&self.path)
            .with_context(|| format!("failed to validate content identity {:?}", self.path))?;
        ensure!(
            named.is_file() && identity == self.identity,
            "file changed during operation: {:?}",
            self.path
        );
        Ok(())
    }
}
