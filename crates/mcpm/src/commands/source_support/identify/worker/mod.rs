//! One bounded local provider scheduler consuming summaries from the digest operation.

use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Result, ensure};
use kako_mc_lib::source::Source;
use mcpm_backend::source::types::{FileSummary, SummaryRequirements};
use mcpm_backend::mods::digest::{DigestEvent, DigestInput, DigestOutcome, JarDigest};
use tracing::Instrument;

use super::{Event, Location, Outcome};
use crate::commands::execution::failure as operation_failure;
use crate::diagnostics::ErrorReport;

pub(in crate::commands) type Sources = Vec<Result<Arc<dyn Source>, ErrorReport>>;

pub(in crate::commands) fn sources(specifications: &[String]) -> Sources {
    specifications
        .iter()
        .map(|specification| {
            let id = specification.split('@').next().unwrap_or(specification);
            tracing::debug!(source = id, "Initializing local identification provider");
            mcpm_backend::SurfaceEnv::new()
                .and_then(|environment| environment.source(specification))
                .map(Arc::<dyn Source>::from)
                .map_err(|error| {
                    tracing::debug!(source = id, error = %format_args!("{error:#}"), "Source provider initialization failed");
                    ErrorReport::capture(&error)
                })
        })
        .collect()
}

/// Prepared before spawning so a caller-owned session need not be borrowed by a task.
pub(in crate::commands) struct Identification {
    paths: Vec<PathBuf>,
    sources: Sources,
    parallelism: usize,
    requirements: SummaryRequirements,
    location: Location,
}

impl std::fmt::Debug for Identification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identification")
            .field("paths", &self.paths)
            .field("sources", &self.sources.len())
            .field("parallelism", &self.parallelism)
            .finish_non_exhaustive()
    }
}

impl Identification {
    pub(in crate::commands) fn start(
        paths: Vec<PathBuf>,
        specifications: &[String],
        parallelism: usize,
        location: Location,
    ) -> Result<Self> {
        Self::with_sources(paths, sources(specifications), parallelism, location)
    }

    #[tracing::instrument(
        name = "identify_setup",
        level = "debug",
        skip_all,
        fields(execution = "local"),
        err(level = "debug")
    )]
    pub(in crate::commands) fn with_sources(
        paths: Vec<PathBuf>,
        sources: Sources,
        parallelism: usize,
        location: Location,
    ) -> Result<Self> {
        ensure!(
            !sources.is_empty(),
            "source identify requires at least one source"
        );
        let parallelism = parallelism.max(1);
        let requirements = SummaryRequirements {
            murmur2: sources
                .iter()
                .filter_map(|source| source.as_ref().ok())
                .any(|source| source.summary_requirements().murmur2),
        };
        tracing::debug!(
            files = paths.len(),
            sources = sources.len(),
            parallelism,
            ?requirements,
            file_host = if location.is_local() {
                "local"
            } else {
                "remote"
            },
            "Prepared identification pipeline"
        );
        Ok(Self {
            paths,
            sources,
            parallelism,
            requirements,
            location,
        })
    }

    /// Provider I/O is always local, regardless of the host owning the input paths.
    pub(in crate::commands) async fn run(
        self,
        emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<()> {
        self.run_checked(|| Ok(()), emit).await
    }

    #[tracing::instrument(
        name = "source_identification",
        level = "info",
        skip_all,
        fields(execution = "local", phase = "provider"),
        err(level = "debug")
    )]
    async fn run_checked(
        self,
        checkpoint: impl Fn() -> Result<()>,
        mut emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<()> {
        let started = std::time::Instant::now();
        let remote = !self.location.is_local();
        tracing::info!(
            files = self.paths.len(),
            sources = self.sources.len(),
            parallelism = self.parallelism,
            remote_summaries = remote,
            "Starting local provider identification"
        );
        let cache = crate::commands::supports::mods::reuse::selection(self.location.environment());
        let mut digest = self
            .location
            .session()
            .start::<JarDigest>(DigestInput {
                paths: self.paths.clone(),
                requirements: self.requirements,
                parallelism: self.parallelism,
                cache,
            })
            .await
            .map_err(|failure| operation_failure(failure, remote))?;
        let mut queued: VecDeque<(Vec<usize>, usize, FileSummary)> = VecDeque::new();
        let mut active = tokio::task::JoinSet::new();
        let mut ended = false;
        let mut failed = 0;
        let mut cancellation = tokio::time::interval(Duration::from_millis(50));
        loop {
            checkpoint()?;
            while active.len() < self.parallelism {
                let Some((indices, source_index, summary)) = queued.pop_front() else {
                    break;
                };
                tracing::trace!(source_index, indices = ?indices, active = active.len(), queued = queued.len(), "Scheduling local provider lookup");
                let source = self.sources[source_index].clone();
                let path = self.paths[indices[0]].clone();
                let span = tracing::debug_span!("identify_source", path = %path.display(), source_index, source = self.sources[source_index].as_ref().map(|source| source.id().0.as_str()).unwrap_or("unavailable"));
                active.spawn(
                    async move {
                        let started = std::time::Instant::now();
                        tracing::debug!("Querying provider locally using file summary");
                        let result = match source {
                            Ok(source) => source.identify_summary(&summary).await,
                            Err(error) => Err(error.into_error()),
                        };
                        let outcome = match result {
                            Ok(Some(file)) => Outcome::Matched { file },
                            Ok(None) => Outcome::Unmatched,
                            Err(error) => Outcome::Failed {
                                remote: false,
                                error: ErrorReport::capture(
                                    &error.context(format!("failed to identify file {path:?}")),
                                ),
                            },
                        };
                        tracing::debug!(outcome = outcome.status(), elapsed_ms = started.elapsed().as_millis(), "Local provider lookup completed");
                        if let Outcome::Failed { error, .. } = &outcome {
                            tracing::debug!(error = %error.message, causes = ?error.causes, "Local provider lookup failed");
                        }
                        Event::Item {
                            indices,
                            source: source_index,
                            outcome,
                        }
                    }
                    .instrument(span),
                );
            }
            if ended && queued.is_empty() && active.is_empty() {
                break;
            }
            tokio::select! {
                _ = cancellation.tick() => checkpoint()?,
                event = digest.next(), if !ended && queued.len() < self.parallelism => {
                    match event {
                        Some(DigestEvent::Loading { indices }) => {
                            tracing::debug!(?indices, file_host = if remote { "remote" } else { "local" }, "File summary computation started");
                            for source in 0..self.sources.len() {
                                emit(Event::Loading { indices: indices.clone(), source })?;
                            }
                        }
                        Some(DigestEvent::Item { indices, outcome }) => {
                            tracing::debug!(?indices, file_host = if remote { "remote" } else { "local" }, success = matches!(outcome, DigestOutcome::Summary { .. }), "Received file summary result");
                            match outcome {
                                DigestOutcome::Summary { summary, cached } => {
                                    let hits: Vec<_> = self.sources.iter().map(|source| {
                                        source.as_ref().ok().and_then(|source| {
                                            cached.get(&source.identification_cache_id().0).and_then(|file| file.for_source(source.as_ref()))
                                        })
                                    }).collect();
                                    emit(Event::Cache {
                                        indices: indices.clone(),
                                        sources: hits.iter().enumerate().filter_map(|(index, hit)| hit.is_some().then_some(index)).collect(),
                                    })?;
                                    for (source_index, hit) in hits.into_iter().enumerate() {
                                        if let Some(file) = hit {
                                            emit(Event::Item { indices: indices.clone(), source: source_index, outcome: Outcome::Matched { file } })?;
                                        } else {
                                            queued.push_back((indices.clone(), source_index, summary.clone()));
                                        }
                                    }
                                }
                                DigestOutcome::Failed { error } => {
                                    emit(Event::Cache { indices: indices.clone(), sources: Vec::new() })?;
                                    failed += indices.len() * self.sources.len();
                                    for source in 0..self.sources.len() {
                                        emit(Event::Item { indices: indices.clone(), source, outcome: Outcome::Failed { error: error.clone(), remote } })?;
                                    }
                                }
                            }
                        }
                        Some(DigestEvent::Done { .. }) => {
                            tracing::debug!(file_host = if remote { "remote" } else { "local" }, "File summary acquisition completed; local provider queries may still be active");
                            ended = true;
                        }
                        None => ended = true,
                    }
                }
                result = active.join_next(), if !active.is_empty() => {
                    let event = result.expect("active provider query exists")?;
                    if let Event::Item { indices, outcome: Outcome::Failed { .. }, .. } = &event {
                        failed += indices.len();
                    }
                    tracing::debug!(?event, "Delivering local identification result");
                    emit(event)?;
                }
            }
        }
        emit(Event::Done { failed })?;
        tracing::debug!(
            files = self.paths.len(),
            failed,
            elapsed_ms = started.elapsed().as_millis(),
            "Local provider identification completed"
        );
        Ok(())
    }
}
