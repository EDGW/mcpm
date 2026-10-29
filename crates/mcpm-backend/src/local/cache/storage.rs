//! Atomic cache publication and owned-name removal.

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use anyhow::{Context, Result};

pub(super) use crate::local::files::Content;

/// Publishes a staged name without overwriting any existing destination.
pub(super) fn publish(staged: &Path, destination: &Path) -> io::Result<()> {
    tempfile::TempPath::try_from_path(staged)?
        .persist_noclobber(destination)
        .map_err(|error| error.error)
}

pub(super) fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().expect("cache sidecar has a parent");
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to stage cache metadata {path:?}"))?;
    temp.write_all(data)
        .with_context(|| format!("failed to write staged metadata {path:?}"))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("failed to flush cache metadata {path:?}"))?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish cache metadata {path:?}"))?;
    Ok(())
}

/// Removes one owned name without recursively removing directories.
pub(super) fn remove_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to remove cache name {path:?}")),
    }
}
