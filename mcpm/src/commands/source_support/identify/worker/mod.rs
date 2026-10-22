//! One bounded local provider scheduler consuming either local or remote file summaries.

use super::{Event, Location, Outcome, remote, summary};
use crate::diagnostics::ErrorReport;
use anyhow::{Result, ensure};
use kako_mods_lib::source::{FileSummary, Source, SummaryRequirements};
use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tracing::Instrument;

pub(super) type Sources = Vec<Result<Arc<dyn Source>, ErrorReport>>;

pub(super) fn sources(specifications: &[String]) -> Sources {
    specifications.iter().map(|specification| {
        let id = specification.split('@').next().unwrap_or(specification);
        tracing::debug!(source = id, "Initializing local identification provider");
        super::super::build_source(specification).map(Arc::<dyn Source>::from).map_err(|error| {
            tracing::debug!(source = id, error = %format_args!("{error:#}"), "Source provider initialization failed");
            ErrorReport::capture(&error)
        })
    }).collect()
}

/// Prepared before spawning so a caller-owned session need not be borrowed by a task.
pub(in crate::commands) struct Identification {
    input: Input,
    paths: Vec<PathBuf>,
    sources: Sources,
    parallelism: usize,
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
        location: Location<'_>,
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
    fn with_sources(
        paths: Vec<PathBuf>,
        sources: Sources,
        parallelism: usize,
        location: Location<'_>,
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
        let input = match location {
            Location::Local => Input::Local { requirements },
            Location::Remote(session) => Input::Remote(remote::Lookup::start(
                session,
                &remote::Request {
                    paths: paths.clone(),
                    requirements,
                    parallelism: std::num::NonZeroUsize::new(parallelism).unwrap(),
                },
            )?),
        };
        Ok(Self {
            input,
            paths,
            sources,
            parallelism,
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
        let remote = matches!(self.input, Input::Remote(_));
        tracing::info!(
            files = self.paths.len(),
            sources = self.sources.len(),
            parallelism = self.parallelism,
            remote_summaries = remote,
            "Starting local provider identification"
        );
        let mut input = self.input;
        // The guard aborts local digest acquisition on every exit path.
        let mut acquisition = tokio::task::JoinSet::new();
        if let Input::Local { requirements } = input {
            let (sender, receiver) = mpsc::channel(self.parallelism);
            let paths = self.paths.clone();
            let parallelism = self.parallelism;
            acquisition.spawn(
                async move {
                    let result = summary::run(
                        paths,
                        requirements,
                        parallelism,
                        || Ok(()),
                        |event| {
                            let sender = sender.clone();
                            async move {
                                sender.send(Ok(event)).await?;
                                Ok(())
                            }
                        },
                    )
                    .await;
                    if let Err(error) = result {
                        let _ = sender.send(Err(error)).await;
                    }
                }
                .in_current_span(),
            );
            input = Input::Receiving(receiver);
        }
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
                active.spawn(async move {
                    let started = std::time::Instant::now();
                    tracing::debug!("Querying provider locally using file summary");
                    let result = match source { Ok(source) => source.identify_summary(&summary).await, Err(error) => Err(error.into_error()) };
                    let outcome = match result {
                        Ok(Some(file)) => Outcome::Matched { file },
                        Ok(None) => Outcome::Unmatched,
                        Err(error) => Outcome::Failed { remote: false, error: ErrorReport::capture(&error.context(format!("failed to identify file {path:?}"))) },
                    };
                    tracing::debug!(outcome = outcome.status(), elapsed_ms = started.elapsed().as_millis(), "Local provider lookup completed");
                    if let Outcome::Failed { error, .. } = &outcome { tracing::debug!(error = %error.message, causes = ?error.causes, "Local provider lookup failed"); }
                    Event::Item { indices, source: source_index, outcome }
                }.instrument(span));
            }
            if ended && queued.is_empty() && active.is_empty() {
                break;
            }
            tokio::select! {
                _ = cancellation.tick() => checkpoint()?,
                event = input.next(), if !ended && queued.len() < self.parallelism => {
                    match event? {
                        Some(summary::Event::Loading { indices }) => {
                            tracing::debug!(?indices, file_host = if remote { "remote" } else { "local" }, "File summary computation started");
                            for source in 0..self.sources.len() { emit(Event::Loading { indices: indices.clone(), source })?; }
                        }
                        Some(summary::Event::Item { indices, outcome }) => {
                            tracing::debug!(?indices, file_host = if remote { "remote" } else { "local" }, success = matches!(outcome, summary::Outcome::Summary { .. }), "Received file summary result");
                            match outcome {
                            summary::Outcome::Summary { summary } => {
                                for source in 0..self.sources.len() { queued.push_back((indices.clone(), source, summary.clone())); }
                            }
                            summary::Outcome::Failed { error } => {
                                failed += indices.len() * self.sources.len();
                                for source in 0..self.sources.len() { emit(Event::Item { indices: indices.clone(), source, outcome: Outcome::Failed { error: error.clone(), remote } })?; }
                            }
                            }
                        },
                        Some(summary::Event::Done { failed }) => { tracing::debug!(file_host = if remote { "remote" } else { "local" }, failed, "File summary acquisition completed; local provider queries may still be active"); },
                        None => ended = true,
                    }
                }
                result = active.join_next(), if !active.is_empty() => {
                    let event = result.expect("active provider query exists")?;
                    if let Event::Item { indices, outcome: Outcome::Failed { .. }, .. } = &event { failed += indices.len(); }
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

enum Input {
    Local { requirements: SummaryRequirements },
    Remote(remote::Lookup),
    Receiving(mpsc::Receiver<Result<summary::Event>>),
}
impl Input {
    async fn next(&mut self) -> Result<Option<summary::Event>> {
        match self {
            Self::Remote(lookup) => lookup.next().await,
            Self::Receiving(receiver) => receiver.recv().await.transpose(),
            Self::Local { .. } => unreachable!("local acquisition has been started"),
        }
    }
}
impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Local { .. } => "Local",
            Self::Remote(_) => "Remote",
            Self::Receiving(_) => "Receiving",
        })
    }
}

#[cfg(test)]
mod tests;
