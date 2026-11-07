//! Ordered bounded Host refresh, retaining source facts even when publication or observation fails.

use super::{CacheInput, Entry, IdentSnapshot, Selection};
use crate::operations::mods::{
    cache::PublicationFailure,
    identification::{self, QueryResult, QueryStatus, QuerySummary, Sources},
    source::Configuration,
};
use anyhow::{Context, Result};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::local::cache::{
    hash::Blake3Hash,
    mods::{ModsCacheDir, ModsCacheSelection},
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Serialize, Deserialize)]
pub struct IdentInput {
    pub cache: CacheInput,
    pub selection: Selection,
    pub sources: Vec<Configuration>,
    pub force: bool,
    pub parallelism: usize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum IdentEvent {
    Selected(usize),
    Entry {
        id: Blake3Hash,
        result: Result<IdentReceipt, PortableError>,
    },
    Done(QuerySummary),
}
#[derive(Debug, Serialize, Deserialize)]
pub struct IdentReceipt {
    pub identification: QueryResult,
    pub publication_errors: Vec<PublicationFailure>,
    pub observation: Result<IdentObservation, PortableError>,
}
impl IdentReceipt {
    pub fn overall_status(&self) -> QueryStatus {
        if !self.publication_errors.is_empty() || self.observation.is_err() {
            QueryStatus::Failed
        } else {
            self.identification.status()
        }
    }
    pub fn errors(&self) -> impl Iterator<Item = &PortableError> {
        self.identification
            .errors()
            .chain(
                self.publication_errors
                    .iter()
                    .map(|error| error.error.as_ref()),
            )
            .chain(self.observation.as_ref().err())
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct IdentObservation {
    pub content: Entry,
    pub info: IdentSnapshot,
    pub metadata_path: PathBuf,
}

pub struct Ident;
impl Operation for Ident {
    const ID: &'static str = "cache.refresh_ident";
    type Input = IdentInput;
    type Output = StreamingOutput<IdentEvent>;
    async fn run(input: IdentInput) -> Result<Self::Output> {
        let sources = Arc::new(Sources::new(input.sources)?);
        let original = input.cache.clone();
        let (cache, ids) = tokio::task::spawn_blocking(move || -> Result<_> {
            let cache = super::super::protection::open(input.cache, true, &[])?;
            let ids = input.selection.resolve(&cache)?;
            Ok((Arc::new(cache), ids))
        })
        .await??;
        Ok(StreamingOutput::new(move |sender| async move {
            sender.send(IdentEvent::Selected(ids.len())).await?;
            let mut summary = QuerySummary {
                total: ids.len(),
                ..Default::default()
            };
            let parallelism = input.parallelism.max(1);
            for window in ids.chunks(parallelism) {
                sender.checkpoint()?;
                let mut active = tokio::task::JoinSet::new();
                for (index, id) in window.iter().copied().enumerate() {
                    sender.checkpoint()?;
                    let sources = sources.clone();
                    let cache = cache.clone();
                    let original = original.clone();
                    active.spawn(async move {
                        (
                            index,
                            refresh_one(cache, original, sources, id, input.force).await,
                        )
                    });
                }
                let mut ordered: Vec<_> = (0..window.len()).map(|_| None).collect();
                while let Some(result) = active.join_next().await {
                    let (index, result) = result?;
                    ordered[index] = Some(result);
                }
                let mut failed = false;
                for (id, result) in window.iter().copied().zip(ordered) {
                    sender.checkpoint()?;
                    let result = result.expect("refresh job completed");
                    match &result {
                        Ok(receipt) => {
                            summary.record(&receipt.identification);
                            failed = receipt.overall_status() != QueryStatus::Success;
                        }
                        Err(_) => {
                            summary.preparation_failed += 1;
                            failed = true;
                        }
                    }
                    sender
                        .send(IdentEvent::Entry {
                            id,
                            result: result.map_err(Into::into),
                        })
                        .await?;
                    if failed {
                        break;
                    }
                }
                if failed {
                    break;
                }
            }
            sender.send(IdentEvent::Done(summary)).await?;
            Ok(())
        }))
    }
}

async fn refresh_one(
    cache: Arc<ModsCacheDir>,
    original: CacheInput,
    sources: Arc<Sources>,
    id: Blake3Hash,
    force: bool,
) -> Result<IdentReceipt> {
    let preparing = cache.clone();
    let requirements = sources.requirements();
    let preparing_sources = sources.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        preparing
            .verified_size(&id)?
            .context("source identification requires cached content")?;
        identification::prepare(
            &preparing.content_path(&id),
            requirements,
            preparing_sources.cache_ids(),
            Some((&preparing, &id.to_string())),
            force,
        )
    })
    .await
    .context("source refresh preparation worker failed")??;
    let identification = sources.identify(&prepared, true).await;
    let mut publication_errors = Vec::new();
    let matches = identification.new_matches();
    let directory = cache.path().to_owned();
    let finished = tokio::task::spawn_blocking(move || -> Result<_> {
        let current = reopen(original, directory)?;
        let mut failures = Vec::new();
        for (source, matched) in matches {
            if let Err(error) = matched.and_then(|matched| {
                identification::publish(&current, &id.to_string(), &prepared.summary, vec![matched])
            }) {
                failures.push(PublicationFailure {
                    source: Some(source),
                    error: Arc::new(error.into()),
                });
                break;
            }
        }
        Ok((failures, observe(&current, id)))
    })
    .await
    .context("source refresh publication worker failed")
    .and_then(|result| result);
    let observation = match finished {
        Ok((failures, observation)) => {
            publication_errors.extend(failures);
            observation.map_err(Into::into)
        }
        Err(error) => Err(error.into()),
    };
    Ok(IdentReceipt {
        identification,
        publication_errors,
        observation,
    })
}

fn reopen(original: CacheInput, directory: PathBuf) -> Result<ModsCacheDir> {
    super::super::check_protection(original.selection, true, &[], original.no_protect_stash)?;
    super::super::protection::open(
        CacheInput {
            selection: ModsCacheSelection::Directory { path: directory },
            no_protect_stash: original.no_protect_stash,
        },
        true,
        &[],
    )
}
fn observe(cache: &ModsCacheDir, id: Blake3Hash) -> Result<IdentObservation> {
    let info = cache
        .try_ident_info(id)
        .with_context(|| format!("failed to observe refreshed source identification for {id}"))?;
    Ok(IdentObservation {
        content: super::snapshot(cache, id)?,
        metadata_path: info.path().to_owned(),
        info: (&info).into(),
    })
}
