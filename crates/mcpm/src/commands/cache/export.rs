//! Ordered export of one cached object's bytes to absent destination files.

use crate::commands::cache_support::FileStrategy;
use anyhow::Result;
use clap::Args;
use mcpm_backend::operations::cache::{Export, ExportInput};
use mcpm_lib::local::cache::hash::Blake3Hash;
use std::{io::Write, path::PathBuf};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV). Destinations are processed in input order; the first failure stops processing and completed exports remain. No overwrite or parent-directory creation. Export checks content against BLAKE3 and does not require metadata."
)]
pub(super) struct ExportArgs {
    #[arg(value_name = "HASH")]
    id: Blake3Hash,
    #[arg(required = true, num_args = 1.., value_name = "PATH")]
    destinations: Vec<PathBuf>,
    /// Copy, require a hard link, or copy when hard links are unsupported.
    #[arg(long, value_enum, default_value = "copy")]
    mode: FileStrategy,
}

pub(super) async fn run(
    args: ExportArgs,
    context: &super::Context,
    out: &mut impl Write,
) -> Result<()> {
    let mut stream = context
        .executor()
        .await?
        .call::<Export>(ExportInput {
            cache: context.cache.clone(),
            id: args.id,
            destinations: args.destinations,
            mode: args.mode.into(),
        })
        .await?;
    let mut first = true;
    while let Some(receipt) = stream.next().await {
        let receipt = receipt?;
        crate::display::cache::operations::exported(
            out,
            receipt.id,
            &receipt.destination,
            receipt.method.into(),
            context.headers && first,
        )?;
        out.flush()?;
        first = false;
    }
    Ok(())
}
