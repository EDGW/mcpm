//! Caller-owned terminal progress for stash host events, without library event reconstruction.

use mcpm_backend::operations::server::stash::{ProgressEvent, Stage};

use crate::{progress::Progress, terminal};

/// A completed stage releases its display; dropping on an error also clears the bar.
#[derive(Default)]
pub(crate) struct FileProgress(Option<(Stage, Progress)>);
impl FileProgress {
    pub(crate) fn event(&mut self, event: ProgressEvent) {
        if event.completed == event.total {
            self.clear();
            return;
        }
        if self.0.as_ref().map(|(stage, _)| *stage) != Some(event.stage) || event.completed == 0 {
            self.clear();
            if terminal::stderr_is_tty() {
                let label = match event.stage {
                    Stage::Checksum => "Computing file checksums",
                    Stage::Cache => "Caching source files",
                    Stage::Verify => "Verifying file contents",
                    Stage::Transfer => "Transferring files",
                };
                let progress = Progress::new(label, label);
                progress.set_total(usize::try_from(event.total).unwrap_or(usize::MAX));
                self.0 = Some((event.stage, progress));
            }
        }
        if let Some((_, progress)) = &self.0 {
            progress.set_position(usize::try_from(event.completed).unwrap_or(usize::MAX));
        }
    }
    pub(crate) fn clear(&mut self) {
        self.0 = None;
    }
}
