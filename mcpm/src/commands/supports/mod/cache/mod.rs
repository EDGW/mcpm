//! Explicit mod caching workflow; selection and library storage remain independent owners.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use anyhow::Result;
use mcpm_backend::local::cache::CacheLinkPreference;
use mcpm_backend::local::cache::ImportOutcome;
use mcpm_backend::local::cache::mods::MetadataRefresh;
use mcpm_backend::local::cache::mods::ModsCacheDir;
use mcpm_backend::local::cache::mods::meta::CacheEntry;
use mcpm_backend::local::mcmod::ModEntry;
use mcpm_backend::local::modloader::LoaderId;
use mcpm_backend::local::modloader::ModLoaderMeta;

use super::{
    scan,
    selection::{self, Scope, Selection},
};
use crate::{commands::execution::blocking, diagnostics::ErrorReport};

mod record;
pub(in crate::commands) use record::{Record, Status};
pub(in crate::commands) mod args;
pub(in crate::commands) mod identification;
pub(in crate::commands) mod output;
mod validation;

/// Command policy over the library's explicit import/refresh operations.
pub(in crate::commands) struct Options {
    pub cache: ModsCacheDir,
    pub loaders: HashMap<LoaderId, ModLoaderMeta>,
    pub mode: CacheLinkPreference,
    pub refresh: MetadataRefresh,
    pub parallelism: usize,
    pub metadata: bool,
}

/// Each selected outer file is processed once, regardless of matching declarations.
struct FileResult {
    pub path: PathBuf,
    pub import: Option<ImportOutcome>,
    pub entry: Option<CacheEntry>,
    pub failure: Option<Failure>,
    pub metadata_requested: bool,
}

struct Failure {
    pub stage: Stage,
    pub error: ErrorReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::commands) enum Stage {
    Import,
    Metadata,
    Ident,
}
impl Stage {
    pub(super) fn index(self) -> usize {
        match self {
            Self::Import => 0,
            Self::Metadata => 1,
            Self::Ident => 2,
        }
    }
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Metadata => "metadata",
            Self::Ident => "ident",
        }
    }
}

/// Object totals count distinct BLAKE3s; an object added here is not also reused here.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::commands) struct Summary {
    pub files: usize,
    pub failed_files: usize,
    pub selection_failures: usize,
    pub objects_added: usize,
    pub objects_reused: usize,
}

impl Summary {
    pub fn failed(&self) -> usize {
        self.failed_files.saturating_add(self.selection_failures)
    }
}

/// Data-only events. Rendering, progress and the final exit policy belong to callers.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "event",
    content = "contents",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(in crate::commands) enum Event {
    Selection(scan::Event),
    Progress {
        stage: Stage,
        completed: usize,
        total: usize,
    },
    StageDone {
        stage: Stage,
        success: bool,
    },
    File(Box<Record>),
    Done(Summary),
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(in crate::commands) fn run(
    selection: Selection,
    scope: Scope,
    options: Options,
    checkpoint: impl Fn() -> Result<()>,
    emit: impl FnMut(Event) -> Result<()>,
) -> Result<()> {
    checkpoint()?;
    let emit = RefCell::new(emit);
    if selection.is_empty() {
        return (emit.borrow_mut())(Event::Done(Summary::default()));
    }
    // Validate before any input scanning or storage. Default-root initialization is
    // owned by the library; explicit roots are never silently adopted.
    // ModsCacheDir was opened before this workflow started.
    tracing::info!("Selecting files for explicit mod caching");
    let (paths, selection_failures) = selection::resolve_paths(
        selection,
        scope,
        options.loaders.clone(),
        options.parallelism,
        &checkpoint,
        |event| (emit.borrow_mut())(Event::Selection(event)),
    )?;
    let mut summary = Summary {
        files: paths.len(),
        selection_failures,
        ..Summary::default()
    };
    let mut added = HashSet::new();
    let mut reused = HashSet::new();
    let mut pending: Vec<Option<FileResult>> = (0..paths.len()).map(|_| None).collect();
    let mut completed = 0;
    (emit.borrow_mut())(Event::Progress {
        stage: Stage::Import,
        completed,
        total: paths.len(),
    })?;
    tracing::info!(
        files = paths.len(),
        parallelism = options.parallelism,
        "Importing selected mod files"
    );
    blocking::run(
        paths.len(),
        options.parallelism,
        |index| import(paths[index].clone(), &options),
        &checkpoint,
        |index, result| {
            completed += 1;
            (emit.borrow_mut())(Event::Progress {
                stage: Stage::Import,
                completed,
                total: paths.len(),
            })?;
            pending[index] = Some(result);
            Ok(())
        },
    )?;
    let mut results: Vec<_> = pending.into_iter().map(Option::unwrap).collect();
    (emit.borrow_mut())(Event::StageDone {
        stage: Stage::Import,
        success: results.iter().all(|result| result.failure.is_none()),
    })?;
    if options.metadata {
        let mut pending: Vec<Option<Option<Result<CacheEntry>>>> =
            (0..paths.len()).map(|_| None).collect();
        let mut completed = 0;
        (emit.borrow_mut())(Event::Progress {
            stage: Stage::Metadata,
            completed,
            total: paths.len(),
        })?;
        blocking::run(
            paths.len(),
            options.parallelism,
            |index| {
                results[index].import.and_then(|imported| {
                    results[index].failure.is_none().then(|| {
                        options.cache.refresh_metadata(
                            imported.key,
                            &options.loaders,
                            options.refresh,
                        )
                    })
                })
            },
            &checkpoint,
            |index, result| {
                completed += 1;
                (emit.borrow_mut())(Event::Progress {
                    stage: Stage::Metadata,
                    completed,
                    total: paths.len(),
                })?;
                pending[index] = Some(result);
                Ok(())
            },
        )?;
        for (result, metadata) in results.iter_mut().zip(pending) {
            match metadata.expect("metadata worker returned a result") {
                Some(Ok(entry)) => result.entry = Some(entry),
                Some(Err(error)) => {
                    result.entry = result
                        .import
                        .map(|import| options.cache.mod_metadata(import.key));
                    result.failure = Some(Failure {
                        stage: Stage::Metadata,
                        error: ErrorReport::capture(
                            &error.context(format!("failed to cache {:?}", result.path)),
                        ),
                    });
                }
                None => {}
            }
        }
        (emit.borrow_mut())(Event::StageDone {
            stage: Stage::Metadata,
            success: results.iter().all(|result| result.failure.is_none()),
        })?;
    }
    for result in results {
        summary.failed_files += usize::from(result.failure.is_some());
        if let Some(imported) = result.import {
            if imported.published {
                added.insert(imported.key);
            } else {
                reused.insert(imported.key);
            }
        }
        (emit.borrow_mut())(Event::File(Box::new(Record::from(result))))?;
    }
    summary.objects_added = added.len();
    summary.objects_reused = reused.difference(&added).count();
    (emit.borrow_mut())(Event::Done(summary))?;
    Ok(())
}

/// Never reuse selection metadata for persistence: identification must run on the
/// actual imported object through the library's provenance-checked commit path.
fn import(path: PathBuf, options: &Options) -> FileResult {
    let mut result = FileResult {
        path,
        import: None,
        entry: None,
        failure: None,
        metadata_requested: options.metadata,
    };
    let operation = (|| {
        ModEntry::new(result.path.clone()).map_err(|error| (Stage::Import, error))?;
        let writer = &options.cache;
        let imported = writer
            .import_with_outcome(&result.path, options.mode)
            .map_err(|error| (Stage::Import, error))?;
        result.import = Some(imported);
        result.entry = Some(writer.mod_metadata(imported.key));
        Ok(())
    })();
    if let Err((stage, error)) = operation {
        let error = error.context(format!("failed to cache {:?}", result.path));
        result.failure = Some(Failure {
            stage,
            error: ErrorReport::capture(&error),
        });
    }
    result
}
