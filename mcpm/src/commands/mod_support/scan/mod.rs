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

use crate::diagnostics::ErrorReport;

mod batch;
mod file;
mod progress;
pub(in crate::commands) use file::ScanResult;
pub(in crate::commands) use progress::ProgressState;

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
pub(in crate::commands) struct ScanSession<'a> {
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    cache: HashMap<PathBuf, Arc<ScanResult>>,
    parallelism: usize,
    checkpoint: Checkpoint<'a>,
    observer: Option<Observer<'a>>,
}
impl<'a> ScanSession<'a> {
    pub(in crate::commands) fn new(loaders: HashMap<LoaderId, ModLoaderMeta>) -> Self {
        Self {
            loaders,
            cache: HashMap::new(),
            parallelism: 1,
            checkpoint: Checkpoint(&|| Ok(())),
            observer: None,
        }
    }

    pub(in crate::commands) fn with_parallelism(mut self, parallelism: usize) -> Self {
        self.parallelism = parallelism.max(1);
        self
    }

    /// Cancellation is independent of diagnostic and progress delivery.
    pub(super) fn with_checkpoint(mut self, checkpoint: &'a dyn Fn() -> Result<()>) -> Self {
        self.checkpoint = Checkpoint(checkpoint);
        self
    }

    /// The consumer owns presentation or transport of all scan events.
    pub(super) fn with_observer(mut self, observer: &'a dyn Fn(Event) -> Result<()>) -> Self {
        self.observer = Some(Observer(observer));
        self
    }

    fn emit(&self, event: Event) -> Result<()> {
        if let Some(observer) = &self.observer {
            (observer.0)(event)?;
        }
        Ok(())
    }

    pub(in crate::commands) fn failed_count(&self) -> usize {
        self.cache.values().filter(|result| result.failed()).count()
    }

    pub(super) fn read(&mut self, path: PathBuf) -> Result<Option<Arc<ScannedFile>>> {
        let key = key(&path);
        if let Some(cached) = self.cache.get(&key) {
            tracing::debug!(path = %path.display(), "Reusing cached mod identification");
            return Ok(cached.file.clone());
        }
        self.emit(Event::Progress {
            completed: 0,
            total: 1,
        })?;
        (self.checkpoint.0)()?;
        let result = Arc::new(file::read(path.clone(), &self.loaders));
        (self.checkpoint.0)()?;
        for error in errors(&path, &result) {
            self.emit(Event::Diagnostic {
                error: ErrorReport::capture(&error),
            })?;
        }
        self.emit(Event::Progress {
            completed: 1,
            total: 1,
        })?;
        self.cache.insert(key, result.clone());
        Ok(result.file.clone())
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
        tracing::debug!(
            files = paths.len(),
            failed = self.failed_count(),
            success = result.is_ok(),
            elapsed_ms = started.elapsed().as_millis(),
            "Mod scan batch ended"
        );
        result
    }

    pub(super) fn directory(
        &mut self,
        path: &Path,
    ) -> std::result::Result<Vec<Arc<ScannedFile>>, DirectoryError> {
        let paths: Vec<_> = (|| -> Result<_> { ModDirectory::new(path.to_owned())?.entries() })()
            .map_err(DirectoryError::Input)?
            .into_iter()
            .map(|entry| entry.path().to_owned())
            .collect();
        self.collect(&paths).map_err(DirectoryError::Execution)
    }

    /// Final presentation order is independent of worker completion order.
    pub(super) fn collect(&mut self, paths: &[PathBuf]) -> Result<Vec<Arc<ScannedFile>>> {
        let mut files = vec![None; paths.len()];
        let checkpoint = self.checkpoint.0;
        let observer = self.observer.as_ref().map(|observer| observer.0);
        let emit = |event| -> Result<()> {
            if let Some(observer) = observer {
                observer(event)?;
            }
            Ok(())
        };
        emit(Event::Progress {
            completed: 0,
            total: paths.len(),
        })?;
        let mut completed = 0;
        self.batch(paths, checkpoint, |index, result, fresh| {
            if fresh {
                for error in errors(&paths[index], result) {
                    emit(Event::Diagnostic {
                        error: ErrorReport::capture(&error),
                    })?;
                }
            }
            files[index] = result.file.clone();
            completed += 1;
            emit(Event::Progress {
                completed,
                total: paths.len(),
            })
        })?;
        Ok(files.into_iter().flatten().collect())
    }
}

/// Scanning produces data only; terminal and transport adapters consume this stream.
pub(in crate::commands) enum Event {
    Progress { completed: usize, total: usize },
    Diagnostic { error: ErrorReport },
}

#[derive(Debug, thiserror::Error)]
pub(super) enum DirectoryError {
    #[error(transparent)]
    Input(anyhow::Error),
    #[error(transparent)]
    Execution(anyhow::Error),
}

fn errors<'a>(path: &'a Path, result: &'a ScanResult) -> impl Iterator<Item = anyhow::Error> + 'a {
    result.errors.iter().map(move |failure| {
        let error = failure.error.clone().into_error();
        if failure.loader.is_none() {
            error.context(format!("failed to inspect {path:?}"))
        } else {
            error
        }
    })
}

struct Observer<'a>(&'a dyn Fn(Event) -> Result<()>);

impl std::fmt::Debug for Observer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Observer")
    }
}

struct Checkpoint<'a>(&'a dyn Fn() -> Result<()>);

impl std::fmt::Debug for Checkpoint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Checkpoint")
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
