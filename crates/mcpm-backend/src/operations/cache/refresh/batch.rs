//! Bounded Host work with ordered receipts and first-failure scheduling.

use anyhow::Result;
use mcpm_lib::local::cache::hash::Blake3Hash;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn run<T: Send>(
    ids: &[Blake3Hash],
    parallelism: usize,
    checkpoint: impl Fn() -> Result<()> + Sync,
    work: impl Fn(Blake3Hash) -> Result<T> + Sync,
    mut completed: impl FnMut(Blake3Hash, Result<T>) -> Result<bool>,
) -> Result<()> {
    if parallelism <= 1 {
        for id in ids {
            checkpoint()?;
            if !completed(*id, work(*id))? {
                break;
            }
        }
        return Ok(());
    }
    let failed_at = AtomicUsize::new(usize::MAX);
    let mut pending: Vec<_> = (0..ids.len()).map(|_| None).collect();
    let mut next = 0;
    let mut stopped = false;
    mcpm_lib::blocking::run(
        ids.len(),
        parallelism,
        |index| {
            if index > failed_at.load(Ordering::Acquire) {
                return None;
            }
            let result = checkpoint().and_then(|()| work(ids[index]));
            if result.is_err() {
                failed_at.fetch_min(index, Ordering::AcqRel);
            }
            Some(result)
        },
        &checkpoint,
        |index, result| {
            pending[index] = result;
            while !stopped && next < pending.len() {
                let Some(result) = pending[next].take() else {
                    break;
                };
                stopped = !completed(ids[next], result)?;
                next += 1;
            }
            Ok(())
        },
    )
}
