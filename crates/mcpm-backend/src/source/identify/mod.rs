//! `source.identify`: identifies computed file summaries on one or more providers.
//!
//! The operation always runs in the calling process, because it owns the providers and any
//! credential in the source specifications. The file host only supplies summaries and its
//! verified cache matches, so no file is read and no cache is written here. Provider
//! lookups are scheduled with bounded concurrency and reported per (index, source); one
//! failure never stops the batch.

use std::{
    collections::VecDeque,
    future::Future,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::source::{FileSummary, Source};
use mcpm_exec::{Context, Operation, Placement};
use tracing::Instrument;

use crate::env::SurfaceEnv;

pub mod profiles;
pub mod types;
pub use profiles::{Profiles, ProfilesError, ProfilesInput, ProfilesOutput};
pub use types::{IdentifyFile, IdentifyOutcome, SourceProfile};

#[cfg(test)]
mod tests;

/// Identifies file summaries against every configured source.
#[derive(Debug)]
pub struct Identify;

crate::surface_operation!(Identify, "source.identify");

impl Operation for Identify {
    const ID: &'static str = <Self as crate::register::SurfaceOperation>::ID;
    const PLACEMENT: Placement = Placement::Surface;
    type Env = SurfaceEnv;
    type Input = IdentifyInput;
    type Event = IdentifyEvent;
    type Output = IdentifyOutput;
    type Error = IdentifyError;

    fn run(
        input: IdentifyInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move { identify(input, &cx).await }
    }
}

/// One batch of summaries and the sources to identify them against.
#[derive(Debug)]
pub struct IdentifyInput {
    /// Source specifications in output order; each may contain a credential.
    pub specifications: Vec<String>,
    pub files: Vec<IdentifyFile>,
    /// Maximum simultaneous provider lookups.
    pub parallelism: usize,
}

/// Ordered identification events; indices refer to the caller's original inputs.
#[derive(Debug)]
pub enum IdentifyEvent {
    /// Verified cache hits for one physical file, reported before provider results.
    Cache {
        indices: Vec<usize>,
        sources: Vec<usize>,
    },
    Item {
        indices: Vec<usize>,
        source: usize,
        outcome: IdentifyOutcome,
    },
}

/// Terminal totals of one identification batch.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentifyOutput {
    /// Input-index failures across every source; matched and unmatched rows are not failures.
    pub failed: usize,
}

/// Identification could not start; per-item failures are part of the events.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct IdentifyError(#[from] pub anyhow::Error);

async fn identify(
    input: IdentifyInput,
    cx: &Context<'_, Identify>,
) -> Result<IdentifyOutput, IdentifyError> {
    if input.specifications.is_empty() {
        return Err(IdentifyError(anyhow::anyhow!(
            "source identify requires at least one source"
        )));
    }
    let parallelism = input.parallelism.max(1);
    let files = input.files.len();
    let sources: Vec<Result<Arc<dyn Source>, ErrorReport>> = input
        .specifications
        .iter()
        .map(|specification| {
            let id = specification.split('@').next().unwrap_or(specification);
            tracing::debug!(source = id, "Initializing local identification provider");
            cx.env()
                .source(specification)
                .map(Arc::<dyn Source>::from)
                .map_err(|error| {
                    tracing::debug!(
                        source = id,
                        error = %format_args!("{error:#}"),
                        "Source provider initialization failed"
                    );
                    ErrorReport::capture(&error)
                })
        })
        .collect();
    tracing::info!(
        files,
        sources = sources.len(),
        parallelism,
        "Starting local provider identification"
    );
    let started = std::time::Instant::now();
    let mut queued: VecDeque<(Vec<usize>, usize, FileSummary, PathBuf)> = VecDeque::new();
    let mut failed = 0;
    for file in input.files {
        let IdentifyFile {
            indices,
            path,
            summary,
            cached,
        } = file;
        let mut hits = Vec::new();
        for (source_index, source) in sources.iter().enumerate() {
            let Ok(source) = source else {
                continue;
            };
            let Some(known) = cached.get(&source.identification_cache_id().0) else {
                continue;
            };
            let Some(file) = known.for_source(source.as_ref()) else {
                continue;
            };
            hits.push((source_index, file, known.clone()));
        }
        cx.emit(IdentifyEvent::Cache {
            indices: indices.clone(),
            sources: hits.iter().map(|(index, _, _)| *index).collect(),
        })
        .await
        .map_err(cancelled)?;
        for (source_index, file, cache) in hits {
            cx.emit(IdentifyEvent::Item {
                indices: indices.clone(),
                source: source_index,
                outcome: IdentifyOutcome::Matched {
                    file: Box::new(file),
                    cache: Box::new(cache),
                },
            })
            .await
            .map_err(cancelled)?;
        }
        for (source_index, source) in sources.iter().enumerate() {
            let hit = source
                .as_ref()
                .ok()
                .is_some_and(|source| cached.contains_key(&source.identification_cache_id().0));
            if hit {
                continue;
            }
            queued.push_back((indices.clone(), source_index, summary.clone(), path.clone()));
        }
    }
    let mut active = tokio::task::JoinSet::new();
    let mut cancellation = tokio::time::interval(Duration::from_millis(50));
    loop {
        cx.checkpoint().map_err(cancelled)?;
        while active.len() < parallelism {
            let Some((indices, source_index, summary, path)) = queued.pop_front() else {
                break;
            };
            tracing::trace!(
                source_index,
                indices = ?indices,
                active = active.len(),
                queued = queued.len(),
                "Scheduling local provider lookup"
            );
            let source = sources[source_index].clone();
            let span = tracing::debug_span!(
                "identify_source",
                path = %path.display(),
                source_index,
                source = sources[source_index]
                    .as_ref()
                    .map(|source| source.id().0.as_str())
                    .unwrap_or("unavailable")
            );
            active.spawn(
                async move {
                    tracing::debug!("Querying provider locally using file summary");
                    let result = match &source {
                        Ok(source) => source.identify_summary(&summary).await,
                        Err(error) => Err(error.clone().into_error()),
                    };
                    let outcome = match result {
                        Ok(Some(file)) => {
                            let provider: Option<&dyn Source> =
                                source.as_ref().ok().map(|source| &**source);
                            let cache = file
                                .for_cache(provider.expect("a matched lookup used a provider"))
                                .map_err(|error| {
                                    error.context("failed to prepare cache identification")
                                });
                            match cache {
                                Ok(cache) => IdentifyOutcome::Matched {
                                    file: Box::new(file),
                                    cache: Box::new(cache),
                                },
                                Err(error) => IdentifyOutcome::Failed {
                                    error: ErrorReport::capture(&error.context(format!(
                                        "failed to identify file {path:?}"
                                    ))),
                                },
                            }
                        }
                        Ok(None) => IdentifyOutcome::Unmatched,
                        Err(error) => IdentifyOutcome::Failed {
                            error: ErrorReport::capture(
                                &error.context(format!("failed to identify file {path:?}")),
                            ),
                        },
                    };
                    (indices, source_index, outcome)
                }
                .instrument(span),
            );
        }
        if active.is_empty() {
            break;
        }
        tokio::select! {
            _ = cancellation.tick() => cx.checkpoint().map_err(cancelled)?,
            result = active.join_next() => {
                let (indices, source, outcome) = result
                    .expect("active provider query exists")
                    .map_err(|error| IdentifyError(anyhow::Error::new(error)))?;
                if matches!(outcome, IdentifyOutcome::Failed { .. }) {
                    failed += indices.len();
                }
                cx.emit(IdentifyEvent::Item { indices, source, outcome })
                    .await
                    .map_err(cancelled)?;
            }
        }
    }
    tracing::debug!(
        files,
        failed,
        elapsed_ms = started.elapsed().as_millis(),
        "Local provider identification completed"
    );
    Ok(IdentifyOutput { failed })
}

fn cancelled(error: mcpm_exec::Cancelled) -> IdentifyError {
    IdentifyError(anyhow::Error::new(error))
}
