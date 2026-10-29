//! `mods.jar.read`: identifies every selected outer mod file on its owning host.
//!
//! The operation enumerates inputs, runs the shared cached scan and emits one record per
//! outer file. Presentation filtering stays with the caller; the output carries only the
//! acquisition totals.

use std::future::Future;

use kako_mc_lib::error::ErrorReport;
use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};

use crate::error::OperationError;
use crate::mods::records::list::{ListSummary, Record};
use crate::mods::scan::{Event as ScanEvent, ScanSession};
use crate::mods::selection::{FileScope, Resolver, Selection, SelectionSpec};
use crate::mods::types::{ModsCachePolicy, ModsScope};
use crate::scope::{self, HostCache, ResolvedScope, ScopeError, ScopeKind};

/// Reads and identifies mod files in one scope.
#[derive(Debug)]
pub struct JarRead;

crate::host_operation!(JarRead, "mods.jar.read");

impl Operation for JarRead {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = ReadInput;
    type Event = ReadEvent;
    type Output = ListSummary;
    type Error = OperationError<ReadKind>;

    fn run(
        input: ReadInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        let emitter = cx.emitter();
        async move {
            cx.blocking(move || read(input, &emitter))
                .await
                .map_err(|error: ReadFailure| error.into_operation())
        }
    }
}

/// One scoped read request; the selection is empty when only the supplied paths are read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    pub scope: ModsScope,
    #[serde(default)]
    pub selection: SelectionSpec,
    #[serde(default = "default_parallelism")]
    pub parallelism: usize,
    #[serde(default)]
    pub cache: ModsCachePolicy,
}

/// Data-only progress and records; rendering belongs to the caller.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadEvent {
    /// Resolved loader constraints and the host cache selection.
    Scope {
        loaders: Vec<String>,
        environment: HostCache,
    },
    Scan {
        completed: usize,
        total: usize,
    },
    Diagnostic {
        error: ErrorReport,
    },
    Item {
        index: usize,
        record: Record,
    },
}

fn default_parallelism() -> usize {
    crate::mods::DEFAULT_PARALLELISM
}

fn read(
    input: ReadInput,
    emitter: &mcpm_exec::Emitter<ReadEvent>,
) -> Result<ListSummary, ReadFailure> {
    if input.parallelism == 0 {
        return Err(ReadFailure::Input(anyhow::anyhow!(
            "parallelism must be positive"
        )));
    }
    let resolved = scope::resolve(&input.scope, &input.cache).map_err(ReadFailure::Scope)?;
    let host_cache = HostCache::capture(&resolved.environment).map_err(ReadFailure::Cache)?;
    emitter
        .emit(ReadEvent::Scope {
            loaders: source_loaders(&resolved),
            environment: host_cache,
        })
        .map_err(ReadFailure::Cancelled)?;
    let selection = input
        .selection
        .into_selection()
        .map_err(ReadFailure::Selection)?;

    let observe = |event: ScanEvent| -> anyhow::Result<()> {
        match event {
            ScanEvent::Progress { completed, total } => emitter
                .emit(ReadEvent::Scan { completed, total })
                .map_err(anyhow::Error::new),
            ScanEvent::Diagnostic { error } => emitter
                .emit(ReadEvent::Diagnostic { error })
                .map_err(anyhow::Error::new),
        }
    };
    let checkpoint = || -> anyhow::Result<()> {
        emitter
            .checkpoint()
            .map_err(anyhow::Error::new)
    };
    let file_scope = match &input.scope {
        ModsScope::Paths { directory, .. } => FileScope::directory(directory.clone()),
        ModsScope::Server { .. } => FileScope::container(resolved.directory.clone()),
    };
    let scan = ScanSession::new(resolved.loaders, &resolved.environment)
        .with_parallelism(input.parallelism)
        .with_checkpoint(&checkpoint)
        .with_observer(&observe);

    let explicit = matches!(input.scope, ModsScope::Paths { .. }) && selection.is_empty();
    if explicit {
        read_explicit(&input.scope, scan, emitter, checkpoint)
    } else {
        read_selected(selection, file_scope, scan, emitter, checkpoint)
    }
}

fn read_explicit(
    scope: &ModsScope,
    mut scan: ScanSession<'_>,
    emitter: &mcpm_exec::Emitter<ReadEvent>,
    checkpoint: impl Fn() -> anyhow::Result<()> + Copy,
) -> Result<ListSummary, ReadFailure> {
    let paths = match scope {
        ModsScope::Paths { paths, .. } => paths.clone(),
        ModsScope::Server { .. } => unreachable!("server scope lists through the resolver"),
    };
    let mut entries = Vec::new();
    for path in paths {
        checkpoint().map_err(ReadFailure::CancelledFrom)?;
        match crate::mods::inputs::read(path.clone()) {
            Ok(found) => entries.extend(
                found
                    .into_iter()
                    .map(|entry| (entry.path().to_owned(), Ok(entry))),
            ),
            Err(error) => {
                tracing::debug!(path = %path.display(), error = %format_args!("{error:#}"), "Retaining failed input as a mod list record");
                entries.push((path, Err(error)));
            }
        }
    }
    let mut files = 0;
    let mut input_errors = 0;
    let mut paths = Vec::new();
    for (path, entry) in entries {
        match entry {
            Ok(_) => paths.push(path),
            Err(error) => {
                input_errors += 1;
                emitter
                    .emit(ReadEvent::Item {
                        index: files,
                        record: Record::failed_input(&path, &error),
                    })
                    .map_err(ReadFailure::Cancelled)?;
                files += 1;
            }
        }
    }
    scan.batch(&paths, checkpoint, |position, result, _| {
        let record = Record::from_scan(&paths[position], result)
            .map_err(anyhow::Error::from)?;
        tracing::debug!(path = %record.path, status = ?record.status, errors = record.errors.len(), "Loaded mod record");
        emitter
            .emit(ReadEvent::Item {
                index: files,
                record,
            })
            .map_err(anyhow::Error::new)?;
        files += 1;
        Ok(())
    })
    .map_err(ReadFailure::Other)?;
    let failed = scan.failed_count();
    Ok(ListSummary {
        files,
        input_errors,
        failed,
        cache: Some(scan.cache_summary()),
    })
}

fn read_selected(
    selection: Selection,
    file_scope: FileScope,
    scan: ScanSession<'_>,
    emitter: &mcpm_exec::Emitter<ReadEvent>,
    checkpoint: impl Fn() -> anyhow::Result<()> + Copy,
) -> Result<ListSummary, ReadFailure> {
    let mut files = 0;
    let mut input_errors = 0;
    let mut resolver = Resolver::new(scan, file_scope);
    resolver
        .visit(
            selection,
            checkpoint,
            |error| {
                input_errors += 1;
                emitter
                    .emit(ReadEvent::Diagnostic { error })
                    .map_err(anyhow::Error::new)
            },
            |selected| {
                let record = Record::from_selected(&selected)
                    .map_err(anyhow::Error::from)?;
                emitter
                    .emit(ReadEvent::Item {
                        index: files,
                        record,
                    })
                    .map_err(anyhow::Error::new)?;
                files += 1;
                Ok(())
            },
        )
        .map_err(ReadFailure::Other)?;
    let failed = resolver.scan.failed_count();
    Ok(ListSummary {
        files,
        input_errors,
        failed,
        cache: Some(resolver.scan.cache_summary()),
    })
}

fn source_loaders(resolved: &ResolvedScope) -> Vec<String> {
    if !resolved.loaders_filtered {
        return Vec::new();
    }
    let mut ids: Vec<_> = resolved
        .loaders
        .keys()
        .map(|id| id.as_str().to_owned())
        .collect();
    ids.sort();
    ids
}

#[derive(Debug, thiserror::Error)]
enum ReadFailure {
    #[error(transparent)]
    Input(anyhow::Error),
    #[error(transparent)]
    Scope(ScopeError),
    #[error("invalid mod selection")]
    Selection(#[source] anyhow::Error),
    #[error("failed to resolve the Mod cache")]
    Cache(#[source] anyhow::Error),
    #[error("mod reading was cancelled")]
    Cancelled(mcpm_exec::Cancelled),
    #[error(transparent)]
    CancelledFrom(anyhow::Error),
    #[error(transparent)]
    Other(anyhow::Error),
}

impl ReadFailure {
    fn into_operation(self) -> OperationError<ReadKind> {
        let kind = match &self {
            Self::Input(_) => ReadKind::Selection,
            Self::Scope(error) => ReadKind::Scope(error.kind()),
            Self::Selection(_) => ReadKind::Selection,
            Self::Cache(_) => ReadKind::Cache,
            Self::Cancelled(_) | Self::CancelledFrom(_) => ReadKind::Cancelled,
            Self::Other(_) => ReadKind::Other,
        };
        OperationError::from_source(kind, self)
    }
}

/// Stable classification of a read failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadKind {
    Scope(ScopeKind),
    Selection,
    Cache,
    Cancelled,
    Other,
}
