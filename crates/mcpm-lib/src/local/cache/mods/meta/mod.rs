//! Loader metadata policy over the generic content cache; no filesystem storage implementation.

use crate::local::{
    cache::{
        CacheDir, VerificationMode,
        hash::Blake3Hash,
        mods::{CacheStatus, MetadataRefresh},
    },
    mcmod::CachedModMeta,
    modloader::{LoaderId, ModLoaderMeta},
};
use anyhow::Result;
use std::collections::HashMap;

mod codec;
pub use codec::METANAME as NAME;
pub use codec::{CACHE_FORMAT_VERSION, CacheDigest, MOD_METADATA_VERSION};
mod observation;
pub use observation::CacheEntry;
use observation::from_content;
mod refresh;
#[cfg(test)]
use refresh::write_metadata;

/// Loader results are distinct from metadata-file presence and JSON representation.
#[derive(Debug)]
pub enum CachedIdentification<'a> {
    Missing,
    Unrecognized,
    Recognized(&'a CachedModMeta),
}

/// Loader metadata belongs to this Mod cache, never to the generic CacheDir API.
impl super::ModsCacheDir {
    pub fn mod_metadata(&self, key: Blake3Hash) -> CacheEntry {
        observation::read(&self.cache, key)
    }
    /// Observes Loader metadata, preserving unexpected I/O errors for diagnostics.
    /// Missing or invalid snapshots remain ordinary availability states.
    pub fn try_mod_metadata(&self, key: Blake3Hash) -> Result<CacheEntry> {
        observation::try_read(&self.cache, key)
    }
    pub fn list_metadata(&self) -> Result<Vec<CacheEntry>> {
        Ok(self
            .cache
            .scan()?
            .into_iter()
            .map(|row| observation::from_inspection(&self.cache, row.key, row.inspection))
            .collect())
    }
    pub fn verify_metadata(&self, key: Blake3Hash, mode: VerificationMode) -> CacheStatus {
        observation::verify(&self.cache, key, mode)
    }
    pub fn refresh_metadata(
        &self,
        key: Blake3Hash,
        loaders: &HashMap<LoaderId, ModLoaderMeta>,
        mode: MetadataRefresh,
    ) -> Result<CacheEntry> {
        refresh::refresh(&self.cache, key, loaders, mode)
    }
    /// Looks up Loader metadata without importing or publishing anything.
    pub fn find_metadata(&self, path: &std::path::Path) -> Result<Option<CacheEntry>> {
        find(&self.cache, path)
    }
}

#[cfg(test)]
mod tests;

/// Reads a matching Mod snapshot through generic lookup; corrupt cached content
/// becomes an unavailable observation so normal Loader identification can proceed.
pub(super) fn find(cache: &CacheDir, path: &std::path::Path) -> Result<Option<CacheEntry>> {
    match cache.find_content(path) {
        Ok(content) => Ok(content.map(|content| from_content(cache, &content))),
        Err(error) => {
            let key = error
                .downcast_ref::<crate::local::cache::CachedContentLookupError>()
                .map(|error| error.key);
            match key {
                Some(key) => Ok(Some(observation::from_inspection(cache, key, Err(error)))),
                None => Err(error),
            }
        }
    }
}
