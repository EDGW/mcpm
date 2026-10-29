//! Cache-first mod opening and per-loader cache usage reports.

use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use kako_mc_lib::local::{
    mcmod::{IdentificationErrorAction, ModEntry, ModFile, identify},
    modloader::{LoaderId, ModLoaderMeta},
};

use super::{CacheStatus, meta::CachedIdentification};
use crate::data::{cache::hash::Blake3Hash, runtime::RuntimeEnvironment};

/// Opens a mod entry using the Mod cache selected by the environment.
///
/// Loader iteration order is unspecified. Cached recognition, non-recognition and
/// reuse are reported per loader; missing or unavailable results are identified
/// by [`kako_mc_lib::local::mcmod::identify`] exactly as an uncached open would.
/// Cache use is read-only: opening never imports files, creates hard links, changes
/// permissions or saves metadata; use explicit cache operations for those.
/// `on_error` receives the entry, loader, attempt number and the contextual error.
/// The returned file retains the input path and enabled state.
///
/// # Errors
///
/// Returns the original contextual error when `on_error` selects
/// [`IdentificationErrorAction::Abort`], or a cache directory/lookup error.
/// No partial file is returned in those cases.
pub fn open<F>(
    entry: ModEntry,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    environment: &RuntimeEnvironment,
    mut on_error: F,
) -> Result<ModOpenReport>
where
    F: FnMut(&ModEntry, &ModLoaderMeta, usize, &anyhow::Error) -> IdentificationErrorAction,
{
    let mut metadata = Vec::new();
    let cache = environment.mods_cache()?;
    let cached = cache
        .map(|cache| cache.find_metadata(entry.path()))
        .transpose()?
        .flatten();
    tracing::debug!(path = %entry.path().display(), cache = ?cached.as_ref().map(|entry| (entry.id(), entry.status())), "Checked mod cache association");
    let mut usage = HashMap::new();

    for loader in loaders.values() {
        if let Some(cached) = &cached {
            let result = if cached.status() != &CacheStatus::Available {
                LoaderCacheUsage::Miss(LoaderCacheMissReason::Unavailable)
            } else if let Some(digest) = cached.digest() {
                match digest.identification(loader.id().as_str()) {
                    CachedIdentification::Missing => {
                        LoaderCacheUsage::Miss(LoaderCacheMissReason::Missing)
                    }
                    CachedIdentification::Unrecognized => LoaderCacheUsage::Unrecognized,
                    CachedIdentification::Recognized(meta) => match meta.to_mod_meta(&loaders) {
                        Ok(meta) => {
                            metadata.push(meta);
                            LoaderCacheUsage::Recognized
                        }
                        Err(_) => LoaderCacheUsage::Miss(LoaderCacheMissReason::InvalidMetadata),
                    },
                }
            } else {
                LoaderCacheUsage::Miss(LoaderCacheMissReason::Unavailable)
            };
            usage.insert(loader.id().clone(), result);
            if !matches!(result, LoaderCacheUsage::Miss(_)) {
                continue;
            }
        }
        metadata.extend(identify(&entry, loader, &mut on_error)?);
    }

    let cache = match cached {
        Some(cached) => ModOpenCacheReport::Matched {
            root: cache
                .map(|cache| cache.path())
                .expect("cache enabled for a match")
                .to_owned(),
            id: cached.id(),
            status: cached.status().clone(),
            loaders: usage,
        },
        None => match cache {
            None => ModOpenCacheReport::Disabled,
            Some(cache) => ModOpenCacheReport::Miss {
                root: cache.path().to_owned(),
            },
        },
    };
    Ok(ModOpenReport {
        file: ModFile { entry, metadata },
        cache,
    })
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
