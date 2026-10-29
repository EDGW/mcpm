//! `mods.select`: resolves ordered selections and reports the selected declarations.
//!
//! One operation serves `info` and `deps` for local paths and registered servers: the
//! selection is resolved on the host that owns the files, so a single JAR is read once and
//! the caller receives only the declarations it asked for.

use std::{collections::HashSet, future::Future, path::PathBuf};

use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};

use crate::error::OperationError;
use crate::mods::records::inspect::{self, Event};
use crate::mods::scan::{CacheSummary, ScanSession};
use crate::mods::selection::{FileScope, Resolver, SelectionSpec};
use crate::mods::types::{ModsCachePolicy, ModsScope};
use crate::scope::{self, ResolvedScope, ScopeError, ScopeKind};

/// Resolves selections and inspects the selected outer mod files.
#[derive(Debug)]
pub struct Select;

crate::host_operation!(Select, "mods.select");

impl Operation for Select {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = SelectInput;
    type Event = Event;
    type Output = SelectSummary;
    type Error = OperationError<SelectKind>;

    fn run(
        input: SelectInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        let emitter = cx.emitter();
        async move {
            cx.blocking(move || select(input, &emitter))
                .await
                .map_err(|error: SelectFailure| error.into_operation())
        }
    }
}

/// One selection request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectInput {
    pub scope: ModsScope,
    #[serde(default)]
    pub selection: SelectionSpec,
    #[serde(default = "default_parallelism")]
    pub parallelism: usize,
    #[serde(default)]
    pub cache: ModsCachePolicy,
    /// Count unrecognized files as ignored instead of reporting them as failures.
    #[serde(default = "default_hide")]
    pub hide_unrecognized: bool,
}

fn default_hide() -> bool {
    true
}

/// Acquisition totals plus the unique selected paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectSummary {
    pub paths: Vec<PathBuf>,
    pub files: usize,
    pub failed: usize,
    pub ignored: usize,
    pub cache: Option<CacheSummary>,
}

fn default_parallelism() -> usize {
    crate::mods::DEFAULT_PARALLELISM
}

fn select(
    input: SelectInput,
    emitter: &mcpm_exec::Emitter<Event>,
) -> Result<SelectSummary, SelectFailure> {
    if input.parallelism == 0 {
        return Err(SelectFailure::Input(anyhow::anyhow!(
            "parallelism must be positive"
        )));
    }
    let resolved = scope::resolve(&input.scope, &input.cache).map_err(SelectFailure::Scope)?;
    let selection = input
        .selection
        .into_selection()
        .map_err(SelectFailure::Selection)?;
    let file_scope = file_scope(&input.scope, &resolved);
    let observe = |event: crate::mods::scan::Event| -> anyhow::Result<()> {
        let event = match event {
            crate::mods::scan::Event::Progress { completed, total } => {
                Event::Scan { completed, total }
            }
            crate::mods::scan::Event::Diagnostic { error } => Event::Error { error },
        };
        emitter
            .emit(event)
            .map_err(anyhow::Error::new)
    };
    let checkpoint = || -> anyhow::Result<()> {
        emitter
            .checkpoint()
            .map_err(anyhow::Error::new)
    };
    let scan = ScanSession::new(resolved.loaders, &resolved.environment)
        .with_parallelism(input.parallelism)
        .with_checkpoint(&checkpoint)
        .with_observer(&observe);
    let hide_unrecognized = input.hide_unrecognized;
    let mut resolver = Resolver::new(scan, file_scope);
    let mut files = 0;
    let mut ignored = 0;
    let mut unrecognized = 0;
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let selection_failures = resolver
        .visit(
            selection,
            checkpoint,
            |error| {
                emitter
                    .emit(Event::Error { error })
                    .map_err(anyhow::Error::new)
            },
            |selected| {
                let file = &selected.scanned.file;
                if selected.scanned.failures.is_empty() && file.metadata.is_empty() {
                    if hide_unrecognized {
                        ignored += 1;
                    } else {
                        let error = UnrecognizedModFile {
                            path: file.entry.path().to_owned(),
                        }
                        .into();
                        emitter
                            .emit(Event::Error {
                                error: kako_mc_lib::error::ErrorReport::capture(&error),
                            })
                            .map_err(anyhow::Error::new)?;
                        unrecognized += 1;
                    }
                    return Ok(());
                }
                tracing::debug!(path = %file.entry.path().display(), loaders = file.metadata.len(), failures = selected.scanned.failures.len(), "Emitting inspected mod file");
                let path = file.entry.path().to_owned();
                if seen.insert(path.clone()) {
                    paths.push(path);
                }
                emitter
                    .emit(Event::File {
                        index: files,
                        file: inspect::File::from(&selected),
                    })
                    .map_err(anyhow::Error::new)?;
                files += 1;
                Ok(())
            },
        )
        .map_err(SelectFailure::Other)?;
    let failed = resolver.scan.failed_count() + selection_failures + unrecognized;
    let summary = SelectSummary {
        paths,
        files,
        failed,
        ignored,
        cache: Some(resolver.scan.cache_summary()),
    };
    emitter
        .emit(Event::Done {
            files,
            failed,
            ignored,
            cache: summary.cache,
        })
        .map_err(SelectFailure::Cancelled)?;
    Ok(summary)
}

fn file_scope(scope: &ModsScope, resolved: &ResolvedScope) -> FileScope {
    match scope {
        ModsScope::Paths { directory, .. } => FileScope::directory(directory.clone()),
        ModsScope::Server { .. } => FileScope::container(resolved.directory.clone()),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("no supported loader recognized mod file {path:?}")]
struct UnrecognizedModFile {
    path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
enum SelectFailure {
    #[error(transparent)]
    Input(anyhow::Error),
    #[error(transparent)]
    Scope(ScopeError),
    #[error("invalid mod selection")]
    Selection(#[source] anyhow::Error),
    #[error("mod selection was cancelled")]
    Cancelled(mcpm_exec::Cancelled),
    #[error(transparent)]
    Other(anyhow::Error),
}

impl SelectFailure {
    fn into_operation(self) -> OperationError<SelectKind> {
        let kind = match &self {
            Self::Input(_) => SelectKind::Selection,
            Self::Scope(error) => SelectKind::Scope(error.kind()),
            Self::Selection(_) => SelectKind::Selection,
            Self::Cancelled(_) => SelectKind::Cancelled,
            Self::Other(_) => SelectKind::Other,
        };
        OperationError::from_source(kind, self)
    }
}

/// Stable classification of a selection failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectKind {
    Scope(ScopeKind),
    Selection,
    Cancelled,
    Other,
}
