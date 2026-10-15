use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use thiserror::Error;

use super::{
    AssetsDirectory, CheckOptions, CheckReport, CheckRules, LibrariesDirectory, MinecraftVersion,
};

/// A Minecraft root containing a `versions` directory.
#[derive(Debug, Clone)]
pub struct MinecraftDirectory {
    path: PathBuf,
}

impl MinecraftDirectory {
    /// Opens an existing root without creating files or requiring `versions` to exist.
    /// Paths are retained as supplied; `~` is not expanded.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if !fs::metadata(&path)
            .with_context(|| format!("failed to inspect {path:?}"))?
            .is_dir()
        {
            return Err(MinecraftDirectoryError::NotDirectory { path }.into());
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Shared caches are represented even when their directories do not exist yet.
    pub fn assets(&self) -> AssetsDirectory {
        AssetsDirectory::new(self.path.join("assets"))
    }

    pub fn libraries(&self) -> LibrariesDirectory {
        LibrariesDirectory::new(self.path.join("libraries"))
    }

    /// Checks selected resources using the caller's platform and feature rules.
    /// All failures are collected in the report; no files are changed/downloaded.
    /// The version's metadata is its loaded snapshot; resource files are read afresh.
    /// Libraries include applicable primary artifacts and selected native archives.
    /// Assets include the index and content-addressed objects, not generated virtual
    /// or resources copies. Client JAR checking is configurable; Java runtime,
    /// mods, and game launchability remain outside its scope.
    pub fn check(
        &self,
        version: &MinecraftVersion,
        options: CheckOptions,
        rules: &CheckRules,
    ) -> CheckReport {
        self.check_with_progress(version, options, rules, |_| {})
    }

    /// Runs the same check while synchronously reporting steps, plans, completed
    /// and pending files. Each file emits only its completion event; reads and
    /// hashing do not emit intermediate callbacks. Snapshots borrow
    /// internal state and must be copied by observers that need to retain data.
    /// Asset object totals remain unknown until the index is verified and parsed.
    pub fn check_with_progress(
        &self,
        version: &MinecraftVersion,
        options: CheckOptions,
        rules: &CheckRules,
        observer: impl FnMut(super::CheckProgress<'_>),
    ) -> CheckReport {
        super::check::check(self, version, options, rules, observer)
    }

    /// Loads every subdirectory of `versions`, sorted by directory name.
    /// An absent `versions` directory is empty; ordinary files are ignored.
    /// Missing/invalid JSON and inheritance failures yield `Broken` entries,
    /// without preventing other profiles from loading. Each entry is a snapshot.
    /// Directory enumeration/metadata errors and non-UTF-8 names fail the scan.
    pub fn versions(&self) -> Result<Vec<MinecraftVersion>> {
        let path = self.path.join("versions");
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).with_context(|| format!("failed to read {path:?}")),
        };
        let mut versions = Vec::new();
        for entry in entries {
            let entry = entry.with_context(|| format!("failed to read entry in {path:?}"))?;
            let directory = entry.path();
            if !fs::metadata(&directory)
                .with_context(|| format!("failed to inspect {directory:?}"))?
                .is_dir()
            {
                continue;
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|name| MinecraftDirectoryError::InvalidVersionName { name })?;
            versions.push(self.version(name)?);
        }
        versions.sort_by(|a, b| a.name().cmp(b.name()));
        Ok(versions)
    }

    /// Loads one profile, capturing load failures in `MinecraftVersion::Broken`.
    /// Invalid names are returned as errors because they cannot refer to a profile
    /// inside this root. Calling this again reloads the files from disk.
    pub fn version(&self, name: impl Into<String>) -> Result<MinecraftVersion> {
        Ok(MinecraftVersion::load(self, name.into())?)
    }
}

#[derive(Debug, Error)]
pub enum MinecraftDirectoryError {
    #[error("Minecraft root {path:?} is not a directory")]
    NotDirectory { path: PathBuf },
    #[error("Minecraft version directory name {name:?} is not valid UTF-8")]
    InvalidVersionName { name: std::ffi::OsString },
}
