//! Command-local scan cache and bounded batches shared by local and remote mod listing.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use kako_mods_lib::local::{
    mcmod::{ModDirectory, ModFile},
    modloader::{LoaderId, ModLoaderMeta},
};

use crate::commands::progress::Progress;

mod batch;
mod file;
pub(in crate::commands) use file::ScanResult;

#[derive(Debug)]
pub(in crate::commands) struct ScannedFile {
    pub file: ModFile,
    pub failures: Vec<FailedIdentification>,
}

#[derive(Debug)]
pub(in crate::commands) struct FailedIdentification {
    pub loader: ModLoaderMeta,
    pub reason: String,
}

/// Results, including failures, live for this command only; no filesystem invalidation.
#[derive(Debug)]
pub(in crate::commands) struct ScanSession {
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    cache: HashMap<PathBuf, Arc<ScanResult>>,
    parallelism: usize,
}
impl ScanSession {
    pub(in crate::commands) fn new(loaders: HashMap<LoaderId, ModLoaderMeta>) -> Self {
        Self {
            loaders,
            cache: HashMap::new(),
            parallelism: 1,
        }
    }

    pub(in crate::commands) fn with_parallelism(mut self, parallelism: usize) -> Self {
        self.parallelism = parallelism.max(1);
        self
    }

    pub(in crate::commands) fn failed_count(&self) -> usize {
        self.cache.values().filter(|result| result.failed()).count()
    }

    pub(super) fn read(
        &mut self,
        path: PathBuf,
        progress: Option<&Progress>,
    ) -> Option<Arc<ScannedFile>> {
        let key = key(&path);
        if let Some(cached) = self.cache.get(&key) {
            tracing::debug!(path = %path.display(), "Reusing cached mod identification");
            return cached.file.clone();
        }
        let result = Arc::new(file::read(path.clone(), &self.loaders));
        report(&path, &result, progress);
        self.cache.insert(key, result.clone());
        result.file.clone()
    }

    /// Deliver completions immediately with original indices. Duplicate normalized
    /// paths share inspection; `fresh` marks the single delivery owning diagnostics.
    pub(in crate::commands) fn batch(
        &mut self,
        paths: &[PathBuf],
        checkpoint: impl Fn() -> Result<()>,
        mut completed: impl FnMut(usize, &ScanResult, bool) -> Result<()>,
    ) -> Result<()> {
        let started = std::time::Instant::now();
        let mut positions = HashMap::new();
        let mut jobs: Vec<(PathBuf, PathBuf, Vec<usize>)> = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            checkpoint()?;
            let key = key(path);
            if let Some(cached) = self.cache.get(&key) {
                tracing::debug!(path = %path.display(), "Reusing cached mod identification");
                completed(index, cached, false)?;
                continue;
            }
            let position = *positions.entry(key.clone()).or_insert_with(|| {
                jobs.push((key, path.clone(), Vec::new()));
                jobs.len() - 1
            });
            jobs[position].2.push(index);
        }
        tracing::info!(
            files = paths.len(),
            unique_pending = jobs.len(),
            parallelism = self.parallelism,
            "Starting mod scan batch"
        );
        let loaders = &self.loaders;
        let result = batch::run(
            jobs.len(),
            self.parallelism,
            |index| file::read(jobs[index].1.clone(), loaders),
            checkpoint,
            |index, result| {
                let (key, _, indices) = &jobs[index];
                let result = Arc::new(result);
                self.cache.insert(key.clone(), result.clone());
                for (position, index) in indices.iter().enumerate() {
                    completed(*index, &result, position == 0)?;
                }
                Ok(())
            },
        );
        tracing::info!(
            files = paths.len(),
            failed = self.failed_count(),
            success = result.is_ok(),
            elapsed_ms = started.elapsed().as_millis(),
            "Mod scan batch ended"
        );
        result
    }

    pub(super) fn directory(&mut self, path: &Path) -> Result<Vec<Arc<ScannedFile>>> {
        let progress = Progress::new("Scanning mod directory...", "Identifying mods");
        let paths: Vec<_> = ModDirectory::new(path.to_owned())?
            .entries()?
            .into_iter()
            .map(|entry| entry.path().to_owned())
            .collect();
        progress.set_total(paths.len());
        self.collect(&paths, &progress)
    }

    /// Final presentation order is independent of worker completion order.
    pub(super) fn collect(
        &mut self,
        paths: &[PathBuf],
        progress: &Progress,
    ) -> Result<Vec<Arc<ScannedFile>>> {
        let mut files = vec![None; paths.len()];
        self.batch(
            paths,
            || Ok(()),
            |index, result, fresh| {
                if fresh {
                    report(&paths[index], result, Some(progress));
                }
                files[index] = result.file.clone();
                progress.advance();
                Ok(())
            },
        )?;
        Ok(files.into_iter().flatten().collect())
    }
}

fn report(path: &Path, result: &ScanResult, progress: Option<&Progress>) {
    for failure in &result.errors {
        let mut error = failure.error.clone().into_error();
        if failure.loader.is_none() {
            error = error.context(format!("failed to inspect {path:?}"));
        }
        match progress {
            Some(progress) => progress.report(&error),
            None => crate::diagnostics::error(&error),
        }
    }
}

/// Normalize parents only: leaf aliases may have different enabled/disabled states.
fn key(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::canonicalize(parent)
        .map(|parent| parent.join(path.file_name().unwrap_or_default()))
        .unwrap_or_else(|_| path.to_owned())
}

#[cfg(test)]
mod tests;
