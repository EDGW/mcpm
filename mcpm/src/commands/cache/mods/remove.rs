//! Removal of cache-owned names without changing exported files or external hard links.

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::cache::mods::ModsCacheSelection;
use std::io::Write;

use super::input::Inputs;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV). HASH arguments and redirected stdin are combined, one BLAKE3 per line. --all removes every owned cache entry in BLAKE3 order, including orphan metadata; it conflicts with HASH arguments and nonblank stdin. An empty --all selection succeeds. Each completed removal emits one record. The first failure stops the batch; completed removals remain. Exported copies and hard links are unaffected. Errors use the shared cause-chain renderer."
)]
pub(super) struct RemoveArgs {
    #[command(flatten)]
    inputs: Inputs,
}

pub(super) fn run(
    args: RemoveArgs,
    selection: ModsCacheSelection,
    out: &mut impl Write,
    headers: bool,
) -> Result<()> {
    let inputs = args.inputs.read()?;
    let cache = selection.open()?;
    let ids = inputs.resolve(&cache)?;
    for (index, id) in ids.into_iter().enumerate() {
        let removed = cache
            .remove(&id)
            .with_context(|| format!("failed to remove {id}"))?;
        crate::display::cache::operations::removed(out, id, removed, headers && index == 0)?;
    }
    Ok(())
}
