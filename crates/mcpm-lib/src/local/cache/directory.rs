//! Cache root validation and serialized initialization through the manifest codec.

use super::{codec::CacheDirMetadata, layout::MANIFEST, storage};
use crate::local::codec::Codec;
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(super) fn open(path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve cache directory {path:?}"))?;
    ensure!(root.is_dir(), "expected a cache directory: {root:?}");
    let directory = root.join(".mcpm");
    let metadata = fs::symlink_metadata(&directory)
        .with_context(|| format!("failed to inspect cache metadata directory {directory:?}"))?;
    ensure!(
        metadata.is_dir(),
        "expected a metadata directory without symbolic links: {directory:?}"
    );
    let manifest = root.join(MANIFEST);
    let metadata = fs::symlink_metadata(&manifest)
        .with_context(|| format!("failed to inspect cache manifest {manifest:?}"))?;
    ensure!(
        metadata.is_file(),
        "expected a regular cache manifest: {manifest:?}"
    );
    CacheDirMetadata::open(&manifest).context("failed to open cache directory metadata")?;
    Ok(root)
}

pub(super) fn initialize(path: &Path) -> Result<PathBuf> {
    let root = std::path::absolute(path)
        .with_context(|| format!("failed to resolve cache initialization path {path:?}"))?;
    // Existing manifests are validated without creating coordination files.
    match fs::symlink_metadata(root.join(MANIFEST)) {
        Ok(_) => return open(&root),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to inspect cache manifest"),
    }
    let parent = root
        .parent()
        .filter(|_| root.file_name().is_some())
        .context("filesystem root cannot be initialized as a cache")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create cache parent {parent:?}"))?;
    let lock_path = parent.join(".mcpm-cache-init.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open cache initialization lock {lock_path:?}"))?;
    lock.lock()
        .with_context(|| format!("failed to lock cache initialization {lock_path:?}"))?;
    match fs::symlink_metadata(root.join(MANIFEST)) {
        Ok(_) => return open(&root),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to inspect cache manifest"),
    }
    match fs::symlink_metadata(&root) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir(),
                "expected an initialization directory without symbolic links: {root:?}"
            );
            ensure!(
                fs::read_dir(&root)
                    .with_context(|| format!("failed to inspect cache directory {root:?}"))?
                    .next()
                    .transpose()
                    .with_context(|| format!("failed to inspect cache directory entry {root:?}"))?
                    .is_none(),
                "nonempty directory has no valid cache manifest: {root:?}"
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&root)
                .with_context(|| format!("failed to create cache directory {root:?}"))?;
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect cache directory {root:?}"));
        }
    }
    let metadata_dir = root.join(".mcpm");
    fs::create_dir(&metadata_dir)
        .with_context(|| format!("failed to create cache metadata directory {metadata_dir:?}"))?;
    let manifest = root.join(MANIFEST);
    let bytes = CacheDirMetadata::new().encode()?;
    let mut temp =
        tempfile::NamedTempFile::new_in(&metadata_dir).context("failed to stage cache manifest")?;
    temp.write_all(&bytes)
        .context("failed to write cache manifest")?;
    temp.as_file()
        .sync_all()
        .context("failed to flush cache manifest")?;
    let staged = temp.into_temp_path();
    storage::publish(&staged, &manifest)
        .with_context(|| format!("failed to publish cache manifest {manifest:?}"))?;
    open(&root)
}
