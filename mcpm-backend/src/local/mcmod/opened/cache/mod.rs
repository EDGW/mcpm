//! Cache selection, reusable read-only Mod context and per-loader usage reports.

pub use crate::local::cache::mods::ModsCacheSelection as ModOpenCache;
use crate::local::{
    cache::{
        hash::Blake3Hash,
        mods::{CacheStatus, ModsCacheDir, meta::CacheEntry},
    },
    mcmod::ModFile,
    modloader::LoaderId,
};
use anyhow::Result;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Default)]
pub struct ModOpenContext {
    cache: Option<ModsCacheDir>,
}
impl ModOpenContext {
    pub fn new(selection: Option<ModOpenCache>) -> Result<Self> {
        Ok(Self {
            cache: selection.map(ModOpenCache::open).transpose()?,
        })
    }
    pub(super) fn find(&self, path: &Path) -> Result<Option<CacheEntry>> {
        let Some(cache) = &self.cache else {
            return Ok(None);
        };
        cache.find_metadata(path)
    }
    pub(super) fn root(&self) -> Option<&Path> {
        self.cache.as_ref().map(ModsCacheDir::path)
    }
    pub(super) fn report(&self) -> ModOpenCacheReport {
        match self.root() {
            None => ModOpenCacheReport::Disabled,
            Some(root) => ModOpenCacheReport::Miss {
                root: root.to_owned(),
            },
        }
    }
}

/// Runtime metadata plus a description of cache reuse; owns no cache lock.
#[derive(Debug)]
pub struct ModOpenReport {
    pub file: ModFile,
    pub cache: ModOpenCacheReport,
}

#[derive(Debug)]
pub enum ModOpenCacheReport {
    Disabled,
    Miss {
        root: PathBuf,
    },
    Matched {
        root: PathBuf,
        id: Blake3Hash,
        status: CacheStatus,
        loaders: HashMap<LoaderId, LoaderCacheUsage>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderCacheUsage {
    Recognized,
    Unrecognized,
    Miss(LoaderCacheMissReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderCacheMissReason {
    Missing,
    Unavailable,
    InvalidMetadata,
}

/// File-level reuse of the requested loader results, including non-recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModCacheReuse {
    None,
    Partial,
    Full,
}

impl ModOpenCacheReport {
    pub fn reuse(&self) -> ModCacheReuse {
        let Self::Matched { loaders, .. } = self else {
            return ModCacheReuse::None;
        };
        let reused = loaders
            .values()
            .filter(|usage| !matches!(usage, LoaderCacheUsage::Miss(_)))
            .count();
        if reused == 0 {
            ModCacheReuse::None
        } else if reused == loaders.len() {
            ModCacheReuse::Full
        } else {
            ModCacheReuse::Partial
        }
    }
}
