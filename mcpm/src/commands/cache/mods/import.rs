//! Ordered content import; no Mod metadata is read, calculated or published.

use crate::commands::cache_support::FileStrategy;
use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::cache::mods::ModsCacheSelection;
use std::{io::Write, path::PathBuf};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV). Files are processed in input order; the first failure stops processing and completed writes remain. Import never calculates meta or ident metadata. Hard links also protect source contents; replace linked files to update them."
)]
pub(super) struct ImportArgs {
    #[arg(required = true, num_args = 1.., value_name = "PATH")]
    paths: Vec<PathBuf>,
    /// Copy, require a hard link, or copy when hard links are unsupported.
    #[arg(long, value_enum, default_value = "copy")]
    mode: FileStrategy,
}

pub(super) fn run(
    args: ImportArgs,
    selection: ModsCacheSelection,
    out: &mut impl Write,
    headers: bool,
) -> Result<()> {
    let cache = selection.open()?;
    for (index, path) in args.paths.into_iter().enumerate() {
        let outcome = cache
            .import_with_outcome(&path, args.mode.into())
            .with_context(|| format!("failed to import {path:?}"))?;
        crate::display::cache::operations::imported(
            out,
            &cache.content_path(&outcome.key),
            outcome,
            headers && index == 0,
        )?;
    }
    Ok(())
}
