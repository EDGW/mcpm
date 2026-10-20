//! Shared immediate scan progress and diagnostics for mod inspection.

use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

pub(in crate::commands) struct Progress {
    bar: ProgressBar,
}

impl Progress {
    pub(in crate::commands) fn new() -> Self {
        let bar = ProgressBar::hidden().with_style(
            ProgressStyle::with_template(&crate::theme::progress_template(
                "{spinner} Scanning mod directory...",
            ))
            .expect("scanning progress template must be valid"),
        );
        if crate::terminal::stderr_is_tty() {
            bar.set_draw_target(ProgressDrawTarget::stderr_with_hz(10));
            bar.enable_steady_tick(Duration::from_millis(100));
            bar.tick();
        }
        Self { bar }
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
        self.bar.suspend(|| crate::diagnostics::error(error));
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}
