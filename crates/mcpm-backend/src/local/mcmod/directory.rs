use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;

use super::ModEntry;

/// A directory containing enabled and disabled mod files.
///
/// Stores the supplied path and scans the filesystem on each call to
/// [`Self::entries`]. Changes to the directory are visible on subsequent scans.
#[derive(Debug, Clone)]
pub struct ModDirectory {
    path: PathBuf,
}

impl ModDirectory {
    /// Opens an existing directory without loading its mod entries.
    ///
    /// Relative paths are retained and resolved against the process's current
    /// working directory when accessed. Symbolic links to directories are allowed.
    ///
    /// # Errors
    ///
    /// Returns an [`anyhow::Error`] if metadata cannot be read or the path does
    /// not point to a directory. No directory is created.
    pub fn new(path: PathBuf) -> Result<Self> {
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to read mod directory metadata for {path:?}"))?;
        if !metadata.is_dir() {
            return Err(ModDirectoryError::NotDirectory { path }.into());
        }

        Ok(Self { path })
    }

    /// Returns the directory path as supplied to [`Self::new`].
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Scans the directory and returns mod entries sorted by path.
    ///
    /// Only files with case-sensitive `.jar` or `.jar.disabled` suffixes are
    /// included. Symbolic links to regular files are included. Other filenames
    /// and subdirectories are ignored; scanning is not recursive.
    ///
    /// The result is collected afresh and is not an atomic filesystem snapshot.
    ///
    /// # Errors
    ///
    /// Returns an [`anyhow::Error`] if the directory cannot be read, an entry
    /// cannot be read, a matching path's metadata cannot be read (including a
    /// broken symbolic link), or a matching file's name is not valid UTF-8.
    /// No partial list is returned on failure.
    pub fn entries(&self) -> Result<Vec<ModEntry>> {
        let directory = fs::read_dir(&self.path)
            .with_context(|| format!("failed to read mod directory {:?}", self.path))?;
        let mut entries = Vec::new();

        for entry in directory {
            let entry = entry.with_context(|| {
                format!("failed to read entry in mod directory {:?}", self.path)
            })?;
            let filename = entry.file_name();
            let filename_bytes = filename.as_encoded_bytes();
            if !filename_bytes.ends_with(b".jar") && !filename_bytes.ends_with(b".jar.disabled") {
                continue;
            }

            let path = entry.path();
            let metadata = fs::metadata(&path)
                .with_context(|| format!("failed to read mod file metadata for {path:?}"))?;
            if metadata.is_file() {
                entries.push(ModEntry::new(path)?);
            }
        }

        entries.sort_by(|left, right| left.path().cmp(right.path()));
        Ok(entries)
    }
}

/// A validation error encountered while opening a [`ModDirectory`].
#[derive(Debug, Error)]
pub enum ModDirectoryError {
    /// The supplied path exists but does not point to a directory.
    #[error("mod directory path {path:?} is not a directory")]
    NotDirectory {
        /// The path that failed validation.
        path: PathBuf,
    },
}
