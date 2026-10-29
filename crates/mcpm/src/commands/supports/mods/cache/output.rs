//! Console/JSON rendering and exit policy for the explicit mod caching workflow.

use std::io::{self, Write};

use anyhow::Result;
use clap::Args;

use crate::{
    commands::supports::mods::{
        cache::{Event, Record, Summary},
        scan_output::ScanOutput,
    },
    diagnostics,
    display::console::ConsoleWriter,
    progress::Progress,
    terminal,
};

/// Presentation stays local even when cache operations execute over SSH.
#[derive(Debug, Args)]
pub(in crate::commands) struct DisplayArgs {
    /// Print per-file details in Console mode; Raw always emits structured file records.
    #[arg(long)]
    details: bool,
}

pub(in crate::commands) struct Presentation {
    scan: ScanOutput,
    progress: Option<Progress>,
    failed: usize,
    details: bool,
    remote: bool,
    remote_failure: bool,
    validator: super::validation::Validator,
}

impl Presentation {
    pub fn new(args: DisplayArgs) -> Self {
        Self {
            scan: ScanOutput::default(),
            progress: None,
            failed: 0,
            details: args.details,
            remote: false,
            remote_failure: false,
            validator: Default::default(),
        }
    }

    pub fn remote(args: DisplayArgs) -> Self {
        Self {
            scan: ScanOutput::remote(),
            remote: true,
            ..Self::new(args)
        }
    }

    pub fn event(&mut self, event: Event) -> Result<()> {
        self.validator.accept(&event)?;
        match event {
            Event::Selection(event) => {
                if self.remote
                    && matches!(
                        event,
                        crate::commands::supports::mods::scan::Event::Diagnostic { .. }
                    )
                {
                    self.remote_failure = true;
                }
                self.scan.event(event)?;
            }
            Event::Progress {
                stage,
                completed,
                total,
            } => {
                if completed == 0 && total > 0 {
                    let label = match stage {
                        super::Stage::Import => "Importing files",
                        super::Stage::Metadata => "Identifying Loader metadata",
                        super::Stage::Ident => "Identifying sources",
                    };
                    let progress = Progress::new("Preparing cache stage...", label);
                    progress.set_total(total);
                    self.progress = Some(progress);
                }
                if let Some(progress) = &self.progress {
                    progress.set_position(completed);
                }
                if completed == total {
                    self.progress.take();
                }
            }
            Event::StageDone { stage, success } => {
                if success && terminal::stdout_is_tty() {
                    let message = match stage {
                        super::Stage::Import => "Imported all selected mod files successfully.",
                        super::Stage::Metadata => {
                            "Identified Loader metadata for all selected mod files successfully."
                        }
                        super::Stage::Ident => {
                            "Identified sources for all selected mod files successfully."
                        }
                    };
                    crate::display::cache::write::stage_success(
                        &mut ConsoleWriter::new(&mut io::stdout().lock()),
                        message,
                    )?;
                }
            }
            Event::File(file) => {
                let source_errors: Vec<_> = file
                    .identification
                    .values()
                    .filter_map(|result| match result {
                        super::record::IdentResult::Failed { error, remote } => {
                            Some((error, *remote))
                        }
                        _ => None,
                    })
                    .collect();
                // The primary error duplicates the first source failure only for the ident stage.
                if (!matches!(file.failed_stage, Some(super::Stage::Ident))
                    || source_errors.is_empty())
                    && let Some(error) = &file.error
                {
                    let remote = file.error_remote
                        || (self.remote && !matches!(file.failed_stage, Some(super::Stage::Ident)));
                    self.report(error, remote);
                }
                for (error, remote) in source_errors {
                    self.report(error, remote);
                }
                if self.details || !terminal::stdout_is_tty() {
                    write_file(&mut io::stdout().lock(), &file)?;
                }
            }
            Event::Done(summary) => {
                self.failed = summary.failed();
                write_summary(&mut io::stdout().lock(), &summary)?;
            }
        }
        Ok(())
    }

    fn report(&mut self, error: &crate::diagnostics::ErrorReport, remote: bool) {
        self.remote_failure |= remote;
        let error = error.clone().into_error();
        let error = if remote {
            diagnostics::RemoteError::wrap(error)
        } else {
            error
        };
        match &self.progress {
            Some(progress) => progress.report(&error),
            None => diagnostics::error(&error),
        }
    }

    pub fn finish(self) -> Result<()> {
        self.validator.finish()?;
        if self.failed != 0 {
            let error = anyhow::anyhow!(
                "mod caching completed with {} file or selection failure(s)",
                self.failed
            );
            return Err(if self.remote_failure {
                diagnostics::RemoteError::wrap(error)
            } else {
                error
            });
        }
        Ok(())
    }
}

fn write_file(out: &mut impl Write, file: &Record) -> Result<()> {
    if terminal::stdout_is_tty() {
        crate::display::cache::write::file(
            &mut ConsoleWriter::new(out),
            &crate::display::cache::write::File {
                path: &file.path,
                id: file.id.as_deref(),
                published: file.published,
                method: file.method.map(|method| method.as_str()),
                failed_stage: file.failed_stage.map(|stage| stage.as_str()),
                metadata_requested: file.metadata_requested,
                metadata_status: file.status.map(|status| status.as_str()),
                reason: file.reason.as_deref(),
                identifications: file
                    .identification
                    .iter()
                    .map(|(source, result)| (source.as_str(), result.status()))
                    .collect(),
            },
        )?;
    } else {
        crate::display::cache::write::raw(out, file)?;
    }
    out.flush()?;
    Ok(())
}

fn write_summary(out: &mut impl Write, summary: &Summary) -> Result<()> {
    if terminal::stdout_is_tty() {
        crate::display::cache::write::summary(
            &mut ConsoleWriter::new(out),
            summary.files,
            summary.objects_added,
            summary.objects_reused,
            summary.failed_files,
            summary.selection_failures,
        )?;
    }
    out.flush()?;
    Ok(())
}
