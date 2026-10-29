//! Read and order lightweight stash records, releasing the lease before confirmation and rendering.

use std::io::Write;

use anyhow::{Result, ensure};
use mcpm_backend::local::mc::server::stash::StashStore;

use super::{Context, columns, query};
use crate::display::stash;

pub(super) fn run(list: ListArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let columns = list.columns.selected(list.details)?;
    let server = context.server()?;
    let store = StashStore::read(&server)?;
    let mut failures = 0;
    let head = query::observed_head(&store, &mut failures);
    let mut entries = Vec::new();
    for result in store.list()? {
        match result {
            Ok(stash) => entries.push(query::entry(&store, stash, head, false)?),
            Err(error) => query::report(&error, &mut failures),
        }
    }
    entries.sort_by(|a, b| {
        b.stash
            .created_at()
            .cmp(&a.stash.created_at())
            .then_with(|| a.stash.id().cmp(&b.stash.id()))
    });
    drop(store);
    if crate::commands::confirm::reading(entries.len(), "stashes")? {
        stash::entries(
            &mut out,
            &entries,
            &columns,
            context.headers,
            false,
            list.details || list.columns.explicit(),
        )?;
    }
    ensure!(failures == 0, "{failures} stash records could not be read");
    Ok(())
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Modes: Console and Raw. Does not hash mods or inspect backup data. --details selects all lightweight fields unless columns are explicit. List columns: name,id,time,head,backup. Console asks Y/n before displaying more than 80 entries."
)]
pub(super) struct ListArgs {
    #[command(flatten)]
    columns: columns::ListColumns,
    /// Show all lightweight fields unless columns are explicitly selected.
    #[arg(long)]
    details: bool,
}
