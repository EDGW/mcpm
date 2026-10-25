//! Rename one stash without changing its identity or workspace baseline.

use std::io::Write;

use anyhow::Result;
use mcpm_backend::local::mc::server::stash::StashStore;

use super::{Context, input};
use crate::display::stash;

pub(super) fn run(rename: RenameArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let server = context.server()?;
    let mut store = StashStore::write(&server)?;
    let stash = store.rename(&rename.selection.selector(), rename.new_name)?;
    drop(store);
    stash::operation(
        &mut out,
        "rename",
        Some(&stash),
        &stash.id().to_string(),
        "renamed",
        context.headers,
    )?;
    Ok(())
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Modes: Console and Raw. Select one stash by --name or --id and supply the new name. Names must be unique; renaming to the existing name succeeds without writing."
)]
pub(super) struct RenameArgs {
    #[command(flatten)]
    selection: input::Single,
    /// New unique exact name.
    new_name: String,
}
