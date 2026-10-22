//! Bounded blocking inspection workers; coordination, cancellation and delivery stay on the caller.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::{Result, anyhow};

/// Workers never print or mutate the cache. On cancellation no new work is accepted;
/// scoped threads finish their current synchronous file operation before returning.
pub(super) fn run<T: Send>(
    count: usize,
    parallelism: usize,
    work: impl Fn(usize) -> T + Sync,
    checkpoint: impl Fn() -> Result<()>,
    mut completed: impl FnMut(usize, T) -> Result<()>,
) -> Result<()> {
    checkpoint()?;
    let workers = parallelism.max(1).min(count);
    if workers == 0 {
        return checkpoint();
    }
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel(workers);
        for _ in 0..workers {
            let sender = sender.clone();
            let work = &work;
            let next = &next;
            let stop = &stop;
            let span = tracing::Span::current();
            scope.spawn(move || {
                let _span = span.enter();
                while !stop.load(Ordering::Acquire) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    // Report a worker panic through the coordinator instead of losing an item.
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(index)))
                            .map_err(|_| anyhow!("mod scan worker panicked at input {index}"));
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let result = (|| {
            for _ in 0..count {
                let (index, result) = loop {
                    checkpoint()?;
                    match receiver.recv_timeout(Duration::from_millis(50)) {
                        Ok(item) => break item,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            return Err(anyhow!("mod scan workers disconnected before completion"));
                        }
                    }
                };
                completed(index, result?)?;
            }
            Ok(())
        })();
        stop.store(true, Ordering::Release);
        // Unblock senders before scoped threads are joined on every exit path.
        drop(receiver);
        result
    })
}
