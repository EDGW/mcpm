//! Console and Raw rendering of validated inspection events and frontend exit policy.

use std::io::{self, Write};

use anyhow::{Context, Result};
use thiserror::Error;

use super::{Display, Event, deps, info};
use crate::commands::mod_support::ignored_output;

/// Keeps diagnostics, styling, path display and final exit policy in the frontend.
pub(in crate::commands) struct Presentation<'a> {
    display: Display<'a>,
    remote: bool,
    written: bool,
    failed: usize,
    progress: Option<crate::commands::progress::Progress>,
}

impl<'a> Presentation<'a> {
    pub(in crate::commands) fn new(display: Display<'a>, remote: bool) -> Self {
        Self {
            display,
            remote,
            written: false,
            failed: 0,
            progress: None,
        }
    }

    pub(in crate::commands) fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Scan { completed, total } => {
                if completed == 0 {
                    tracing::debug!(total, "Started inspection scan progress");
                    let progress = crate::commands::progress::Progress::new(
                        "Scanning mod directory...",
                        "Identifying mods",
                    );
                    progress.set_total(total);
                    self.progress = Some(progress);
                }
                if let Some(progress) = &self.progress {
                    progress.set_position(completed);
                }
                if completed == total {
                    self.progress.take();
                    tracing::debug!(total, "Completed inspection scan progress");
                }
            }
            Event::Error { error } => {
                let error = error.into_error();
                let error = if self.remote {
                    crate::diagnostics::RemoteError::wrap(error)
                } else {
                    error
                };
                match &self.progress {
                    Some(progress) => progress.report(&error),
                    None => crate::diagnostics::error(&error),
                }
            }
            Event::File { file, .. } => {
                if matches!(self.display, Display::Info) && file.metadata.is_empty() {
                    return Ok(());
                }
                let mut stdout = io::stdout().lock();
                if self.written {
                    writeln!(stdout).context("failed to write mod inspection separator")?;
                }
                match self.display {
                    Display::Info => info::write_info(&mut stdout, &file),
                    Display::Dependencies(paths) => deps::write_deps(&mut stdout, &file, paths),
                }
                .with_context(|| format!("failed to write {}", self.display.description()))?;
                stdout
                    .flush()
                    .context("failed to flush mod inspection output")?;
                self.written = true;
            }
            Event::Done {
                failed, ignored, ..
            } => {
                if matches!(self.display, Display::Info) {
                    let mut stdout = io::stdout().lock();
                    ignored_output::write_loader_filtered(&mut stdout, ignored, &[])
                        .context("failed to write ignored mod files summary")?;
                    stdout.flush().context("failed to flush mod information")?;
                }
                self.failed = failed;
            }
        }
        Ok(())
    }

    pub(in crate::commands) fn finish(self) -> Result<()> {
        let failed = self.failed;
        if failed == 0 {
            return Ok(());
        }
        let error: anyhow::Error = match self.display {
            Display::Info => InspectionError::FailedInfo { count: failed },
            Display::Dependencies(_) => InspectionError::FailedDependencies { count: failed },
        }
        .into();
        Err(if self.remote {
            crate::diagnostics::RemoteError::wrap(error)
        } else {
            error
        })
    }
}

#[derive(Debug, Error)]
enum InspectionError {
    #[error("mod inspection completed with {count} input or identification failure(s)")]
    FailedInfo { count: usize },
    #[error("dependency inspection completed with {count} input or identification failure(s)")]
    FailedDependencies { count: usize },
}
