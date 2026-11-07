//! Rename one stash without changing its identity or workspace baseline.

use std::io::Write;

use anyhow::Result;
use mcpm_backend::operations::server::stash::{Rename, RenameInput};

use super::{Context, input};
use crate::display::stash;

pub(super) async fn run(
    rename: RenameArgs,
    context: &Context,
    mut out: &mut impl Write,
) -> Result<()> {
    let stash = context
        .executor
        .call::<Rename>(RenameInput {
            path: context.path.clone(),
            selector: rename.selection.selector(),
            new_name: rename.new_name,
        })
        .await?;
    stash::operation(
        &mut out,
        "rename",
        Some(&stash),
        &stash.id.to_string(),
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
