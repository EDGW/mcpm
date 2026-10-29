//! Bounded cache batches with ordered results and caller-selected handling of worker failures.

use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::Result;
use clap::Args;
use mcpm_backend::local::cache::hash::Blake3Hash;

use crate::commands::execution::{blocking, default_parallelism};

#[derive(Debug, Args)]
pub(super) struct Parallelism {
    /// Maximum concurrent cache entries with --all; explicit HASH inputs stay sequential.
    #[arg(long, global = true, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
}

impl Parallelism {
    pub(super) fn selected(&self, all: bool) -> usize {
        if all { self.parallelism.get() } else { 1 }
    }
}

pub(super) fn run<T: Send>(
    ids: &[Blake3Hash],
    parallelism: usize,
    failure_policy: FailurePolicy,
    work: impl Fn(Blake3Hash) -> Result<T> + Sync,
    mut completed: impl FnMut(Blake3Hash, Result<T>) -> Result<()>,
) -> Result<()> {
    if parallelism == 1 {
        for id in ids {
            completed(*id, work(*id))?;
        }
        return Ok(());
    }
    let failed_at = AtomicUsize::new(usize::MAX);
    let mut pending: Vec<_> = (0..ids.len()).map(|_| None).collect();
    let mut next = 0;
    blocking::run(
        ids.len(),
        parallelism,
        |index| {
            if index > failed_at.load(Ordering::Acquire) {
                return None;
            }
            let result = work(ids[index]);
            if result.is_err() && matches!(failure_policy, FailurePolicy::Stop) {
                failed_at.fetch_min(index, Ordering::AcqRel);
            }
            Some(result)
        },
        || Ok(()),
        |index, result| {
            pending[index] = result;
            while next < pending.len() {
                let Some(result) = pending[next].take() else {
                    break;
                };
                completed(ids[next], result)?;
                next += 1;
            }
            Ok(())
        },
    )
}

/// Completion errors always stop the batch; this policy controls worker failures only.
#[derive(Clone, Copy)]
pub(super) enum FailurePolicy {
    Stop,
    Continue,
}

#[cfg(test)]
mod tests;
