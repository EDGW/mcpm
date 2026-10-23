//! Explicit mod caching workflow; selection and library storage remain independent owners.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use anyhow::Result;
use kako_mods_lib::local::{
    mcmod::{
        ModEntry,
        cache::{CacheEntry, ImportMode, ImportOutcome, MetadataRefresh, ModsCache},
    },
    modloader::{LoaderId, ModLoaderMeta},
};

use super::{
    scan,
    selection::{self, Scope, Selection},
};
use crate::{commands::execution::blocking, diagnostics::ErrorReport};

mod record;
pub(in crate::commands) use record::{Record, Status};
pub(in crate::commands) mod args;
pub(in crate::commands) mod output;
mod validation;

/// Command policy over the library's explicit import/refresh operations.
pub(in crate::commands) struct Options {
    pub cache: ModsCache,
    pub loaders: HashMap<LoaderId, ModLoaderMeta>,
    pub mode: ImportMode,
    pub refresh: MetadataRefresh,
    pub parallelism: usize,
}

/// Each selected outer file is processed once, regardless of matching declarations.
struct FileResult {
    pub path: PathBuf,
    pub import: Option<ImportOutcome>,
    pub entry: Option<CacheEntry>,
    pub failure: Option<Failure>,
}

struct Failure {
    pub stage: Stage,
    pub error: ErrorReport,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::commands) enum Stage {
    Import,
    Metadata,
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
    Progress { completed: usize, total: usize },
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
    options.cache.reader()?;
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
    let mut next = 0;
    let mut completed = 0;
    (emit.borrow_mut())(Event::Progress {
        completed,
        total: paths.len(),
    })?;
    tracing::info!(
        files = paths.len(),
        parallelism = options.parallelism,
        "Caching selected mod files"
    );
    blocking::run(
        paths.len(),
        options.parallelism,
        |index| process(paths[index].clone(), &options),
        checkpoint,
        |index, result| {
            completed += 1;
            (emit.borrow_mut())(Event::Progress {
                completed,
                total: paths.len(),
            })?;
            if result.failure.is_some() {
                summary.failed_files += 1;
            }
            if let Some(imported) = result.import {
                if imported.published {
                    added.insert(imported.id);
                } else {
                    reused.insert(imported.id);
                }
            }
            pending[index] = Some(result);
            while next < pending.len() {
                let Some(result) = pending[next].take() else {
                    break;
                };
                (emit.borrow_mut())(Event::File(Box::new(Record::from(result))))?;
                next += 1;
            }
            Ok(())
        },
    )?;
    summary.objects_added = added.len();
    summary.objects_reused = reused.difference(&added).count();
    (emit.borrow_mut())(Event::Done(summary))?;
    Ok(())
}

/// Never reuse selection metadata for persistence: identification must run on the
/// actual imported object through the library's provenance-checked commit path.
fn process(path: PathBuf, options: &Options) -> FileResult {
    let mut result = FileResult {
        path,
        import: None,
        entry: None,
        failure: None,
    };
    let operation = (|| {
        ModEntry::new(result.path.clone()).map_err(|error| (Stage::Import, error))?;
        let mut writer = options
            .cache
            .writer()
            .map_err(|error| (Stage::Import, anyhow::Error::new(error)))?;
        let imported = writer
            .import_with_mode(&result.path, options.mode)
            .map_err(|error| (Stage::Import, anyhow::Error::new(error)))?;
        result.import = Some(imported);
        result.entry = Some(writer.get(imported.id));
        match writer.refresh_metadata(imported.id, &options.loaders, options.refresh) {
            Ok(entry) => result.entry = Some(entry),
            Err(error) => {
                result.entry = Some(writer.get(imported.id));
                return Err((Stage::Metadata, anyhow::Error::new(error)));
            }
        }
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
