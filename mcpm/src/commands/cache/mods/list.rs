//! Cache inventory listing; only selected metadata columns trigger metadata decoding.

use std::io::Write;

use anyhow::Result;
use clap::Args;
use mcpm_backend::local::cache::mods::ModsCacheSelection;

use super::{columns::Columns, entry};
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

pub(super) fn run(
    args: ListArgs,
    selection: ModsCacheSelection,
    out: &mut impl Write,
    headers: bool,
) -> Result<()> {
    let columns = args.columns.selected(if args.details {
        &Column::ALL
    } else {
        &[Column::Blake3]
    });
    let cache = selection.open()?;
    let entries = cache.scan()?;
    if !super::confirm::reading(entries.len())? {
        return Ok(());
    }
    if terminal::stdout_is_tty() && columns.len() == 1 {
        cache::single_header(out, columns[0], args.columns.explicit() || args.details)?;
    } else {
        cache::headers(out, &columns, headers)?;
    }
    for (index, item) in entries.into_iter().enumerate() {
        if index > 0 && terminal::stdout_is_tty() && columns.len() > 1 {
            writeln!(out)?;
        }
        let observed = entry::observe(
            &cache,
            item.key,
            item.inspection,
            &columns,
            terminal::stdout_is_tty(),
        )?;
        cache::list_entry(out, &observed, &columns)?;
    }
    Ok(())
}
