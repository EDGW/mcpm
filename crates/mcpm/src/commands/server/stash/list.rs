//! Present lightweight host inventory, confirming only after the operation has released its lease.

use std::io::Write;

use anyhow::{Result, ensure};
use mcpm_backend::operations::server::stash::{List, ListInput};

use super::{Context, columns, query};
use crate::display::stash;

pub(super) async fn run(list: ListArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let columns = list.columns.selected(list.details)?;
    let result = context
        .executor
        .call::<List>(ListInput {
            path: context.path.clone(),
        })
        .await?;
    let mut failures = 0;
    let head = query::observed_head(result.head, &mut failures);
    let mut entries = Vec::new();
    for result in result.entries {
        match result {
            Ok(entry) => entries.push(stash::Entry {
                head: head.map(|head| head == Some(entry.stash.id)),
                stash: entry.stash,
                backup: entry.backup_path,
                manifest: None,
            }),
            Err(error) => query::report(error, &mut failures),
        }
    }
    entries.sort_by(|a, b| {
        b.stash
            .created_at
            .cmp(&a.stash.created_at)
            .then_with(|| a.stash.id.cmp(&b.stash.id))
    });
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
