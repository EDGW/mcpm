//! A shared cache selection opened lazily through the cache owner's directory policy.

use std::sync::OnceLock;

use anyhow::Result;

use crate::local::cache::mods::{ModsCacheDir, ModsCacheSelection};

/// A successful open is retained; failures remain contextual errors and may be retried.
#[derive(Debug)]
pub struct RuntimeCache {
    selection: ModsCacheSelection,
    directory: OnceLock<ModsCacheDir>,
}

impl RuntimeCache {
    pub fn new(selection: ModsCacheSelection) -> Self {
        Self {
            selection,
            directory: OnceLock::new(),
        }
    }

    pub fn selection(&self) -> &ModsCacheSelection {
        &self.selection
    }

    pub fn open(&self) -> Result<&ModsCacheDir> {
        if let Some(directory) = self.directory.get() {
            return Ok(directory);
        }
        // Concurrent first opens are safe: cache initialization coordinates writers.
        // Only a successful directory becomes the shared resource.
        let directory = self.selection.clone().open()?;
        Ok(self.directory.get_or_init(|| directory))
    }
}
