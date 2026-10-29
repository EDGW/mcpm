//! `cache refresh ident`: re-identifies cached objects and publishes only verified matches.
//!
//! The pipeline reuses [`super::identify`] on the cache content paths; the cache's own
//! verified ident snapshots become digest hits, so an unchanged match is reported as reused
//! without a provider request. Only matches resolved by a provider are written back through
//! `cache.mods.ident.write`, which validates the stored content again.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use mcpm_backend::cache::{
    IdentEntry, IdentWrite, IdentWriteEvent, IdentWriteInput, IdentWriteStatus,
};
use mcpm_backend::source::identify::IdentifyOutcome;
use mcpm_backend::source::types::SourceId;

use crate::commands::execution::failure;

use super::identify::{self, Event, Target};

/// One cached object: its content identity and the path that holds the bytes.
#[derive(Debug, Clone)]
pub(crate) struct CacheObject {
    pub key: String,
    pub path: PathBuf,
}

/// Per-source refresh result; only matches are persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// The stored snapshot was verified by the digest and no request was made.
    Reused,
    /// The provider returned a verified match, which was published.
    Matched,
    /// The provider returned no match; existing records were preserved.
    Unmatched,
}

/// Reidentifies one cached object and returns results in source order.
pub(crate) async fn refresh(
    target: Target,
    force: bool,
    specifications: Vec<String>,
    object: CacheObject,
) -> Result<Vec<(SourceId, Status)>> {
    let remote = target.session.target().is_remote();
    let mut outcomes: Vec<Option<IdentifyOutcome>> =
        (0..specifications.len()).map(|_| None).collect();
    let mut reused = vec![false; specifications.len()];
    let lookup = Target {
        session: target.session.clone(),
        cache: if force { None } else { target.cache.clone() },
    };
    let summary = identify::run(
        lookup,
        specifications.clone(),
        vec![object.path.clone()],
        1,
        |event| {
            match event {
                Event::Cache { sources, .. } => {
                    for source in sources {
                        if let Some(slot) = reused.get_mut(source) {
                            *slot = true;
                        }
                    }
                }
                Event::Item {
                    source, outcome, ..
                } => {
                    if let Some(slot) = outcomes.get_mut(source) {
                        *slot = Some(outcome);
                    }
                }
                _ => {}
            }
            Ok(())
        },
    )
    .await?;
    let mut statuses = Vec::with_capacity(summary.keys.len());
    let mut entries = Vec::new();
    for (index, key) in summary.keys.iter().enumerate() {
        let outcome = outcomes
            .get_mut(index)
            .and_then(Option::take)
            .with_context(|| {
                format!(
                    "failed to refresh source identification for {}",
                    object.key
                )
            })?;
        let status = match outcome {
            IdentifyOutcome::Matched { cache, .. } => {
                if reused[index] {
                    Status::Reused
                } else {
                    entries.push(IdentEntry {
                        key: object.key.clone(),
                        file: *cache,
                    });
                    Status::Matched
                }
            }
            IdentifyOutcome::Unmatched => Status::Unmatched,
            IdentifyOutcome::Failed { error } => {
                return Err(error.into_error().context(format!(
                    "failed to refresh source identification for {}",
                    object.key
                )));
            }
        };
        statuses.push((SourceId(key.clone()), status));
    }
    if !entries.is_empty() {
        let cache = target
            .cache
            .clone()
            .context("Mod cache is disabled for identification refresh")?;
        let mut handle = target
            .session
            .start::<IdentWrite>(IdentWriteInput {
                cache,
                entries,
                force: false,
                protect_stash: false,
            })
            .await
            .map_err(|error| failure(error, remote))?;
        while let Some(IdentWriteEvent::Item { index, status }) = handle.next().await {
            if let IdentWriteStatus::Failed { error } = status {
                return Err(error.into_error().context(format!(
                    "failed to refresh source identification for {} (entry {index})",
                    object.key
                )));
            }
        }
        let written = handle
            .finish()
            .await
            .map_err(|error| failure(error, remote))?;
        if written.failed > 0 {
            bail!(
                "failed to refresh {} source identification result(s) for {}",
                written.failed,
                object.key
            );
        }
    }
    Ok(statuses)
}
