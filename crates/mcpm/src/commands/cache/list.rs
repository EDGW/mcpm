//! Cache inventory listing; only selected metadata columns trigger metadata decoding.

use std::io::Write;

use anyhow::Result;
use clap::Args;
use mcpm_backend::operations::cache::{Info, InfoInput, List, Selection};

use super::columns::{self, Columns};
use crate::{
    display::cache::{self, Column},
    terminal,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV; --raw or redirected stdout).\n\nDefault output contains only BLAKE3. --details selects all columns unless columns are explicitly selected. Metadatas lists named metadata; Console includes their complete details. Raw lists metadata names only. Console asks Y/n before listing more than 80 entries (Enter means Yes). --headers adds a Raw header; Console prints each selected field on its own line with the label immediately before its value."
)]
pub(super) struct ListArgs {
    #[command(flatten)]
    columns: Columns,
    /// Select all columns by default; explicit column selection takes precedence.
    #[arg(long)]
    details: bool,
}

pub(super) async fn run(
    args: ListArgs,
    context: &super::Context,
    out: &mut impl Write,
) -> Result<()> {
    let columns = args.columns.selected(if args.details {
        &Column::ALL
    } else {
        &[Column::Blake3]
    });
    let executor = context.executor().await?;
    let inventory = executor.call::<List>(context.cache.clone()).await?;
    if !crate::commands::confirm::reading(inventory.entries.len(), "cache entries")? {
        return Ok(());
    }
    if terminal::stdout_is_tty() && columns.len() == 1 {
        cache::single_header(out, columns[0], args.columns.explicit() || args.details)?;
    } else {
        cache::headers(out, &columns, context.headers)?;
    }
    let options = columns::read_options(&columns);
    let mut first = true;
    let mut render = |entry: &mcpm_backend::operations::cache::Entry| -> Result<()> {
        if !first && terminal::stdout_is_tty() && columns.len() > 1 {
            writeln!(out)?;
        }
        cache::list_entry(out, entry, &columns)?;
        out.flush()?;
        first = false;
        Ok(())
    };
    if options.sha256 || (options.metadata && options.metadata_details) {
        let mut selected = context.cache.clone();
        selected.selection = mcpm_lib::local::cache::mods::ModsCacheSelection::Directory {
            path: inventory.directory,
        };
        let mut stream = executor
            .call::<Info>(InfoInput {
                cache: selected,
                selection: Selection::Ids(
                    inventory
                        .entries
                        .into_iter()
                        .map(|entry| entry.key)
                        .collect(),
                ),
                options,
            })
            .await?;
        while let Some(entry) = stream.next().await {
            render(&entry?)?;
        }
    } else {
        for entry in inventory.entries {
            render(&entry.into_entry(options.metadata)?)?;
        }
    }
    Ok(())
}
