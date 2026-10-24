//! Content-addressed mods cache with immutable data and atomic snapshot publication.

use std::path::{Path, PathBuf};

mod access;
pub use access::{CacheReader, CacheWriter};
mod error;
pub use error::CacheError;
mod model;
pub use model::*;

mod directory;
pub use directory::CACHE_DIRECTORY_VERSION;
mod lookup;
pub use lookup::{CacheLookup, CachedIdentification};

mod ident_info;
pub use ident_info::CacheIdentInfo;

mod content;
mod import;
mod index;
mod operations;
mod storage;

#[cfg(test)]
mod tests;

/// File-host cache selection shared by loader and source identification.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CacheSelection {
    Default,
    Directory { path: PathBuf },
}
impl CacheSelection {
    pub fn open(self) -> Result<ModsCache, CacheError> {
        match self {
            Self::Default => ModsCache::user_default(),
            Self::Directory { path } => Ok(ModsCache::new(path)),
        }
    }
}

/// A user-wide disk location, not a process-global mutable singleton.
///
/// Construction performs no I/O. Access handles do not lock the cache.
/// Managed files and directories must not be symbolic links.
#[derive(Debug)]
pub struct ModsCache {
    root: PathBuf,
    policy: directory::OpenPolicy,
}

impl ModsCache {
    /// Selects a root without creating, scanning or modifying it.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            policy: directory::OpenPolicy::Existing,
        }
    }

    /// Selects ~/.mcpm/cache/mods without touching the filesystem.
    pub fn user_default() -> Result<Self, CacheError> {
        Ok(Self {
            root: dirs::home_dir()
                .ok_or(CacheError::MissingHome)?
                .join(".mcpm/cache/mods"),
            policy: directory::OpenPolicy::InitializeEmpty,
        })
    }

    /// Initializes only an absent or empty directory; never adopts existing data.
    pub fn initialize(&self) -> Result<(), CacheError> {
        let store = self.access_store()?;
        directory::prepare(&store.root, directory::OpenPolicy::InitializeEmpty)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Opens a read handle. Observations are snapshots and may change immediately.
    /// The default root initializes when absent or empty; explicit roots must be valid.
    pub fn reader(&self) -> Result<CacheReader<'_>, CacheError> {
        let store = self.access_store()?;
        directory::prepare(&store.root, self.policy)?;
        Ok(CacheReader {
            store,
            _owner: std::marker::PhantomData,
        })
    }

    /// Opens a write-capable handle without acquiring an object or global lock.
    /// Concurrent updates may be lost; published snapshots must remain correct.
    /// A sequence of calls is not an exclusive transaction.
    pub fn writer(&self) -> Result<CacheWriter<'_>, CacheError> {
        Ok(CacheWriter {
            reader: self.reader()?,
        })
    }

    fn access_store(&self) -> Result<operations::Store, CacheError> {
        use error::IoContext;
        // Bind every operation to the selected directory, even
        // if the process later changes cwd. Symbolic links are outside the API contract.
        let root = std::path::absolute(&self.root).at("resolve cache root", &self.root)?;
        Ok(operations::Store { root })
    }
}
