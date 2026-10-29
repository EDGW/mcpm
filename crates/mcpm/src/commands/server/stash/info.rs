//! Resolve selected stash details and capture manifests only when their fields are requested.

use std::io::Write;

use anyhow::{Result, ensure};
use mcpm_backend::local::mc::server::stash::StashStore;

use super::{Context, columns, input, query};
use crate::display::stash;

pub(super) fn run(info: InfoArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let selectors = info.selection.read()?;
    let columns = info.columns.selected()?;
    let server = context.server()?;
    let store = StashStore::read(&server)?;
    let mut failures = 0;
    let head = query::observed_head(&store, &mut failures);
    let mut entries = Vec::new();
    let manifest = (info.details && crate::terminal::stdout_is_tty())
        || columns.iter().any(|column| column.needs_manifest());
    for (_, result) in input::resolve(&store, selectors) {
        match result.and_then(|stash| query::entry(&store, stash, head, manifest)) {
            Ok(entry) => entries.push(entry),
            Err(error) => query::report(&error, &mut failures),
        }
    }
    drop(store);
    stash::entries(
        &mut out,
        &entries,
        &columns,
        context.headers,
        info.details,
        true,
    )?;
    ensure!(failures == 0, "{failures} stash queries failed");
    Ok(())
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Modes: Console and Raw. Accepts repeated --name/--id, plural lists and typed stdin. Does not verify backup file contents. Explicit columns replace defaults. --details adds manifest file records in Console only."
)]
pub(super) struct InfoArgs {
    #[command(flatten)]
    selection: input::Multiple,
    #[command(flatten)]
    columns: columns::InfoColumns,
    /// Append per-file manifest records in Console only; does not change selected summary fields.
    #[arg(long)]
    details: bool,
}
