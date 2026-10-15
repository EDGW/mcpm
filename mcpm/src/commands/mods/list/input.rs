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

pub(super) fn read(path: PathBuf) -> Result<Vec<ModEntry>> {
    let metadata = fs::metadata(&path)?;
    if metadata.is_dir() {
        ModDirectory::new(path)?.entries()
    } else if metadata.is_file() {
        Ok(vec![ModEntry::new(path)?])
    } else {
        Err(InputError::UnsupportedFileType { path }.into())
    }
}
