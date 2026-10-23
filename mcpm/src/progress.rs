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

    pub(crate) fn set_position(&self, position: usize) {
        self.bar.set_position(position as u64);
    }

    pub(crate) fn advance(&self) {
        self.bar.inc(1);
    }

    pub(crate) fn report(&self, error: &anyhow::Error) {
        crate::logging::record_error(error);
        if !crate::logging::stream::is_forwarding() {
            self.bar
                .suspend(|| crate::diagnostics::display_error(error));
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}
