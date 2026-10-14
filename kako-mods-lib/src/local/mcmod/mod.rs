//! Local mod file entries, filename validation, and directory scanning.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;

mod directory;

pub use directory::*;

mod opened;

pub use opened::*;

mod dependency;
mod dependency_constraint;
mod version_constraint;
mod version_operator;

pub use dependency::ModDependency;
pub use dependency_constraint::DependencyConstraint;
pub use version_constraint::VersionConstraint;
pub use version_operator::VersionOperator;

/// A mod file's path, name, and enabled state.
#[derive(Debug)]
pub struct ModEntry {
    path: PathBuf,
    name: ModEntryName,
}

impl ModEntry {
    /// Creates a mod entry by parsing the filename in the supplied path.
    ///
    /// This does not check whether the file exists.
    ///
    /// # Errors
    ///
    /// Returns an [`anyhow::Error`] if the path has no filename, the filename is
    /// not valid UTF-8, or it does not end in `.jar` or `.jar.disabled`.
    pub fn new(path: PathBuf) -> Result<Self> {
        let filename = path
            .file_name()
            .ok_or_else(|| ModEntryError::MissingFilename { path: path.clone() })?
            .to_str()
            .ok_or_else(|| ModEntryError::InvalidFilenameEncoding { path: path.clone() })?;
        let name = ModEntryName::new(filename)
            .with_context(|| format!("failed to parse mod filename in path {path:?}"))?;

        Ok(Self { path, name })
    }

    /// Returns the mod file's path as supplied to [`Self::new`].
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the filename with its `.jar` or `.jar.disabled` suffix removed.
    pub fn name(&self) -> &str {
        self.name.name()
    }

    /// Returns whether the mod is enabled, as indicated by its filename suffix.
    pub fn status(&self) -> ModStatus {
        self.name.status()
    }
}

/// A path parsing error encountered while constructing a [`ModEntry`].
#[derive(Debug, Error)]
pub enum ModEntryError {
    /// The path does not contain a filename.
    #[error("mod path {path:?} has no filename")]
    MissingFilename {
        /// The path that failed validation.
        path: PathBuf,
    },
    /// The filename cannot be represented as UTF-8.
    #[error("mod path {path:?} has a filename that is not valid UTF-8")]
    InvalidFilenameEncoding {
        /// The path that failed validation.
        path: PathBuf,
    },
}

/// The name and enabled state encoded in a mod file's filename.
///
/// Mod filenames are expected to end in `.jar` for enabled mods or
/// `.jar.disabled` for disabled mods. Construction validates the suffix and
/// stores the complete filename.
#[derive(Debug)]
pub struct ModEntryName {
    filename: String,
}

impl ModEntryName {
    /// Creates a mod entry name from a complete filename.
    ///
    /// # Errors
    ///
    /// Returns an [`anyhow::Error`] wrapping
    /// [`ModEntryNameError::InvalidFilename`] if the filename does not end in
    /// `.jar` or `.jar.disabled`. Suffix matching is case-sensitive.
    pub fn new(filename: impl Into<String>) -> Result<Self> {
        let filename = filename.into();
        if !filename.ends_with(".jar") && !filename.ends_with(".jar.disabled") {
            return Err(ModEntryNameError::InvalidFilename { filename }.into());
        }

        Ok(Self { filename })
    }

    /// Returns the mod name with the `.jar` or `.jar.disabled` suffix removed.
    ///
    /// The returned string borrows from the stored filename.
    pub fn name(&self) -> &str {
        if let Some(name) = self.filename.strip_suffix(".jar.disabled") {
            name
        } else if let Some(name) = self.filename.strip_suffix(".jar") {
            name
        } else {
            unreachable!("mod entry filename must end with .jar or .jar.disabled")
        }
    }

    /// Returns the mod's enabled state based on its filename suffix.
    ///
    /// Returns [`ModStatus::Enabled`] for `.jar` and [`ModStatus::Disabled`]
    /// for `.jar.disabled`.
    pub fn status(&self) -> ModStatus {
        if self.filename.ends_with(".disabled") {
            ModStatus::Disabled
        } else if self.filename.ends_with(".jar") {
            ModStatus::Enabled
        } else {
            unreachable!("mod entry filename must end with .jar or .jar.disabled")
        }
    }
}

/// An error encountered while constructing a [`ModEntryName`].
#[derive(Debug, Error)]
pub enum ModEntryNameError {
    /// The filename does not end in `.jar` or `.jar.disabled`.
    #[error("invalid mod filename {filename:?}: expected .jar or .jar.disabled suffix")]
    InvalidFilename {
        /// The filename that failed validation.
        filename: String,
    },
}

/// Whether a mod is enabled or disabled, as indicated by its filename.
pub enum ModStatus {
    /// The mod filename ends in `.jar`.
    Enabled,
    /// The mod filename ends in `.jar.disabled`.
    Disabled,
}
