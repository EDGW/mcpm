//! Read-only lookup using imohash candidates, pinned identities and BLAKE3 fallback.

use std::{fs, path::Path};

use super::{
    CacheEntry, CacheError, CacheStatus, ModsCache, content::ContentFile, error::IoContext, index,
};
use crate::local::mcmod::CachedModMeta;

/// The three identification states, independent of persisted JSON representation.
#[derive(Debug)]
pub enum CachedIdentification<'a> {
    Missing,
    Unrecognized,
    Recognized(&'a CachedModMeta),
}

/// Reusable cache selection. No full-cache scan, copied metadata or persistent identities.
/// Queries never import files, alter permissions or publish metadata/index updates.
#[derive(Debug)]
pub struct CacheLookup {
    cache: ModsCache,
}

impl CacheLookup {
    pub fn new(cache: ModsCache) -> Result<Self, CacheError> {
        let reader = cache.reader()?;
        let root = fs::canonicalize(&reader.store.root)
            .at("resolve cache directory", &reader.store.root)?;
        Ok(Self {
            cache: ModsCache::new(root),
        })
    }

    pub fn root(&self) -> &Path {
        self.cache.root()
    }

    pub fn find(&self, path: &Path) -> Result<Option<CacheEntry>, CacheError> {
        let reader = self.cache.reader()?;
        let mut source = ContentFile::open(path)?;
        let resolved = fs::canonicalize(path).at("resolve mod file", path)?;
        let direct = (resolved.extension() == Some(std::ffi::OsStr::new("dat")))
            .then(|| reader.store.id_from_path(&resolved))
            .flatten();
        let quick = source.quick_hash()?;
        for id in direct
            .into_iter()
            .chain(index::candidates(self.root(), &quick))
        {
            let entry = reader.get(id);
            // Pin both objects while comparing so deleted identities cannot be reused.
            let Ok(candidate) = ContentFile::open(entry.data_path()) else {
                continue;
            };
            if candidate.identity == source.identity && candidate.validate().is_ok() {
                source.validate()?;
                return Ok(Some(entry));
            }
        }
        let id = source.id()?;
        let entry = reader.get(id);
        source.validate()?;
        Ok((entry.status() != &CacheStatus::NotFound).then_some(entry))
    }
}
