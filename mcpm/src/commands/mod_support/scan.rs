//! Cached mod identification shared by lists and metadata queries.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModDirectory, ModEntry, ModFile},
    modloader::{LoaderId, ModLoaderMeta},
};

use super::{loaders, progress::Progress};

pub(super) struct ScannedFile {
    pub file: ModFile,
    pub failures: Vec<FailedIdentification>,
}

pub(super) struct FailedIdentification {
    pub loader: ModLoaderMeta,
    pub reason: String,
}

/// A command-local cache, including failures, keyed by normalized entry path.
/// Original input paths remain on the records for display.
pub(super) struct ScanSession {
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    cache: HashMap<PathBuf, Option<Arc<ScannedFile>>>,
    failed: HashSet<PathBuf>,
}

impl ScanSession {
    pub(super) fn new(loaders: HashMap<LoaderId, ModLoaderMeta>) -> Self {
        Self {
            loaders,
            cache: HashMap::new(),
            failed: HashSet::new(),
        }
    }

    pub(super) fn failed_count(&self) -> usize {
        self.failed.len()
    }

    pub(super) fn read(
        &mut self,
        path: PathBuf,
        progress: Option<&Progress>,
    ) -> Option<Arc<ScannedFile>> {
        // Keep the leaf filename: .jar and .jar.disabled aliases have different
        // enabled states even when symlinks point to the same archive bytes.
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let key = std::fs::canonicalize(parent)
            .map(|parent| parent.join(path.file_name().unwrap_or_default()))
            .unwrap_or_else(|_| path.clone());
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }
        let report = |error: &anyhow::Error| match progress {
            Some(progress) => progress.report(error),
            None => crate::diagnostics::error(error),
        };
        let mut failures = Vec::new();
        let result = ModEntry::new(path.clone())
            .context("failed to validate mod file path")
            .and_then(|entry| {
                mcmod::open(entry, self.loaders.clone(), |_, loader, _, error| {
                    report(error);
                    failures.push(FailedIdentification {
                        loader: loader.clone(),
                        reason: error.root_cause().to_string(),
                    });
                    IdentificationErrorAction::Skip
                })
            });
        let file = match result {
            Ok(mut file) => {
                loaders::sort_metadata(&mut file);
                if !failures.is_empty() {
                    self.failed.insert(key.clone());
                }
                Some(Arc::new(ScannedFile { file, failures }))
            }
            Err(error) => {
                report(&error.context(format!("failed to inspect {path:?}")));
                self.failed.insert(key.clone());
                None
            }
        };
        self.cache.insert(key, file.clone());
        file
    }

    /// Enumerates only this directory and identifies previously unseen files.
    pub(super) fn directory(&mut self, path: &Path) -> Result<Vec<Arc<ScannedFile>>> {
        let progress = Progress::new();
        let entries = ModDirectory::new(path.to_owned())?.entries()?;
        progress.set_total(entries.len());
        let mut files = Vec::new();
        for entry in entries {
            if let Some(file) = self.read(entry.path().to_owned(), Some(&progress)) {
                files.push(file);
            }
            progress.advance();
        }
        Ok(files)
    }
}
