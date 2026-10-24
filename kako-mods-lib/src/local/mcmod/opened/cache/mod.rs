//! Cache selection, reusable open context and per-loader cache usage reports.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::local::{
    mcmod::{
        ModFile,
        cache::{CacheEntry, CacheError, CacheId, CacheLookup, CacheStatus, ModsCache},
    },
    modloader::LoaderId,
};

pub use crate::local::mcmod::cache::CacheSelection as ModOpenCache;

/// Read-only cache lookup context, safe to share across parallel opens.
/// Disabled contexts perform no cache filesystem access. Enabled contexts validate
/// the selected root without scanning every cached object.
#[derive(Debug, Default)]
pub struct ModOpenContext {
    lookup: Option<CacheLookup>,
}

impl ModOpenContext {
    pub fn new(selection: Option<ModOpenCache>) -> Result<Self, CacheError> {
        let cache = match selection {
            None => return Ok(Self::default()),
            Some(ModOpenCache::Default) => ModsCache::user_default()?,
            Some(ModOpenCache::Directory { path }) => ModsCache::new(path),
        };
        Ok(Self {
            lookup: Some(CacheLookup::new(cache)?),
        })
    }

    pub(super) fn find(&self, path: &Path) -> Result<Option<CacheEntry>, CacheError> {
        match &self.lookup {
            None => Ok(None),
            Some(lookup) => lookup.find(path),
        }
    }

    pub(super) fn root(&self) -> Option<&Path> {
        self.lookup.as_ref().map(CacheLookup::root)
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
        id: CacheId,
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
