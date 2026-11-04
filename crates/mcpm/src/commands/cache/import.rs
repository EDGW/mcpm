//! Ordered content import; no Mod metadata is read, calculated or published.

use crate::commands::cache_support::FileStrategy;
use anyhow::Result;
use clap::Args;
use mcpm_backend::operations::cache::{Import, ImportInput};
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

pub(super) async fn run(
    args: ImportArgs,
    context: &super::Context,
    out: &mut impl Write,
) -> Result<()> {
    let mut stream = context
        .executor()
        .await?
        .call::<Import>(ImportInput {
            cache: context.cache.clone(),
            paths: args.paths,
            mode: args.mode.into(),
        })
        .await?;
    let mut first = true;
    while let Some(receipt) = stream.next().await {
        let receipt = receipt?;
        crate::display::cache::operations::imported(
            out,
            &receipt.file,
            mcpm_lib::local::cache::ImportOutcome {
                key: receipt.id,
                published: receipt.published,
                method: receipt.method.map(Into::into),
            },
            context.headers && first,
        )?;
        out.flush()?;
        first = false;
    }
    Ok(())
}
