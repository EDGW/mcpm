//! Explicit file selection and opt-in directory enumeration for info/deps.

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;
use kako_mods_lib::local::mcmod::ModDirectory;
use thiserror::Error;

/// Files are literal paths; only --all requests directory enumeration.
#[derive(Args)]
pub(in crate::commands) struct FileSelectionArgs {
    /// Mod file paths; server commands also accept mods/FILE paths.
    #[arg(value_name = "FILE", num_args = 0..)]
    files: Vec<PathBuf>,
    /// Inspect every mod file in the selected directory.
    #[arg(long, conflicts_with = "files")]
    all: bool,
}

impl FileSelectionArgs {
    pub(in crate::commands) fn with_stdin(mut self) -> Result<Self> {
        self.files = super::super::stdin::append(self.files)?;
        if self.all && !self.files.is_empty() {
            return Err(AllWithFiles.into());
        }
        Ok(self)
    }

    pub(in crate::commands) fn is_empty(&self) -> bool {
        !self.all && self.files.is_empty()
    }

    /// Returns None for --all, distinct from an empty explicit selection.
    pub(in crate::commands) fn into_files(self) -> Option<Vec<PathBuf>> {
        (!self.all).then_some(self.files)
    }

    pub(in crate::commands) fn resolve(self, directory: &Path) -> Result<Vec<PathBuf>> {
        match self.into_files() {
            Some(files) => Ok(files),
            None => Ok(ModDirectory::new(directory.to_owned())?
                .entries()?
                .into_iter()
                .map(|entry| entry.path().to_owned())
                .collect()),
        }
    }
}

#[derive(Debug, Error)]
#[error("--all cannot be combined with file arguments from stdin")]
struct AllWithFiles;
