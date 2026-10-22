//! Resolves user-supplied files and directories into mod entries.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use kako_mods_lib::local::mcmod::{ModDirectory, ModEntry};
use thiserror::Error;

#[derive(Debug, Error)]
enum InputError {
    #[error("list input {path:?} is neither a regular file nor a directory")]
    UnsupportedFileType { path: PathBuf },
}

#[tracing::instrument(level = "debug", fields(path = %path.display()), skip_all, err(level = "debug"))]
pub(super) fn read(path: PathBuf) -> Result<Vec<ModEntry>> {
    tracing::debug!("Resolving mod list input");
    let metadata = fs::metadata(&path)?;
    if metadata.is_dir() {
        let entries = ModDirectory::new(path)?.entries()?;
        tracing::debug!(count = entries.len(), "Enumerated mod directory");
        Ok(entries)
    } else if metadata.is_file() {
        let entry = ModEntry::new(path)?;
        tracing::debug!("Resolved individual mod file");
        Ok(vec![entry])
    } else {
        Err(InputError::UnsupportedFileType { path }.into())
    }
}
