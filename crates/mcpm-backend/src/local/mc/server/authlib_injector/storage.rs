//! Server-owned agent paths, short-lived coordination leases and verified pair publication.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::{
    Unavailable,
    metadata::{Metadata, UnsupportedFormat},
};
use crate::{download::ChecksumMismatch, local::codec::Codec};

/// Fixed workspace locations; no global cache or working-directory inference.
pub(super) struct Storage {
    directory: PathBuf,
}

impl Storage {
    pub fn new(root: &Path) -> Self {
        Self {
            directory: root.join(".mcpm/authlib-injector"),
        }
    }

    pub fn jar(&self) -> PathBuf {
        self.directory.join("authlib-injector.jar")
    }

    fn metadata(&self) -> PathBuf {
        self.directory.join("meta.json")
    }

    pub fn lock(&self) -> Result<File> {
        fs::create_dir_all(&self.directory)
            .context("failed to create authlib-injector directory")?;
        let path = self.directory.join("lock");
        regular_file(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open authlib-injector lock {path:?}"))?;
        // The feature is small: one exclusive preparation lease avoids upgrade races.
        file.try_lock()
            .context("authlib-injector is busy or cannot be locked")?;
        Ok(file)
    }

    pub fn inspect(&self) -> Result<std::result::Result<Metadata, Unavailable>> {
        let jar = self.jar();
        let meta = self.metadata();
        // Validate both types even when one file is missing; do not overwrite directories.
        let jar_exists = regular_file(&jar)?;
        let meta_exists = regular_file(&meta)?;
        if !meta_exists {
            return Ok(Err(Unavailable::MissingMetadata));
        }
        let bytes = fs::read(&meta).with_context(|| format!("failed to read {meta:?}"))?;
        let metadata = match Metadata::decode(&bytes) {
            Ok(value) => value,
            Err(error) if error.is::<UnsupportedFormat>() => {
                return Err(error).with_context(|| format!("cannot use {meta:?}"));
            }
            Err(error) => return Ok(Err(Unavailable::InvalidMetadata(error))),
        };
        if !jar_exists {
            return Ok(Err(Unavailable::MissingJar));
        }
        let mut file = File::open(&jar).with_context(|| format!("failed to open {jar:?}"))?;
        match metadata.checksum()?.verify_reader(&mut file) {
            Ok(()) => Ok(Ok(metadata)),
            Err(error) if error.is::<ChecksumMismatch>() => Ok(Err(Unavailable::Checksum(error))),
            Err(error) => Err(error).with_context(|| format!("failed to verify {jar:?}")),
        }
    }

    pub fn staging(&self) -> Result<tempfile::TempDir> {
        tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(&self.directory)
            .context("failed to stage authlib-injector installation")
    }

    pub fn publish(&self, staged: &Path, metadata: &Metadata) -> Result<()> {
        let mut meta = tempfile::NamedTempFile::new_in(&self.directory)?;
        meta.write_all(&metadata.encode()?)?;
        meta.as_file().sync_all()?;
        regular_file(&self.jar())?;
        regular_file(&self.metadata())?;
        // tempfile persist supplies replace semantics on both Unix and Windows.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(staged)?
            .sync_all()?;
        tempfile::TempPath::try_from_path(staged)?
            .persist(self.jar())
            .map_err(|error| error.error)
            .context("failed to publish authlib-injector JAR")?;
        meta.persist(self.metadata()).map_err(|error| error.error).context("authlib-injector JAR was published, but publishing metadata failed; next launch must recheck the pair")?;
        Ok(())
    }
}

fn regular_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file(),
                "authlib-injector path is not a regular file: {path:?}"
            );
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {path:?}")),
    }
}
