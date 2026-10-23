//! Bounded blocking command workers with caller-owned cancellation and result delivery.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::{Result, anyhow};

/// Workers perform caller-supplied work without printing. On cancellation no new work is accepted;
/// scoped threads finish their current synchronous file operation before returning.
pub(in crate::commands) fn run<T: Send>(
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
                            .map_err(|_| anyhow!("blocking worker panicked at input {index}"));
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
                            return Err(anyhow!("blocking workers disconnected before completion"));
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

#[cfg(test)]
mod tests {
    use super::run;
    use anyhow::Result;
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };

    #[test]
    fn bounded_workers_deliver_later_completion_without_waiting_for_first_input() {
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let (began, running) = mpsc::channel();
        let running = Mutex::new(running);
        let active = AtomicUsize::new(0);
        let maximum = AtomicUsize::new(0);
        let mut order = Vec::new();
        run(
            8,
            2,
            |index| {
                let concurrency = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(concurrency, Ordering::SeqCst);
                if index == 0 {
                    began.send(()).unwrap();
                    // Timeout avoids hanging the suite if scheduling accidentally becomes serial.
                    blocked
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3))
                        .unwrap();
                }
                if index == 1 {
                    running
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3))
                        .unwrap();
                }
                active.fetch_sub(1, Ordering::SeqCst);
                index
            },
            || Ok(()),
            |index, result| {
                assert_eq!(index, result);
                order.push(index);
                if index == 1 {
                    release.send(()).unwrap();
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(maximum.load(Ordering::SeqCst), 2);
        assert_ne!(order[0], 0);
        order.sort_unstable();
        assert_eq!(order, (0..8).collect::<Vec<_>>());

        // Consumer/transport errors stop the batch without blocked channel senders.
        let failure = run(
            100,
            2,
            |index| index,
            || Ok(()),
            |_, _| -> Result<()> { anyhow::bail!("consumer closed") },
        );
        assert!(failure.unwrap_err().to_string().contains("consumer closed"));

        let cancelled = std::cell::Cell::new(false);
        let started = AtomicUsize::new(0);
        let failure = run(
            100,
            2,
            |_| {
                started.fetch_add(1, Ordering::SeqCst);
            },
            || {
                if cancelled.get() {
                    anyhow::bail!("cancelled");
                }
                Ok(())
            },
            |_, _| {
                cancelled.set(true);
                Ok(())
            },
        );
        assert!(failure.unwrap_err().to_string().contains("cancelled"));
        assert!(
            started.load(Ordering::SeqCst) <= 6,
            "cancellation must stop dispatch rather than drain all work"
        );
    }
}
