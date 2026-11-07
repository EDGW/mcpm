//! Shared digest options and ordered presentation for literal and server-selected files.

use std::{io, num::NonZeroUsize, sync::Arc};

use anyhow::{Context, Result, ensure};
use clap::Args;
use mcpm_backend::operations::mods::digest::{Digest, DigestEvent, DigestInput, DigestTarget};
use mcpm_exec::{error::PortableError, service::Executor};
use mcpm_lib::source::{FileSummary, SummaryRequirements};

use super::scan_output::ScanOutput;
use crate::{
    commands::execution::default_parallelism, display::digest::Presentation, progress::Progress,
};

#[derive(Debug, Args)]
pub(in crate::commands) struct Options {
    /// Also compute the CurseForge-compatible Murmur2 fingerprint.
    #[arg(long)]
    murmur2: bool,
    /// Maximum concurrent unique files, including selection indexing; use 1 for sequential processing.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
    /// Print the Raw TSV column names before the first result.
    #[arg(long)]
    headers: bool,
}

pub(in crate::commands) async fn run(
    executor: &Executor,
    target: DigestTarget,
    options: Options,
) -> Result<()> {
    let mut stream = executor
        .call::<Digest>(DigestInput {
            target,
            requirements: SummaryRequirements {
                murmur2: options.murmur2,
            },
            parallelism: options.parallelism.get(),
        })
        .await?;
    let mut display = Presentation::new(io::stdout().lock(), options.murmur2, options.headers);
    let mut paths = Vec::new();
    let mut outcomes: Vec<Option<Arc<Result<FileSummary, PortableError>>>> = Vec::new();
    let mut next = 0;
    let mut failed = None;
    let selection = ScanOutput::default();
    let mut progress: Option<Progress> = None;
    let mut completed = 0;
    while let Some(event) = stream.next().await {
        match event? {
            DigestEvent::Selection(event) => selection.event(event)?,
            DigestEvent::Loading { .. } => {}
            DigestEvent::Prepared { paths: prepared } => {
                outcomes.resize_with(prepared.len(), || None);
                paths = prepared;
                if !paths.is_empty() {
                    let active = Progress::new("Computing file digests...", "Computing digests");
                    active.set_total(paths.len());
                    progress = Some(active);
                }
            }
            DigestEvent::Item { indices, outcome } => {
                if let Err(error) = &outcome {
                    match &progress {
                        Some(progress) => progress.report(error.as_inner()),
                        None => crate::diagnostics::error(error.as_inner()),
                    }
                }
                completed += indices.len();
                if let Some(progress) = &progress {
                    progress.set_position(completed);
                }
                let outcome = Arc::new(outcome);
                for index in indices {
                    outcomes[index] = Some(Arc::clone(&outcome));
                }
                while next < outcomes.len() {
                    let Some(outcome) = outcomes[next].take() else {
                        break;
                    };
                    match &progress {
                        Some(progress) => {
                            progress.suspend(|| {
                                display.file(&paths[next], outcome.as_ref().as_ref().ok())
                            })?;
                        }
                        None => display.file(&paths[next], outcome.as_ref().as_ref().ok())?,
                    }
                    next += 1;
                }
            }
            DigestEvent::Done {
                failed: count,
                selection_failed,
                ..
            } => {
                progress.take();
                failed = Some((count, selection_failed));
            }
        }
    }
    let (failed, selection_failed) = failed.context("file digests ended without a summary")?;
    ensure!(
        next == paths.len(),
        "file digests ended before all results arrived"
    );
    ensure!(
        selection_failed == 0,
        "{selection_failed} server mod selection failures; {failed} file digests could not be computed"
    );
    ensure!(failed == 0, "{failed} file digests could not be computed");
    Ok(())
}
