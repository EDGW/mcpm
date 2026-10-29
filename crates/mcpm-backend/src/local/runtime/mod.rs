//! Shared operation resources, with lazy cache access and explicit Mod-cache disabling.

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::local::cache::mods::{ModsCacheDir, ModsCacheSelection};

mod cache;
pub use cache::RuntimeCache;

/// Resources selected once for an operation; clones share the underlying caches.
#[derive(Debug, Clone, Default)]
pub struct RuntimeEnvironment {
    pub cache: RuntimeCacheInfo,
}

impl RuntimeEnvironment {
    /// Uses the global cache for every role without accessing the filesystem.
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects the Mod cache. `None` disables it without falling back to global.
    pub fn with_mods_cache(mut self, selection: Option<ModsCacheSelection>) -> Self {
        self.cache.mods = selection.map(|selection| self.cache.select(selection));
        self
    }

    /// Selects the content cache used to create backups.
    pub fn with_backup_cache(mut self, selection: ModsCacheSelection) -> Self {
        self.cache.backup = self.cache.select(selection);
        self
    }

    /// Selects the server's stash content cache without opening or initializing it.
    pub fn with_stash_cache(mut self, selection: ModsCacheSelection) -> Self {
        self.cache.stash = self.cache.select(selection);
        self
    }

    /// Read and write operations use the same selected Mod cache.
    pub fn mods_cache(&self) -> Result<Option<&ModsCacheDir>> {
        self.cache
            .mods
            .as_ref()
            .map(|cache| cache.open())
            .transpose()
    }

    /// Explicit writes fail when no Mod cache is configured.
    pub fn require_mods_cache(&self) -> Result<&ModsCacheDir> {
        self.mods_cache()?.context("Mod cache is disabled")
    }
}

/// Cache roles default to the same global resource; only Mod caching is optional.
///
/// # Implementations
/// - [`Default`]: constructs shared global resources without filesystem access.
/// - [`Self::select`]: resolves explicit overrides against the shared global resource.
#[derive(Debug, Clone)]
pub struct RuntimeCacheInfo {
    pub global: Arc<RuntimeCache>,
    pub mods: Option<Arc<RuntimeCache>>,
    pub backup: Arc<RuntimeCache>,
    pub stash: Arc<RuntimeCache>,
}

impl RuntimeCacheInfo {
    fn select(&self, selection: ModsCacheSelection) -> Arc<RuntimeCache> {
        match selection {
            ModsCacheSelection::Default => self.global.clone(),
            selection => Arc::new(RuntimeCache::new(selection)),
        }
    }
}

impl Default for RuntimeCacheInfo {
    fn default() -> Self {
        let global = Arc::new(RuntimeCache::new(ModsCacheSelection::Default));
        Self {
            mods: Some(global.clone()),
            backup: global.clone(),
            stash: global.clone(),
            global,
        }
    }
}
