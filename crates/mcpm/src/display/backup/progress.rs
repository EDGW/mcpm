//! Shared file-operation progress for backup and workflows that compose its operations.

use mcpm_backend::local::mcmod::backup::{Event, Stage};

use crate::{progress::Progress, terminal};

/// Stage changes release the previous progress before creating a new display.
#[derive(Default)]
pub(crate) struct FileProgress(Option<(Stage, Progress)>);

impl FileProgress {
    pub(crate) fn event(&mut self, event: Event) {
        match event {
            Event::Progress {
                stage,
                completed,
                total,
            } => {
                if self.0.as_ref().map(|(current, _)| *current) != Some(stage) {
                    self.clear();
                    if terminal::stderr_is_tty() {
                        let label = match stage {
                            Stage::Checksum => "Computing file checksums",
                            Stage::Cache => "Caching source files",
                            Stage::Verify => "Verifying file contents",
                            Stage::Transfer => "Transferring files",
                        };
                        let progress = Progress::new(label, label);
                        progress.set_total(total);
                        self.0 = Some((stage, progress));
                    }
                }
                if let Some((_, progress)) = &self.0 {
                    progress.set_position(completed);
                }
            }
            Event::StageDone(_) => self.clear(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.0 = None;
    }
}
