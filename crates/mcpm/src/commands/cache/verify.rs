//! Content-only batch verification with optional cleanup, progress and final statistics.

use super::{batch::Parallelism, input::Inputs};
use anyhow::{Context, Result, ensure};
use clap::Args;
use mcpm_backend::operations::cache::{ContentStatus, Verify, VerifyEvent, VerifyInput};
use std::io::{self, Write};

use crate::{
    display::{cache::operations, console::ConsoleWriter},
    progress::Progress,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV; --raw or redirected stdout).\n\nArguments and redirected stdin are combined, one HASH per line. --all selects the complete cache inventory in BLAKE3 order, including orphan metadata and invalid content types; it conflicts with HASH arguments or nonblank stdin. An empty --all selection succeeds. With --all, --parallelism COUNT limits concurrent entries (default 10; positive integers only). Use 1 for sequential work. Explicit HASH inputs stay sequential. Results retain selection order.\nChecks content against BLAKE3; --structure-only checks only that content is a regular file. Named metadata contents are never read or validated. By default, only unavailable and not_found entries are shown. --details also shows available entries in Console and Raw. --headers adds a Raw header when a result is shown.\nEvery selected entry is checked even if verification or automatic removal fails. --autoremove removes failed entries and their associated metadata using the same behavior as remove; exported copies and hard links remain. Progress is shown on terminal stderr unless --raw is set. Statistics are written to stderr after processing, including removal counts with --autoremove. Errors go to stderr with their cause chain. Any verification failure returns a nonzero exit code, including failures whose entries were removed."
)]
pub(super) struct VerifyArgs {
    #[command(flatten)]
    inputs: Inputs,
    #[command(flatten)]
    parallelism: Parallelism,
    /// Check content structure without calculating its BLAKE3 hash.
    #[arg(long)]
    structure_only: bool,
    /// Show every verification result, including available entries (Console and Raw).
    #[arg(long)]
    details: bool,
    /// Remove entries that fail verification, including their associated metadata.
    #[arg(long)]
    autoremove: bool,
}

pub(super) async fn run(
    args: VerifyArgs,
    context: &super::Context,
    out: &mut impl Write,
) -> Result<()> {
    let inputs = args.inputs.read()?;
    let parallelism = std::num::NonZeroUsize::new(args.parallelism.selected(inputs.is_all()))
        .expect("positive parallelism");
    let mut stream = context
        .executor()
        .await?
        .call::<Verify>(VerifyInput {
            cache: context.cache.clone(),
            selection: inputs.into(),
            parallelism,
            structure_only: args.structure_only,
            autoremove: args.autoremove,
        })
        .await?;
    let progress = Progress::new("Selecting cache entries...", "Verifying cached mods");
    let mut first = true;
    let mut summary = None;
    while let Some(event) = stream.next().await {
        match event? {
            VerifyEvent::Selected { total } => progress.set_total(total),
            VerifyEvent::Item(item) => {
                if let Some(error) = item.error {
                    progress.report(&error.into_inner());
                }
                if let Some(Err(error)) = item.removal {
                    progress.report(&error.into_inner());
                }
                if args.details || item.status != ContentStatus::Available {
                    progress.suspend(|| -> Result<()> {
                        operations::verification(
                            out,
                            item.id,
                            item.status.as_str(),
                            "",
                            context.headers && first,
                        )?;
                        out.flush().context("failed to flush verification results")
                    })?;
                    first = false;
                }
                progress.advance();
            }
            VerifyEvent::Finished(result) => summary = Some(result),
        }
    }
    drop(progress);
    let summary = summary.context("cache verification ended without a summary")?;
    operations::verification_summary(&mut ConsoleWriter::new(&mut io::stderr().lock()), &summary)?;
    ensure!(
        summary.failed == 0,
        "{} cache entries failed verification",
        summary.failed
    );
    Ok(())
}
