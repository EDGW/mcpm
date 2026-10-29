//! The identification pipeline: host summaries and surface provider lookups overlap.
//!
//! `mods.jar.digest` streams one summary per physical file from the file host. The flow
//! collects summaries into one bounded batch, starts `source.identify` for that batch on
//! the calling process, and keeps consuming the next batch while the provider lookups
//! run, so digest and identification overlap without per-file operation round trips. At
//! most one identification batch and one pending batch are buffered; a full pending batch
//! stops digest polling, leaving backpressure to the digest event queue.

use std::path::PathBuf;

use anyhow::Result;
use mcpm_backend::data::cache::mods::ModsCacheSelection;
use mcpm_backend::mods::digest::{DigestEvent, DigestInput, DigestOutcome, JarDigest};
use mcpm_backend::source::identify::{
    Identify, IdentifyEvent, IdentifyFile, IdentifyInput, IdentifyOutcome, Profiles, ProfilesInput,
    SourceProfile,
};
use mcpm_backend::source::types::SummaryRequirements;
use mcpm_backend::Session;
use mcpm_exec::{Handle, Operation};

use crate::commands::execution::{failure, surface_failure};

pub(crate) use mcpm_backend::source::identify::IdentifyOutcome as Outcome;

/// The file host and the cache policy shared by every identification caller.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    /// Host that owns the bytes; summaries are computed there.
    pub session: Session,
    /// Cache used only for verified reuse; `None` identifies without lookup.
    pub cache: Option<ModsCacheSelection>,
}

impl Target {
    pub(crate) fn is_local(&self) -> bool {
        !self.session.target().is_remote()
    }
}

/// Ordered identification events; indices refer to the caller's original inputs.
#[derive(Debug)]
pub(crate) enum Event {
    /// Verified cache hits for one physical file, before provider results.
    Cache {
        indices: Vec<usize>,
        sources: Vec<usize>,
    },
    /// A summary is being computed on the file host, once per source.
    Loading {
        indices: Vec<usize>,
        source: usize,
    },
    Item {
        indices: Vec<usize>,
        source: usize,
        outcome: IdentifyOutcome,
        /// The failure was observed on the file host, not in this process.
        remote: bool,
    },
    /// All results were delivered; the total counts input-index failures.
    Done { failed: usize },
}

impl From<IdentifyEvent> for Event {
    fn from(event: IdentifyEvent) -> Self {
        match event {
            IdentifyEvent::Cache { indices, sources } => Self::Cache { indices, sources },
            IdentifyEvent::Item {
                indices,
                source,
                outcome,
            } => Self::Item {
                indices,
                source,
                outcome,
                remote: false,
            },
        }
    }
}

/// Per-source cache identities of one pipeline run.
#[derive(Debug)]
pub(crate) struct Summary {
    /// Cache identity of each source in request order, for later publication.
    pub keys: Vec<String>,
}

/// Cache identities and summary requirements of the requested sources, without a request.
pub(crate) async fn profiles(
    session: &Session,
    specifications: &[String],
) -> Result<Vec<SourceProfile>> {
    let mut handle = session
        .start::<Profiles>(ProfilesInput {
            specifications: specifications.to_vec(),
        })
        .await
        .map_err(surface_failure)?;
    while handle.next().await.is_some() {}
    let output = handle.finish().await.map_err(surface_failure)?;
    Ok(output.profiles)
}

/// Waits for one event, parking forever when the call has already finished.
///
/// `tokio::select!` builds every branch future before it checks the branch guards, so a
/// guarded branch must tolerate a finished call instead of unwrapping it.
async fn next_event<O: Operation>(handle: &mut Option<Handle<O>>) -> Option<O::Event> {
    match handle {
        Some(handle) => handle.next().await,
        None => std::future::pending().await,
    }
}

/// Runs digest and identification as one bounded pipeline over a batch of paths.
///
/// Digest failures become per-source item failures, exactly like provider failures; the
/// flow itself only fails when an operation could not be dispatched or the caller stopped
/// consuming events. Dropping this future drops both handles, which cancels both calls.
pub(crate) async fn run(
    target: Target,
    specifications: Vec<String>,
    paths: Vec<PathBuf>,
    parallelism: usize,
    mut emit: impl FnMut(Event) -> Result<()>,
) -> Result<Summary> {
    enum Step {
        Digest(Option<DigestEvent>),
        Identify(Option<IdentifyEvent>),
    }

    let sources = specifications.len();
    let parallelism = parallelism.max(1);
    let profiles = profiles(&target.session, &specifications).await?;
    let keys: Vec<String> = profiles
        .iter()
        .map(|profile| profile.cache_key.clone())
        .collect();
    let requirements = SummaryRequirements {
        murmur2: profiles
            .iter()
            .any(|profile| profile.error.is_none() && profile.requirements.murmur2),
    };
    let Target { session, cache } = target;
    let remote = session.target().is_remote();
    // One identification batch holds at most this many files; a bounded next batch is
    // collected while the current one runs. At least two files per batch so even
    // `--parallelism 1` with one source never turns into per-file operation round trips.
    let batch_size = parallelism.max(sources).max(2);
    tracing::info!(
        files = paths.len(),
        sources,
        parallelism,
        batch_size,
        remote,
        "Preparing source identification pipeline"
    );
    let mut digest: Option<Handle<JarDigest>> = Some(
        session
            .start::<JarDigest>(DigestInput {
                paths: paths.clone(),
                requirements,
                parallelism,
                cache,
            })
            .await
            .map_err(|error| failure(error, remote))?,
    );
    let mut identify: Option<Handle<Identify>> = None;
    let mut pending: Vec<IdentifyFile> = Vec::new();
    let mut digest_done = false;
    let mut failed = 0;
    loop {
        if identify.is_none()
            && !pending.is_empty()
            && (pending.len() >= batch_size || digest_done)
        {
            let files = std::mem::take(&mut pending);
            tracing::debug!(
                files = files.len(),
                digest_running = digest.is_some() && !digest_done,
                "Starting an identification batch"
            );
            identify = Some(
                session
                    .start::<Identify>(IdentifyInput {
                        specifications: specifications.clone(),
                        files,
                        parallelism,
                    })
                    .await
                    .map_err(surface_failure)?,
            );
            continue;
        }
        if digest.is_none() && identify.is_none() && pending.is_empty() {
            break;
        }
        let step = tokio::select! {
            // Stop polling the digest while a whole batch is waiting for the running
            // identification; its bounded event queue then applies backpressure.
            event = next_event(&mut digest), if digest.is_some() && pending.len() < batch_size => Step::Digest(event),
            event = next_event(&mut identify), if identify.is_some() => Step::Identify(event),
        };
        match step {
            Step::Digest(Some(DigestEvent::Loading { indices })) => {
                for source in 0..sources {
                    emit(Event::Loading {
                        indices: indices.clone(),
                        source,
                    })?;
                }
            }
            Step::Digest(Some(DigestEvent::Item { indices, outcome })) => match outcome {
                DigestOutcome::Summary { summary, cached } => {
                    let path = indices
                        .first()
                        .and_then(|index| paths.get(*index))
                        .cloned()
                        .unwrap_or_default();
                    pending.push(IdentifyFile {
                        indices,
                        path,
                        summary,
                        cached,
                    });
                }
                DigestOutcome::Failed { error } => {
                    failed += indices.len() * sources;
                    emit(Event::Cache {
                        indices: indices.clone(),
                        sources: Vec::new(),
                    })?;
                    for source in 0..sources {
                        emit(Event::Item {
                            indices: indices.clone(),
                            source,
                            outcome: IdentifyOutcome::Failed {
                                error: error.clone(),
                            },
                            remote,
                        })?;
                    }
                }
            },
            Step::Digest(Some(DigestEvent::Done { .. })) => {
                digest_done = true;
            }
            Step::Digest(None) => {
                let handle = digest.take().expect("the finished digest is still owned");
                handle
                    .finish()
                    .await
                    .map_err(|error| failure(error, remote))?;
                digest_done = true;
            }
            Step::Identify(Some(event)) => emit(Event::from(event))?,
            Step::Identify(None) => {
                let handle = identify.take().expect("the finished identification is owned");
                let output = handle.finish().await.map_err(surface_failure)?;
                failed += output.failed;
            }
        }
    }
    emit(Event::Done { failed })?;
    tracing::debug!(
        files = paths.len(),
        failed,
        "Source identification pipeline completed"
    );
    Ok(Summary { keys })
}
