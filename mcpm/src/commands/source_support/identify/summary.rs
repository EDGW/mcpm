//! File-summary acquisition shared by local identification and the remote command handler.

use anyhow::{Context, Result};
use mcpm_backend::local::cache::mods::ModsCacheSelection;
use mcpm_backend::source::FileMatch;
use mcpm_backend::source::FileSummary;
use mcpm_backend::source::SummaryRequirements;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    time::Duration,
};
use tracing::Instrument;

use crate::diagnostics::ErrorReport;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Event {
    Loading {
        indices: Vec<usize>,
    },
    Item {
        indices: Vec<usize>,
        outcome: Outcome,
    },
    Done {
        failed: usize,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Outcome {
    Summary {
        summary: FileSummary,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        cached: BTreeMap<String, FileMatch>,
    },
    Failed {
        error: ErrorReport,
    },
}

/// Files are read on their owning host, deduplicated there, and never uploaded.
#[tracing::instrument(
    name = "file_summaries",
    level = "info",
    skip_all,
    fields(phase = "summary"),
    err(level = "debug")
)]
pub(super) async fn run<F: std::future::Future<Output = Result<()>>>(
    paths: Vec<PathBuf>,
    requirements: SummaryRequirements,
    parallelism: usize,
    cache: Option<ModsCacheSelection>,
    checkpoint: impl Fn() -> Result<()>,
    mut emit: impl FnMut(Event) -> F,
) -> Result<()> {
    let lookup = cache
        .map(ModsCacheSelection::open)
        .transpose()?
        .map(std::sync::Arc::new);
    let started = std::time::Instant::now();
    let count = paths.len();
    let mut positions = HashMap::new();
    let mut jobs: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        checkpoint()?;
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
        ?requirements,
        "Computing file summaries without network requests"
    );
    let mut jobs = jobs.into_iter();
    let mut active = tokio::task::JoinSet::new();
    let mut cancellation = tokio::time::interval(Duration::from_millis(50));
    let mut failed = 0;
    loop {
        checkpoint()?;
        while active.len() < parallelism.max(1) {
            let Some((path, indices)) = jobs.next() else {
                break;
            };
            emit(Event::Loading {
                indices: indices.clone(),
            })
            .await?;
            let lookup = lookup.clone();
            let span = tracing::debug_span!("file_summary", path = %path.display());
            active.spawn(async move {
                let started = std::time::Instant::now();
                tracing::debug!(?requirements, "Reading file bytes for summary");
                let result = tokio::task::spawn_blocking(move || -> Result<_> {
                    let summary = FileSummary::read(&path, requirements).with_context(|| format!("failed to summarize file {path:?}"))?;
                    let cached = match lookup {
                        Some(lookup) => lookup.find_ident_info(&path)?.map(|info| info.verified_matches(&summary)).unwrap_or_default(),
                        None => BTreeMap::new(),
                    };
                    Ok((summary, cached))
                }).await.context("file summary worker failed").and_then(|result| result);
                let outcome = match result {
                    Ok((summary, cached)) => { tracing::debug!(size = summary.size, elapsed_ms = started.elapsed().as_millis(), "Computed file summary"); Outcome::Summary { summary, cached } },
                    Err(error) => { tracing::debug!(error = %format_args!("{error:#}"), "File summary failed"); Outcome::Failed { error: ErrorReport::capture(&error) } },
                };
                Event::Item { indices, outcome }
            }.instrument(span));
        }
        if active.is_empty() {
            break;
        }
        tokio::select! {
            _ = cancellation.tick() => checkpoint()?,
            result = active.join_next() => {
                let event = result.expect("active summary exists")?;
                if let Event::Item { indices, outcome: Outcome::Failed { .. } } = &event { failed += indices.len(); }
                emit(event).await?;
            }
        }
    }
    emit(Event::Done { failed }).await?;
    tracing::debug!(
        files = count,
        failed,
        elapsed_ms = started.elapsed().as_millis(),
        "File summaries completed"
    );
    Ok(())
}
