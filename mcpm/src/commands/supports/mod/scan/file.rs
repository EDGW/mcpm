//! Adapts library identification reports to command-local scan results and diagnostics.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModCacheReuse, ModEntry, ModOpenContext},
    modloader::{LoaderId, ModLoaderMeta},
};

use super::{FailedIdentification, ScannedFile};
use crate::{commands::supports::mods::loaders, diagnostics::ErrorReport};

/// A cached inspection includes unsuccessful inputs and complete loader error chains.
#[derive(Debug)]
pub(in crate::commands) struct ScanResult {
    pub file: Option<Arc<ScannedFile>>,
    pub errors: Vec<ScanError>,
    pub cache_reuse: ModCacheReuse,
}
impl ScanResult {
    pub fn failed(&self) -> bool {
        !self.errors.is_empty()
    }
}

#[derive(Debug)]
pub(in crate::commands) struct ScanError {
    pub loader: Option<ModLoaderMeta>,
    pub error: ErrorReport,
}

pub(super) fn read(
    path: PathBuf,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
    context: &ModOpenContext,
) -> ScanResult {
    let started = std::time::Instant::now();
    tracing::debug!(path = %path.display(), "Identifying mod file");
    let mut errors = Vec::new();
    let mut failures = Vec::new();
    let result = ModEntry::new(path.clone()).and_then(|entry| {
        mcmod::open(entry, loaders.clone(), context, |_, loader, _, error| {
            tracing::debug!(path = %path.display(), loader = %loader.id(), error = %format_args!("{error:#}"), "Mod loader identification failed");
            failures.push(FailedIdentification {
                loader: loader.clone(),
                reason: error.root_cause().to_string(),
            });
            errors.push(ScanError { loader: Some(loader.clone()), error: ErrorReport::capture(error) });
            IdentificationErrorAction::Skip
        })
    });
    let mut cache_reuse = ModCacheReuse::None;
    let file = match result {
        Ok(report) => {
            tracing::debug!(cache = ?report.cache, "Mod cache usage");
            cache_reuse = report.cache.reuse();
            let mut file = report.file;
            loaders::sort_metadata(&mut file);
            Some(Arc::new(ScannedFile { file, failures }))
        }
        Err(error) => {
            tracing::debug!(path = %path.display(), error = %format_args!("{error:#}"), "Mod input inspection failed");
            errors.push(ScanError {
                loader: None,
                error: ErrorReport::capture(&error),
            });
            None
        }
    };
    errors.sort_by(|a, b| {
        a.loader
            .as_ref()
            .map(|loader| loader.id())
            .cmp(&b.loader.as_ref().map(|loader| loader.id()))
    });
    tracing::debug!(path = %path.display(), loaded = file.is_some(), errors = errors.len(), elapsed_ms = started.elapsed().as_millis(), "Mod identification completed");
    ScanResult {
        file,
        errors,
        cache_reuse,
    }
}
