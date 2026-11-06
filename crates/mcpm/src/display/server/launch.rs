//! Stderr preparation stages and download progress from Local or Remote Host events.

use crate::{display::style::text, progress::Progress, terminal};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct LaunchProgress {
    progress: Option<Progress>,
    message: String,
    last_draw: Option<Instant>,
    downloading: bool,
}
impl LaunchProgress {
    pub(crate) fn host_event(&mut self, message: &str, bytes: Option<(u64, Option<u64>)>) {
        let source_changed = self.message != message;
        self.phase(message);
        if let Some((received, total)) = bytes {
            if !self.downloading || source_changed || received == 0 {
                if let Some(progress) = &self.progress {
                    progress.start_bytes(&self.message);
                }
                self.downloading = true;
                self.last_draw = None;
            }
            if self
                .last_draw
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(100))
                || received == 0
                || Some(received) == total
            {
                if let Some(progress) = &self.progress {
                    progress.bytes(received, total);
                }
                self.last_draw = Some(Instant::now());
            }
        } else {
            self.downloading = false;
        }
    }
    pub(crate) fn starting(&mut self) {
        self.phase("Starting Java");
    }
    fn phase(&mut self, message: &str) {
        if !terminal::stderr_is_tty() || console::is_dumb() || self.message == message {
            return;
        }
        self.message = text(message);
        match &self.progress {
            Some(progress) => progress.phase(&self.message),
            None => self.progress = Some(Progress::new(&self.message, "Preparing launch")),
        }
    }
}
