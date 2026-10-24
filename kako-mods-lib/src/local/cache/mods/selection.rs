//! Transportable Mod cache location selection and the shared default-directory policy.

use super::ModsCacheDir;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModsCacheSelection {
    Default,
    Directory { path: PathBuf },
}

impl ModsCacheSelection {
    /// Resolves a location without I/O. The established default path is retained.
    pub fn path(&self) -> Result<PathBuf> {
        match self {
            Self::Default => Ok(dirs::home_dir()
                .context("cannot locate the user's home directory")?
                .join(".mcpm/cache/mods")),
            Self::Directory { path } => Ok(path.clone()),
        }
    }
    /// Defaults initialize an absent/empty root; explicit locations must be valid.
    pub fn open(self) -> Result<ModsCacheDir> {
        let path = self.path()?;
        match self {
            Self::Default => ModsCacheDir::initialize(path),
            Self::Directory { .. } => ModsCacheDir::new(path),
        }
    }
}
