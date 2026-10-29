//! `mods.jar.digest`: computes file summaries on the host that owns the bytes.
//!
//! Provider requests stay with the surface: the operation reads bytes, optionally reuses
//! verified cache matches, and reports one outcome per input index. Paths are deduplicated
//! before reading, so duplicate inputs cost one read.

use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context as _, Result};
use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::source::{FileMatch, FileSummary, SummaryRequirements};
use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};
use tracing::Instrument;

use crate::data::cache::mods::ModsCacheSelection;
use crate::data::runtime::RuntimeEnvironment;
use crate::error::OperationError;

/// Computes portable file summaries without contacting any provider.
#[derive(Debug)]
pub struct JarDigest;

crate::host_operation!(JarDigest, "mods.jar.digest");

impl Operation for JarDigest {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = DigestInput;
    type Event = DigestEvent;
    type Output = DigestSummary;
    type Error = OperationError<DigestKind>;

    fn run(
        input: DigestInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            digest(input, &cx)
                .await
                .map_err(|error: DigestFailure| error.into_operation())
        }

    }
}

/// Paths, digest requirements and the cache used for verified reuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestInput {
    pub paths: Vec<PathBuf>,
    pub requirements: SummaryRequirements,
    #[serde(default = "default_parallelism")]
    pub parallelism: usize,
    /// `None` disables cache lookup; the default cache stays read-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<ModsCacheSelection>,
}

/// One physical file's summary, shared by several input indices.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum DigestOutcome {
    Summary {
        summary: FileSummary,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        cached: BTreeMap<String, FileMatch>,
    },
    Failed {
        error: ErrorReport,
    },
}

/// Ordered summary events; indices refer to the original input positions.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum DigestEvent {
    Loading { indices: Vec<usize> },
    Item { indices: Vec<usize>, outcome: DigestOutcome },
    Done { failed: usize },
}

/// Terminal totals of one digest call.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestSummary {
    pub failed: usize,
}

fn default_parallelism() -> usize {
    crate::mods::DEFAULT_PARALLELISM
}

async fn digest(
    input: DigestInput,
    cx: &Context<'_, JarDigest>,
) -> std::result::Result<DigestSummary, DigestFailure> {
    if input.paths.is_empty() {
        return Err(DigestFailure::Input(anyhow::anyhow!(
            "source identify requires at least one path"
        )));
    }
    let environment = RuntimeEnvironment::new().with_mods_cache(input.cache);
    environment
        .mods_cache()
        .map_err(DigestFailure::Cache)?;
    if input.parallelism == 0 {
        return Err(DigestFailure::Input(anyhow::anyhow!(
            "parallelism must be positive"
        )));
    }
    let parallelism = input.parallelism;
    let started = std::time::Instant::now();
    let count = input.paths.len();
    let mut positions = HashMap::new();
    let mut jobs: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for (index, path) in input.paths.into_iter().enumerate() {
        cx.checkpoint().map_err(DigestFailure::Cancelled)?;
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let position = *positions.entry(key).or_insert_with(|| {
            jobs.push((path, Vec::new()));
            jobs.len() - 1
        });
        jobs[position].1.push(index);
    }
    tracing::info!(
        files = count,
        unique_files = jobs.len(),
        parallelism,
        requirements = ?input.requirements,
        "Computing file summaries without network requests"
    );
    let mut jobs = jobs.into_iter();
    let mut active = tokio::task::JoinSet::new();
    let mut cancellation = tokio::time::interval(Duration::from_millis(50));
    let mut failed = 0;
    loop {
        cx.checkpoint().map_err(DigestFailure::Cancelled)?;
        while active.len() < parallelism {
            let Some((path, indices)) = jobs.next() else {
                break;
            };
            cx.emit(DigestEvent::Loading {
                indices: indices.clone(),
            })
            .await
            .map_err(DigestFailure::Cancelled)?;
            let environment = environment.clone();
            let requirements = input.requirements;
            let span = tracing::debug_span!("file_summary", path = %path.display());
            active.spawn(
                async move {
                    let started = std::time::Instant::now();
                    tracing::debug!(?requirements, "Reading file bytes for summary");
                    let result = tokio::task::spawn_blocking(move || -> Result<_> {
                        let summary = FileSummary::read(&path, requirements)
                            .with_context(|| format!("failed to summarize file {path:?}"))?;
                        let cached = match environment.mods_cache()? {
                            Some(lookup) => lookup
                                .find_ident_info(&path)?
                                .map(|info| info.verified_matches(&summary))
                                .unwrap_or_default(),
                            None => BTreeMap::new(),
                        };
                        Ok((summary, cached))
                    })
                    .await
                    .context("file summary worker failed")
                    .and_then(|result| result);
                    let outcome = match result {
                        Ok((summary, cached)) => {
                            tracing::debug!(
                                size = summary.size,
                                elapsed_ms = started.elapsed().as_millis(),
                                "Computed file summary"
                            );
                            DigestOutcome::Summary { summary, cached }
                        }
                        Err(error) => {
                            tracing::debug!(error = %format_args!("{error:#}"), "File summary failed");
                            DigestOutcome::Failed {
                                error: ErrorReport::capture(&error),
                            }
                        }
                    };
                    (indices, outcome)
                }
                .instrument(span),
            );
        }
        if active.is_empty() {
            break;
        }
        tokio::select! {
            _ = cancellation.tick() => cx.checkpoint().map_err(DigestFailure::Cancelled)?,
            result = active.join_next() => {
                let (indices, outcome) = result
                    .expect("active summary exists")
                    .map_err(|error| DigestFailure::Other(error.into()))?;
                if matches!(outcome, DigestOutcome::Failed { .. }) {
                    failed += indices.len();
                }
                cx.emit(DigestEvent::Item { indices, outcome })
                    .await
                    .map_err(DigestFailure::Cancelled)?;
            }
        }
    }
    cx.emit(DigestEvent::Done { failed })
        .await
        .map_err(DigestFailure::Cancelled)?;
    tracing::debug!(
        files = count,
        failed,
        elapsed_ms = started.elapsed().as_millis(),
        "File summaries completed"
    );
    Ok(DigestSummary { failed })
}

#[derive(Debug, thiserror::Error)]
enum DigestFailure {
    #[error("source identify requires at least one path")]
    Input(#[source] anyhow::Error),
    #[error("failed to resolve the Mod cache")]
    Cache(#[source] anyhow::Error),
    #[error("file summary computation was cancelled")]
    Cancelled(mcpm_exec::Cancelled),
    #[error(transparent)]
    Other(anyhow::Error),
}

impl DigestFailure {
    fn into_operation(self) -> OperationError<DigestKind> {
        let kind = match &self {
            Self::Input(_) => DigestKind::Input,
            Self::Cache(_) => DigestKind::Cache,
            Self::Cancelled(_) => DigestKind::Cancelled,
            Self::Other(_) => DigestKind::Other,
        };
        OperationError::from_source(kind, self)
    }
}

/// Stable classification of a digest failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestKind {
    Input,
    Cache,
    Cancelled,
    Other,
}
