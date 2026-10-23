//! Console/JSON rendering and exit policy for the explicit mod caching workflow.

use std::io::{self, Write};

use anyhow::Result;
use clap::Args;

use crate::{
    commands::{
        mod_support::{
            caching::{Event, Record, Summary},
            scan_output::ScanOutput,
        },
        output_style::{OutputStyle, text},
    },
    diagnostics,
    progress::Progress,
    terminal,
};

/// Presentation stays local even when cache operations execute over SSH.
#[derive(Debug, Args)]
pub(in crate::commands) struct DisplayArgs {
    /// Print per-file details in Console mode; Raw always emits structured file records.
    #[arg(short = 'E', long)]
    expanded: bool,
}

pub(in crate::commands) struct Presentation {
    scan: ScanOutput,
    progress: Option<Progress>,
    failed: usize,
    expanded: bool,
    remote: bool,
    validator: super::validation::Validator,
}

impl Presentation {
    pub fn new(args: DisplayArgs) -> Self {
        Self {
            scan: ScanOutput::default(),
            progress: None,
            failed: 0,
            expanded: args.expanded,
            remote: false,
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
            Event::Selection(event) => self.scan.event(event)?,
            Event::Progress { completed, total } => {
                if completed == 0 && total > 0 {
                    let progress =
                        Progress::new("Caching mod files...", "Importing and identifying");
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
            Event::File(file) => {
                if let Some(error) = &file.error {
                    let error = error.clone().into_error();
                    let error = if self.remote {
                        diagnostics::RemoteError::wrap(error)
                    } else {
                        error
                    };
                    match &self.progress {
                        Some(progress) => progress.report(&error),
                        None => diagnostics::error(&error),
                    }
                }
                if self.expanded || !terminal::stdout_is_tty() {
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

    pub fn finish(self) -> Result<()> {
        self.validator.finish()?;
        if self.failed != 0 {
            let error = anyhow::anyhow!(
                "mod caching completed with {} file or selection failure(s)",
                self.failed
            );
            return Err(if self.remote {
                diagnostics::RemoteError::wrap(error)
            } else {
                error
            });
        }
        Ok(())
    }
}

fn write_file(out: &mut impl Write, file: &Record) -> Result<()> {
    let id = file.id.as_deref();
    let published = file.published;
    let method = file.method.map(|method| method.as_str());
    let status = file.status.map(|status| status.as_str());
    let reason = file.reason.as_deref();
    let stage = file.failed_stage.map(|stage| match stage {
        super::Stage::Import => "import",
        super::Stage::Metadata => "metadata",
    });
    if terminal::stdout_is_tty() {
        let style = OutputStyle::new();
        writeln!(
            out,
            "{}",
            style.muted(format!("File: {}", text(&file.path.to_string_lossy())))
        )?;
        if let Some(id) = id {
            writeln!(out, "  BLAKE3: {}", style.primary(id))?;
        }
        if let Some(published) = published {
            writeln!(
                out,
                "  Object: {}",
                if published { "added" } else { "reused" }
            )?;
        }
        if let Some(method) = method {
            writeln!(out, "  Method: {method}")?;
        }
        if let Some(stage) = stage {
            writeln!(out, "  Result: {}", style.danger(format!("{stage} failed")))?;
        }
        if let Some(status) = status {
            writeln!(out, "  Metadata: {status}")?;
            if let Some(reason) = reason {
                writeln!(out, "  Reason: {}", text(reason))?;
            }
        }
    } else {
        writeln!(out, "{}", serde_json::to_string(file)?)?;
    }
    out.flush()?;
    Ok(())
}

fn write_summary(out: &mut impl Write, summary: &Summary) -> Result<()> {
    if terminal::stdout_is_tty() {
        writeln!(out)?;
        writeln!(out, "{}", OutputStyle::new().muted(format!(
            "{} files · {} objects added · {} objects reused · {} files failed · {} selection failures",
            summary.files, summary.objects_added, summary.objects_reused,
            summary.failed_files, summary.selection_failures,
        )))?;
    }
    out.flush()?;
    Ok(())
}
