//! Per-loader cache usage reports for Mod operations using the shared runtime environment.

use crate::local::{
    cache::{hash::Blake3Hash, mods::CacheStatus},
    mcmod::ModFile,
    modloader::LoaderId,
};
use std::{collections::HashMap, path::PathBuf};

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
