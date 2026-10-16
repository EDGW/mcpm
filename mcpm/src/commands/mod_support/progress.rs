//! Shared delayed scan progress and diagnostics for mod inspection.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

pub(in crate::commands) struct Progress {
    bar: ProgressBar,
    stop: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl Progress {
    pub(in crate::commands) fn new(enabled: bool) -> Self {
        let bar = ProgressBar::hidden().with_style(
            ProgressStyle::with_template("{spinner} Scanning mod directory...")
                .expect("scanning progress template must be valid"),
        );
        let (stop, worker) = if enabled {
            let (sender, receiver) = mpsc::channel();
            let delayed = bar.clone();
            let worker = thread::spawn(move || {
                if matches!(
                    receiver.recv_timeout(Duration::from_millis(500)),
                    Err(RecvTimeoutError::Timeout)
                ) {
                    delayed.set_draw_target(ProgressDrawTarget::stderr_with_hz(10));
                    delayed.enable_steady_tick(Duration::from_millis(100));
                    delayed.tick();
                }
            });
            (Some(sender), Some(worker))
        } else {
            (None, None)
        };
        Self { bar, stop, worker }
    }

    pub(in crate::commands) fn set_total(&self, total: usize) {
        self.bar.set_length(total as u64);
        self.bar.set_style(
            ProgressStyle::with_template(
                "{spinner} Identifying mods {pos}/{len} [{wide_bar}] {elapsed_precise}",
            )
            .expect("identification progress template must be valid"),
        );
    }

    pub(in crate::commands) fn advance(&self) {
        self.bar.inc(1);
    }

    pub(in crate::commands) fn report(&self, error: &anyhow::Error) {
        self.bar.suspend(|| crate::diagnostics::error(error));
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // Join before clearing so the delayed worker cannot redraw a finished bar.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.bar.finish_and_clear();
    }
}
