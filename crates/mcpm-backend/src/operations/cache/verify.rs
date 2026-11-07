//! Ordered content verification with independent item failures and optional cache-owned cleanup.

use anyhow::{Context, Result};
use mcpm_exec::operation::{Operation, StreamingOutput};
use mcpm_lib::local::cache::{VerificationMode, hash::Blake3Hash, mods::ModsCacheDir};

use super::*;

/// Verify content only, continuing after corrupt/missing objects and cleanup failures.
pub struct Verify;
impl Operation for Verify {
    const ID: &'static str = "cache.verify";
    type Input = VerifyInput;
    type Output = StreamingOutput<VerifyEvent>;
    async fn run(input: VerifyInput) -> Result<Self::Output> {
        Ok(stream::blocking_stream(move |sender| {
            let cache = protection::open(input.cache, input.autoremove, &[])?;
            let parallelism = if matches!(&input.selection, Selection::All) {
                input.parallelism.get()
            } else {
                1
            };
            let ids = input.selection.resolve(&cache)?;
            sender.send(VerifyEvent::Selected { total: ids.len() })?;
            let mode = if input.structure_only {
                VerificationMode::Structure
            } else {
                VerificationMode::Content
            };
            let mut summary = VerificationSummary {
                removal: input.autoremove.then(RemovalSummary::default),
                ..Default::default()
            };
            let mut completed = |id, result: Result<()>| -> Result<()> {
                sender.checkpoint()?;
                let status = match &result {
                    Ok(()) => ContentStatus::Available,
                    Err(error) if error.is::<MissingContent>() => ContentStatus::NotFound,
                    Err(_) => ContentStatus::Unavailable,
                };
                let error = result.err();
                let removal = if error.is_some() {
                    summary.failed += 1;
                    if let Some(stats) = &mut summary.removal {
                        let result = cache
                            .remove(&id)
                            .with_context(|| format!("failed to automatically remove {id}"));
                        match &result {
                            Ok(true) => stats.removed += 1,
                            Ok(false) => stats.absent += 1,
                            Err(_) => stats.failed += 1,
                        }
                        Some(result.map_err(Into::into))
                    } else {
                        None
                    }
                } else {
                    summary.available += 1;
                    None
                };
                sender.send(VerifyEvent::Item(VerificationItem {
                    id,
                    status,
                    error: error.map(Into::into),
                    removal,
                }))
            };
            if parallelism == 1 {
                for id in ids {
                    sender.checkpoint()?;
                    completed(id, verify_one(&cache, id, mode))?;
                }
            } else {
                let mut pending: Vec<Option<Result<()>>> = (0..ids.len()).map(|_| None).collect();
                let mut next = 0;
                mcpm_lib::blocking::run(
                    ids.len(),
                    parallelism,
                    |index| verify_one(&cache, ids[index], mode),
                    || sender.checkpoint(),
                    |index, result| {
                        pending[index] = Some(result);
                        while next < pending.len() {
                            let Some(result) = pending[next].take() else {
                                break;
                            };
                            completed(ids[next], result)?;
                            next += 1;
                        }
                        Ok(())
                    },
                )?;
            }
            sender.send(VerifyEvent::Finished(summary))
        }))
    }
}

fn verify_one(cache: &ModsCacheDir, id: Blake3Hash, mode: VerificationMode) -> Result<()> {
    cache
        .verify(&id, mode)
        .with_context(|| format!("failed to verify content {id}"))?
        .ok_or_else(|| MissingContent(id).into())
}
#[derive(Debug, thiserror::Error)]
#[error("cached content {0} does not exist")]
struct MissingContent(Blake3Hash);
