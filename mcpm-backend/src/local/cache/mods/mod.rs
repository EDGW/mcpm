//! Mod cache policy over a private generic content cache and the named meta/ident snapshots.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{
    CacheDir, CacheInspection, CacheLinkPreference, CacheScanEntry, ImportOutcome, TransferMethod,
    VerificationMode, hash::Blake3Hash,
};

pub mod ident;
pub mod meta;

mod selection;
pub use selection::ModsCacheSelection;
mod status;
pub use status::{CacheStatus, CacheUnavailableReason, MetadataRefresh};

/// Owns Mod-specific metadata interpretation; the underlying cache remains an implementation detail.
#[derive(Debug, Clone)]
pub struct ModsCacheDir {
    cache: CacheDir,
}

impl ModsCacheDir {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            cache: CacheDir::new(path)?,
        })
    }

    pub fn initialize(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            cache: CacheDir::initialize(path)?,
        })
    }

    pub fn path(&self) -> &Path {
        self.cache.path()
    }

    pub fn metadata_path(&self) -> PathBuf {
        self.cache.metadata_path()
    }

    pub fn content_path(&self, key: &Blake3Hash) -> PathBuf {
        self.cache.content_path(key)
    }

    /// Reads inventory only; it neither decodes nor validates named metadata.
    pub fn inspect(&self, key: &Blake3Hash) -> Result<CacheInspection> {
        self.cache.inspect(key)
    }

    pub fn list(&self) -> Result<Vec<Blake3Hash>> {
        self.cache.list()
    }

    pub fn scan(&self) -> Result<Vec<CacheScanEntry>> {
        self.cache.scan()
    }

    /// Imports bytes without creating meta or ident metadata.
    pub fn import(&self, path: &Path, mode: CacheLinkPreference) -> Result<Blake3Hash> {
        self.cache.import(path, mode)
    }

    pub fn import_with_outcome(
        &self,
        path: &Path,
        mode: CacheLinkPreference,
    ) -> Result<ImportOutcome> {
        self.cache.import_with_outcome(path, mode)
    }

    pub fn export_file(
        &self,
        key: &Blake3Hash,
        path: &Path,
        mode: CacheLinkPreference,
    ) -> Result<TransferMethod> {
        self.cache.export_file(key, path, mode)
    }

    pub fn remove(&self, key: &Blake3Hash) -> Result<bool> {
        self.cache.remove(key)
    }

    pub fn rebuild_index(&self) -> Result<usize> {
        self.cache.rebuild_index()
    }

    /// Verifies only content structure or its BLAKE3 identity; metadata is never opened.
    pub fn verify(&self, key: &Blake3Hash, mode: VerificationMode) -> Result<Option<()>> {
        Ok(self.cache.open_content_with_mode(key, mode)?.map(|_| ()))
    }
}
