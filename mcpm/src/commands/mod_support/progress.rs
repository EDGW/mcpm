//! Shared immediate scan progress and diagnostics for mod inspection.

use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

#[derive(Debug)]
pub(in crate::commands) struct Progress {
    bar: ProgressBar,
    _logs: crate::logging::ProgressGuard,
}

impl Progress {
    pub(in crate::commands) fn new() -> Self {
        let bar = ProgressBar::hidden().with_style(
            ProgressStyle::with_template(&crate::theme::progress_template(
                "{spinner} Scanning mod directory...",
            ))
            .expect("scanning progress template must be valid"),
        );
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
        Self { bar, _logs: logs }
    }

    pub(in crate::commands) fn set_total(&self, total: usize) {
        self.bar.set_length(total as u64);
        self.bar.set_style(
            ProgressStyle::with_template(&crate::theme::progress_template(
                "{spinner} Identifying mods {pos}/{len} [{wide_bar}] {elapsed_precise}",
            ))
            .expect("identification progress template must be valid"),
        );
    }

    pub(in crate::commands) fn set_position(&self, position: usize) {
        self.bar.set_position(position as u64);
    }

    pub(in crate::commands) fn advance(&self) {
        self.bar.inc(1);
    }

    pub(in crate::commands) fn report(&self, error: &anyhow::Error) {
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
