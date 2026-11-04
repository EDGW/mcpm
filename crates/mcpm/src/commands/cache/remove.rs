//! Removal of cache-owned names without changing exported files or external hard links.

use anyhow::Result;
use clap::Args;
use mcpm_backend::operations::cache::{List, Remove, RemoveInput};
use std::io::Write;

use super::input::Inputs;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV). HASH arguments and redirected stdin are combined, one BLAKE3 per line. --all removes the complete inventory, including orphan metadata; it conflicts with HASH arguments and nonblank stdin. Before removing all entries, the selected host and resolved cache path are shown for confirmation (default No). Use --yes in Raw or non-interactive mode. Only keys listed before confirmation are removed. Each completed removal emits one record. The first failure stops processing; completed removals remain. Exported copies and hard links are unaffected. --yes does not bypass stash protection."
)]
pub(super) struct RemoveArgs {
    #[command(flatten)]
    inputs: Inputs,
    /// Skip the remove-all confirmation; required for --all in Raw or non-interactive mode.
    #[arg(long)]
    yes: bool,
}

pub(super) async fn run(
    args: RemoveArgs,
    context: &super::Context,
    out: &mut impl Write,
) -> Result<()> {
    let inputs = args.inputs.read()?;
    let executor = context.executor().await?;
    let mut directory = None;
    let ids = match inputs {
        super::input::Selection::Ids(ids) => ids,
        super::input::Selection::All => {
            let inventory = executor.call::<List>(context.cache.clone()).await?;
            if !crate::commands::confirm::cache_removal(
                &inventory.directory,
                &context.target,
                args.yes,
            )? {
                return Ok(());
            }
            directory = Some(inventory.directory);
            inventory
                .entries
                .into_iter()
                .map(|entry| entry.key)
                .collect()
        }
    };
    let mut stream = executor
        .call::<Remove>(RemoveInput {
            cache: context.cache.clone(),
            directory,
            ids,
        })
        .await?;
    let mut first = true;
    while let Some(receipt) = stream.next().await {
        let receipt = receipt?;
        crate::display::cache::operations::removed(
            out,
            receipt.id,
            receipt.removed,
            context.headers && first,
        )?;
        out.flush()?;
        first = false;
    }
    Ok(())
}
