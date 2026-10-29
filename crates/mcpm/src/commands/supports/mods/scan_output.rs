//! Console adapter for scan events; scanning itself never owns terminal output.

use std::cell::RefCell;

use anyhow::Result;

use super::scan::Event;
use crate::progress::Progress;

#[derive(Default)]
pub(in crate::commands) struct ScanOutput {
    progress: RefCell<Option<Progress>>,
    remote: bool,
}

impl ScanOutput {
    pub(in crate::commands) fn remote() -> Self {
        Self {
            remote: true,
            ..Self::default()
        }
    }

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
            Event::Diagnostic { error } => {
                let error = error.into_error();
                let error = if self.remote {
                    crate::diagnostics::RemoteError::wrap(error)
                } else {
                    error
                };
                match active.as_ref() {
                    Some(progress) => progress.report(&error),
                    None => crate::diagnostics::error(&error),
                }
            }
        }
        Ok(())
    }
}
