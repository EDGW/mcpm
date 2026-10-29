//! Adapts library identification reports to scan results and diagnostics.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::local::{
    mcmod::{IdentificationErrorAction, ModEntry, ModFile},
    modloader::{LoaderId, ModLoaderMeta},
};

use crate::data::cache::{self, mods::ModCacheReuse};
use crate::data::runtime::RuntimeEnvironment;

use super::{FailedIdentification, ScannedFile};

/// A cached inspection includes unsuccessful inputs and complete loader error chains.
#[derive(Debug)]
pub(crate) struct ScanResult {
    pub(crate) file: Option<Arc<ScannedFile>>,
    pub(crate) errors: Vec<ScanError>,
    pub(crate) cache_reuse: ModCacheReuse,
}

impl ScanResult {
    pub(crate) fn failed(&self) -> bool {
        !self.errors.is_empty()
    }
}

#[derive(Debug)]
pub(crate) struct ScanError {
    pub(crate) loader: Option<ModLoaderMeta>,
    pub(crate) error: ErrorReport,
}

pub(super) fn read(
    path: PathBuf,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
    environment: &RuntimeEnvironment,
) -> ScanResult {
    let started = std::time::Instant::now();
    tracing::debug!(path = %path.display(), "Identifying mod file");
    let mut errors = Vec::new();
    let mut failures = Vec::new();
    let result = ModEntry::new(path.clone()).and_then(|entry| {
        cache::mods::open(entry, loaders.clone(), environment, |_, loader, _, error| {
            tracing::debug!(path = %path.display(), loader = %loader.id(), error = %format_args!("{error:#}"), "Mod loader identification failed");
            let report = ErrorReport::capture(error);
            failures.push(FailedIdentification {
                loader: loader.clone(),
                error: report.clone(),
            });
            errors.push(ScanError { loader: Some(loader.clone()), error: report });
            IdentificationErrorAction::Skip
        })
    });
    let mut cache_reuse = ModCacheReuse::None;
    let file = match result {
        Ok(report) => {
            tracing::debug!(cache = ?report.cache, "Mod cache usage");
            cache_reuse = report.cache.reuse();
            let mut file = report.file;
            sort_metadata(&mut file);
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

/// Keeps console output stable independently of hash map iteration order.
fn sort_metadata(file: &mut ModFile) {
    file.metadata.sort_by(|left, right| {
        display_order(left.loader.id())
            .cmp(&display_order(right.loader.id()))
            .then_with(|| left.loader.id().cmp(right.loader.id()))
    });
}

fn display_order(id: &LoaderId) -> usize {
    match id.as_str() {
        "forge" => 0,
        "neoforge" => 1,
        "fabric" => 2,
        _ => 4,
    }
}
