//! Shared validation for sequential scan batches, independent of event envelopes.

use anyhow::{Result, ensure};

/// Each scan starts at zero, advances once per input, and finishes at its total.
#[derive(Default)]
pub(in crate::commands) struct ProgressState {
    active: Option<(usize, usize)>,
}
impl ProgressState {
    pub(in crate::commands) fn accept(&mut self, completed: usize, total: usize) -> Result<()> {
        ensure!(completed <= total, "scan progress exceeds total");
        match self.active {
            None => ensure!(completed == 0, "scan progress missing start"),
            Some((previous, expected)) => ensure!(
                total == expected && completed == previous + 1,
                "invalid scan progress sequence"
            ),
        }
        self.active = (completed < total).then_some((completed, total));
        Ok(())
    }

    pub(in crate::commands) fn is_complete(&self) -> bool {
        self.active.is_none()
    }
}
