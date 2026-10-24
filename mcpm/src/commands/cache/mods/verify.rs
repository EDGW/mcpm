//! Content-only batch verification independent of the presence or correctness of metadata.

use super::{
    batch::{self, Parallelism},
    input::Inputs,
};
use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::cache::{VerificationMode, hash::Blake3Hash, mods::ModsCacheSelection};
use std::io::Write;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV). Arguments and redirected stdin are combined, one HASH per line. --all selects the complete cache inventory in BLAKE3 order, including orphan metadata and invalid content types; it conflicts with HASH arguments or nonblank stdin. An empty --all selection succeeds. With --all, --parallelism COUNT limits concurrent entries (default 10; positive integers only). Use 1 for sequential work. Explicit HASH inputs stay sequential. Results retain BLAKE3 order. Checks content against BLAKE3; --structure-only checks only that content is a regular file. Named metadata contents are never read or validated. A failed entry stops new work beyond it; in-flight operations finish; errors go to stderr with their cause chain."
)]
pub(super) struct VerifyArgs {
    #[command(flatten)]
    inputs: Inputs,
    #[command(flatten)]
    parallelism: Parallelism,
    /// Check content structure without calculating its BLAKE3 hash.
    #[arg(long)]
    structure_only: bool,
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
    let ids = inputs.resolve(&cache)?;
    let mode = if args.structure_only {
        VerificationMode::Structure
    } else {
        VerificationMode::Content
    };
    let mut first = true;
    batch::run(
        &ids,
        parallelism,
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
            crate::display::cache::operations::verification(out, id, status, "", headers && first)?;
            first = false;
            result
        },
    )
}

#[derive(Debug, thiserror::Error)]
#[error("cached content {0} does not exist")]
struct MissingContent(Blake3Hash);
