//! Ordered export of one cached object's bytes to absent destination files.

use crate::commands::cache_support::FileStrategy;
use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::cache::{hash::Blake3Hash, mods::ModsCacheSelection};
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

pub(super) fn run(
    args: ExportArgs,
    selection: ModsCacheSelection,
    out: &mut impl Write,
    headers: bool,
) -> Result<()> {
    let cache = selection.open()?;
    for (index, path) in args.destinations.into_iter().enumerate() {
        let method = cache
            .export_file(&args.id, &path, args.mode.into())
            .with_context(|| format!("failed to export {} to {path:?}", args.id))?;
        crate::display::cache::operations::exported(
            out,
            args.id,
            &path,
            method,
            headers && index == 0,
        )?;
    }
    Ok(())
}
