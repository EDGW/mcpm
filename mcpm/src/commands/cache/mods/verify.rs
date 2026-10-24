//! Content-only batch verification with optional cleanup, progress and final statistics.

use super::{
    batch::{self, FailurePolicy, Parallelism},
    input::Inputs,
};
use anyhow::{Context, Result, ensure};
use clap::Args;
use mcpm_backend::local::cache::{VerificationMode, hash::Blake3Hash, mods::ModsCacheSelection};
use std::io::{self, Write};

use crate::{
    display::{
        cache::operations::{self, RemovalSummary, VerificationSummary},
        console::ConsoleWriter,
    },
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

pub(super) fn run(
    args: VerifyArgs,
    selection: ModsCacheSelection,
    out: &mut impl Write,
    headers: bool,
) -> Result<()> {
    let inputs = args.inputs.read()?;
    let parallelism = args.parallelism.selected(inputs.is_all());
    let cache = selection.open()?;
    let progress = Progress::new("Selecting cache entries...", "Verifying cached mods");
    let ids = inputs.resolve(&cache)?;
    progress.set_total(ids.len());
    let mode = if args.structure_only {
        VerificationMode::Structure
    } else {
        VerificationMode::Content
    };
    let mut first = true;
    let mut summary = VerificationSummary {
        removal: args.autoremove.then(RemovalSummary::default),
        ..Default::default()
    };
    batch::run(
        &ids,
        parallelism,
        FailurePolicy::Continue,
        |id| {
            cache
                .verify(&id, mode)
                .with_context(|| format!("failed to verify content {id}"))?
                .ok_or_else(|| MissingContent(id).into())
        },
        |id, result| {
            let status = match &result {
                Ok(()) => "available",
                Err(error) if error.is::<MissingContent>() => "not_found",
                Err(_) => "unavailable",
            };
            match result {
                Ok(()) => summary.available += 1,
                Err(error) => {
                    summary.failed += 1;
                    progress.report(&error);
                    if let Some(removal) = &mut summary.removal {
                        match cache
                            .remove(&id)
                            .with_context(|| format!("failed to automatically remove {id}"))
                        {
                            Ok(true) => removal.removed += 1,
                            Ok(false) => removal.absent += 1,
                            Err(error) => {
                                removal.failed += 1;
                                progress.report(&error);
                            }
                        }
                    }
                }
            }
            if args.details || status != "available" {
                progress.suspend(|| -> Result<()> {
                    operations::verification(out, id, status, "", headers && first)?;
                    out.flush().context("failed to flush verification results")
                })?;
                first = false;
            }
            progress.advance();
            Ok(())
        },
    )?;
    out.flush()
        .context("failed to flush verification results")?;
    drop(progress);
    operations::verification_summary(&mut ConsoleWriter::new(&mut io::stderr().lock()), &summary)?;
    ensure!(
        summary.failed == 0,
        "{} cache entries failed verification",
        summary.failed
    );
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("cached content {0} does not exist")]
struct MissingContent(Blake3Hash);
