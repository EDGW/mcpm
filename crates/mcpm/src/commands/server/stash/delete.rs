//! Delete a selected batch after backend HEAD protection, retaining per-input failure identity.

use std::io::Write;

use anyhow::{Result, ensure};
use mcpm_backend::local::mc::server::stash::StashStore;

use super::{Context, input, query};
use crate::display::stash;

pub(super) fn run(args: DeleteArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let selectors = args.selection.read()?;
    let server = context.server()?;
    let mut store = StashStore::write(&server)?;
    let results = store.delete(&selectors)?;
    drop(store);
    let mut failures = 0;
    for (index, deletion) in results.into_iter().enumerate() {
        let label = input::label(&deletion.selector);
        match deletion.result {
            Ok(stash) => stash::operation(
                &mut out,
                "delete",
                Some(&stash),
                &label,
                "deleted",
                context.headers && index == 0,
            )?,
            Err(error) => {
                query::report(&error, &mut failures);
                stash::operation(
                    &mut out,
                    "delete",
                    None,
                    &label,
                    "failed",
                    context.headers && index == 0,
                )?;
            }
        }
    }
    ensure!(failures == 0, "{failures} stash deletions failed");
    Ok(())
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Modes: Console and Raw. Accepts repeated --name/--id, plural lists and typed stdin. Selecting HEAD refuses the entire batch before deletion. Backup is removed before its index; missing backups permit index cleanup. Partial failures may remain retryable. No extra confirmation; no --force."
)]
pub(super) struct DeleteArgs {
    #[command(flatten)]
    selection: input::Multiple,
}
