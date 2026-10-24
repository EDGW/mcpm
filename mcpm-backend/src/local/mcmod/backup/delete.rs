//! Backup deletion with manifest ownership checks and directory-only permission changes.

use std::{fs, io, path::Path};

use anyhow::{Context, Result, ensure};

use super::{MANIFEST_NAME, Manifest, tree};
use crate::local::files::{DirectoryProtection, remove_protected_tree};

/// Deletes a recognized backup, including damaged data, without opening a cache.
///
/// The root must contain a valid manifest and no entries other than the manifest
/// and an optional regular data directory. Data contents need not pass verification.
/// File permissions are never changed, so other hard links retain their protection.
/// An I/O failure can leave a partially deleted backup; the manifest is removed
/// only after the data directory has been deleted.
pub fn delete(backup: &Path) -> Result<()> {
    let directory = tree::root(backup)?;
    let manifest = directory.join(MANIFEST_NAME);
    Manifest::read(&manifest).context("refusing to delete an unrecognized backup")?;
    for entry in fs::read_dir(&directory).context("failed to inspect backup directory")? {
        let entry = entry?;
        ensure!(
            entry.file_name() == MANIFEST_NAME || entry.file_name() == "data",
            "refusing to delete a backup with an unexpected top-level entry: {:?}",
            entry.path()
        );
    }
    let data = directory.join("data");
    let has_data = match fs::symlink_metadata(&data) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir(),
                "expected a regular data directory: {data:?}"
            );
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error).context("failed to inspect backup data directory"),
    };
    let protection = DirectoryProtection::allow_removal(&directory)
        .context("failed to permit removal from backup directory")?;
    if has_data {
        remove_protected_tree(&data).context("failed to delete backup data")?;
    }
    fs::remove_file(&manifest).context("failed to delete backup manifest")?;
    fs::remove_dir(&directory).context("failed to delete backup directory")?;
    protection.persist();
    Ok(())
}
