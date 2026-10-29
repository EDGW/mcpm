//! Shared terminal progress and coordinated diagnostics with caller-owned task labels.

use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

#[derive(Debug)]
pub(crate) struct Progress {
    bar: ProgressBar,
    task: &'static str,
    _logs: crate::logging::ProgressGuard,
}

impl Progress {
    pub(crate) fn new(discovery: &str, task: &'static str) -> Self {
        let bar = ProgressBar::hidden().with_style(
            ProgressStyle::with_template(&crate::theme::progress_template("{spinner} {msg}"))
                .expect("scanning progress template must be valid"),
        );
        bar.set_message(discovery.to_owned());
        let display = MultiProgress::with_draw_target(if crate::terminal::stderr_is_tty() {
            ProgressDrawTarget::stderr_with_hz(10)
        } else {
            ProgressDrawTarget::hidden()
        });
        let bar = display.add(bar);
        let logs = crate::logging::attach_progress(Some(display));
        if crate::terminal::stderr_is_tty() {
            bar.enable_steady_tick(Duration::from_millis(100));
            bar.tick();
        }
        Self {
            bar,
            task,
            _logs: logs,
        }
    }

    pub(crate) fn set_total(&self, total: usize) {
        self.bar.set_length(total as u64);
        self.bar.set_style(
            ProgressStyle::with_template(&crate::theme::progress_template(&format!(
                "{{spinner}} {} {{pos}}/{{len}} [{{wide_bar}}] {{elapsed_precise}}",
                self.task
            )))
            .expect("identification progress template must be valid"),
        );
    }

    /// Changes a task label without inventing an overall percentage.
    pub(crate) fn set_message(&self, message: &str) {
        self.bar.set_message(message.to_owned());
    }

    /// Resets elapsed time and speed when a new download attempt starts.
    pub(crate) fn start_bytes(&self, message: &str) {
        self.bar.reset();
        self.set_message(message);
        self.bytes(0, None);
    }

    /// Shows determinate transfer progress only while its total remains usable.
    pub(crate) fn bytes(&self, received: u64, total: Option<u64>) {
        let total = total.filter(|total| *total > 0 && received <= *total);
        let template = if let Some(total) = total {
            self.bar.set_length(total);
            "{spinner} {msg} [{wide_bar}] {percent}% {bytes}/{total_bytes} {bytes_per_sec} ETA {eta}"
        } else {
            self.bar.unset_length();
            "{spinner} {msg} {bytes} {bytes_per_sec} {elapsed_precise}"
        };
        self.bar.set_style(
            ProgressStyle::with_template(&crate::theme::progress_template(template))
                .expect("byte progress template must be valid"),
        );
        self.bar.set_position(received);
    }

    /// Returns to a phase indicator after transfer, with no stale byte percentage.
    pub(crate) fn phase(&self, message: &str) {
        self.bar.set_style(
            ProgressStyle::with_template(&crate::theme::progress_template("{spinner} {msg}"))
                .expect("phase progress template must be valid"),
        );
        self.set_message(message);
        self.bar.force_draw();
    }

    pub(crate) fn set_position(&self, position: usize) {
        self.bar.set_position(position as u64);
    }

    pub(crate) fn advance(&self) {
        self.bar.inc(1);
    }

    pub(crate) fn suspend<T>(&self, output: impl FnOnce() -> T) -> T {
        self.bar.suspend(output)
    }

    pub(crate) fn report(&self, error: &anyhow::Error) {
        crate::logging::record_error(error);
        if !crate::logging::stream::is_forwarding() {
            self.suspend(|| crate::diagnostics::display_error(error));
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}
