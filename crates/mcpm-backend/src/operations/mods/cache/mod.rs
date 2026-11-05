//! Explicit mod caching workflow; selection and library storage remain independent owners.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use anyhow::{Context, Result};
use mcpm_lib::local::cache::CacheLinkPreference;
use mcpm_lib::local::cache::ImportOutcome;
use mcpm_lib::local::cache::mods::MetadataRefresh;
use mcpm_lib::local::cache::mods::meta::CacheEntry;
use mcpm_lib::local::mcmod::ModEntry;
use mcpm_lib::local::modloader::LoaderId;
use mcpm_lib::local::modloader::ModLoaderMeta;
use mcpm_lib::local::runtime::RuntimeEnvironment;

use super::loading::{
    scan,
    selection::{self, Scope, Selection},
};
use mcpm_exec::error::PortableError;
use mcpm_lib::blocking;

pub mod record;
pub use record::{PublicationFailure, Record, Status};
mod model;
use mcpm_exec::operation::{Operation, StreamingOutput};
pub use model::*;

/// Command policy over the library's explicit import/refresh operations.
pub struct Options {
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
    pub error: std::sync::Arc<PortableError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Import,
    Metadata,
    Ident,
}
impl Stage {
    pub fn index(self) -> usize {
        match self {
            Self::Import => 0,
            Self::Metadata => 1,
            Self::Ident => 2,
        }
    }
    pub fn as_str(self) -> &'static str {
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
pub struct Summary {
    pub files: usize,
    pub failed_files: usize,
    pub partial_failed_files: usize,
    pub selection_failures: usize,
    pub objects_added: usize,
    pub objects_reused: usize,
}

impl Summary {
    pub fn failed(&self) -> usize {
        self.failed_files
            .saturating_add(self.partial_failed_files)
            .saturating_add(self.selection_failures)
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
pub enum Event {
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
fn store(
    selection: Selection,
    scope: Scope,
    environment: &RuntimeEnvironment,
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
    let cache = environment.require_mods_cache()?;
    let selection_environment = environment.clone().with_mods_cache(None);
    tracing::info!("Selecting files for explicit mod caching");
    let (paths, selection_failures) = selection::resolve_paths(
        selection,
        scope,
        options.loaders.clone(),
        &selection_environment,
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
        |index| import(paths[index].clone(), environment, &options),
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
                        cache.refresh_metadata(imported.key, &options.loaders, options.refresh)
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
                    result.entry = result.import.map(|import| cache.mod_metadata(import.key));
                    result.failure = Some(Failure {
                        stage: Stage::Metadata,
                        error: std::sync::Arc::new(
                            error
                                .context(format!("failed to cache {:?}", result.path))
                                .into(),
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
fn import(path: PathBuf, environment: &RuntimeEnvironment, options: &Options) -> FileResult {
    let mut result = FileResult {
        path,
        import: None,
        entry: None,
        failure: None,
        metadata_requested: options.metadata,
    };
    let operation = (|| {
        ModEntry::new(result.path.clone()).map_err(|error| (Stage::Import, error))?;
        let writer = environment
            .require_mods_cache()
            .map_err(|error| (Stage::Import, error))?;
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
            error: std::sync::Arc::new(error.into()),
        });
    }
    result
}

/// Store and identify on the owning Host; query and publication failures remain independent.
pub struct Cache;
impl Operation for Cache {
    const ID: &'static str = "mod.cache";
    type Input = Input;
    type Output = StreamingOutput<Event>;
    async fn run(input: Input) -> Result<Self::Output> {
        use super::identification::Sources;
        use std::sync::Arc;
        let identification = input.identification;
        let sources = identification
            .as_ref()
            .map(|options| Sources::new(options.sources.clone()))
            .transpose()?
            .map(Arc::new);
        let parallelism = input.options.parallelism.max(1);
        let (resolved, selection) = tokio::task::spawn_blocking(move || {
            let resolved = input.scope.resolve()?;
            resolved.environment.require_mods_cache()?;
            Ok::<_, anyhow::Error>((resolved, input.selection.into_selection()?))
        })
        .await??;
        Ok(StreamingOutput::new(move |sender| async move {
            let environment = Arc::new(resolved.environment);
            let stored_environment = environment.clone();
            let progress = sender.clone();
            let runtime = tokio::runtime::Handle::current();
            let (mut records, mut summary) = tokio::task::spawn_blocking(move || -> Result<_> {
                let mut records = Vec::new();
                let mut summary = None;
                store(
                    selection,
                    resolved.scope,
                    &stored_environment,
                    Options {
                        loaders: resolved.loaders,
                        mode: input.options.mode.into(),
                        refresh: if input.options.force {
                            MetadataRefresh::Force
                        } else {
                            MetadataRefresh::MissingOrOutdated
                        },
                        parallelism,
                        metadata: input.options.metadata,
                    },
                    || progress.checkpoint().map_err(Into::into),
                    |event| {
                        match event {
                            Event::File(record) => records.push(*record),
                            Event::Done(done) => summary = Some(done),
                            event => runtime.block_on(progress.send(event))?,
                        }
                        Ok(())
                    },
                )?;
                Ok((records, summary.context("missing Host storage completion")?))
            })
            .await??;
            if let Some(sources) = sources {
                let force = identification
                    .as_ref()
                    .expect("sources require identification options")
                    .force;
                let total = records.len();
                sender
                    .send(Event::Progress {
                        stage: Stage::Ident,
                        completed: 0,
                        total,
                    })
                    .await?;
                let mut results: Vec<Option<Record>> = (0..total).map(|_| None).collect();
                let mut pending = records.into_iter().enumerate();
                let mut active = tokio::task::JoinSet::new();
                let mut completed = 0;
                loop {
                    while active.len() < parallelism {
                        let Some((index, record)) = pending.next() else {
                            break;
                        };
                        sender.checkpoint()?;
                        let sources = sources.clone();
                        let environment = environment.clone();
                        active.spawn(async move {
                            (
                                index,
                                identify_record(record, environment, sources, force).await,
                            )
                        });
                    }
                    let Some(result) = active.join_next().await else {
                        break;
                    };
                    let (index, record) = result?;
                    results[index] = Some(record);
                    completed += 1;
                    sender
                        .send(Event::Progress {
                            stage: Stage::Ident,
                            completed,
                            total,
                        })
                        .await?;
                }
                records = results
                    .into_iter()
                    .map(|record| record.expect("completed cache file"))
                    .collect();
                sender
                    .send(Event::StageDone {
                        stage: Stage::Ident,
                        success: records.iter().all(|record| {
                            record.overall_status() == super::identification::QueryStatus::Success
                        }),
                    })
                    .await?;
            }
            summary.failed_files = 0;
            summary.partial_failed_files = 0;
            for record in records {
                match record.overall_status() {
                    super::identification::QueryStatus::Success => {}
                    super::identification::QueryStatus::PartialFailed => {
                        summary.partial_failed_files += 1
                    }
                    super::identification::QueryStatus::Failed => summary.failed_files += 1,
                }
                sender.send(Event::File(Box::new(record))).await?;
            }
            sender.send(Event::Done(summary)).await?;
            Ok(())
        }))
    }
}

async fn identify_record(
    mut record: Record,
    environment: std::sync::Arc<RuntimeEnvironment>,
    sources: std::sync::Arc<super::identification::Sources>,
    force: bool,
) -> Record {
    let Some(id) = record.id.clone() else {
        return record;
    };
    let preparing_environment = environment.clone();
    let preparing_id = id.clone();
    let requirements = sources.requirements();
    let preparing_sources = sources.clone();
    let prepared = tokio::task::spawn_blocking(move || -> Result<_> {
        let cache = preparing_environment.require_mods_cache()?;
        let key = preparing_id.parse()?;
        super::identification::prepare(
            &cache.content_path(&key),
            requirements,
            preparing_sources.cache_ids(),
            Some((cache, &preparing_id)),
            force,
        )
    })
    .await
    .context("cache identification preparation worker failed")
    .and_then(|result| result);
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            record.identification_error = Some(std::sync::Arc::new(error.into()));
            return record;
        }
    };
    let identification = sources.identify(&prepared, false).await;
    let matches = identification.new_matches();
    record.identification = Some(identification);
    if matches.is_empty() {
        return record;
    }
    let publication = tokio::task::spawn_blocking(move || -> Result<_> {
        let cache = environment.require_mods_cache()?;
        let mut failures = Vec::new();
        for (source, matched) in matches {
            if let Err(error) = matched.and_then(|matched| {
                super::identification::publish(cache, &id, &prepared.summary, vec![matched])
            }) {
                failures.push(PublicationFailure {
                    source: Some(source),
                    error: std::sync::Arc::new(error.into()),
                });
            }
        }
        Ok(failures)
    })
    .await
    .context("cache identification publication worker failed")
    .and_then(|result| result);
    match publication {
        Ok(failures) => record.publication_errors = failures,
        Err(error) => {
            record.publication_errors.push(PublicationFailure {
                source: None,
                error: std::sync::Arc::new(error.into()),
            });
        }
    }
    record
}

#[cfg(test)]
mod tests;
