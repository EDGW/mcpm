//! Stderr-only request activity indicator for initial and noninteractive lookups.

use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

pub(super) struct Progress(ProgressBar);

impl Progress {
    pub(super) fn new(message: impl Into<String>) -> Self {
        let template = "{spinner} {msg} {elapsed_precise}";
        let bar = ProgressBar::hidden()
            .with_message(message.into())
            .with_style(
                ProgressStyle::with_template(&crate::theme::progress_template(template))
                    .expect("source request progress template must be valid"),
            );
        if crate::terminal::stderr_is_tty() {
            bar.set_draw_target(ProgressDrawTarget::stderr_with_hz(10));
            bar.enable_steady_tick(Duration::from_millis(100));
            bar.tick();
        }
        Self(bar)
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.0.finish_and_clear();
    }
}
