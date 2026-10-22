//! Console adapter for scan events; scanning itself never owns terminal output.

use std::cell::RefCell;

use anyhow::Result;

use super::scan::Event;
use crate::commands::progress::Progress;

#[derive(Default)]
pub(in crate::commands) struct ScanOutput {
    progress: RefCell<Option<Progress>>,
}

impl ScanOutput {
    pub(in crate::commands) fn event(&self, event: Event) -> Result<()> {
        let mut active = self.progress.borrow_mut();
        match event {
            Event::Progress { completed, total } => {
                if completed == 0 {
                    let progress = Progress::new("Scanning mod directory...", "Identifying mods");
                    progress.set_total(total);
                    *active = Some(progress);
                }
                if let Some(progress) = active.as_ref() {
                    progress.set_position(completed);
                }
                if completed == total {
                    active.take();
                }
            }
            Event::Diagnostic { error } => match active.as_ref() {
                Some(progress) => progress.report(&error.into_error()),
                None => crate::diagnostics::error(&error.into_error()),
            },
        }
        Ok(())
    }
}
