//! Batch cache information queries from arguments and stdin with selectable detail fields.

use std::io::Write;

use anyhow::Result;
use clap::Args;
use mcpm_backend::operations::cache::{Info, InfoInput};

use super::{
    columns::{self, Columns},
    input::Inputs,
};
use crate::display::cache::{self, Column};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV; --raw or redirected stdout).\n\nArguments and redirected stdin are combined; one complete BLAKE3 per nonblank line. Duplicates are processed once, in first-seen order. --all selects the complete cache inventory in BLAKE3 order, including orphan metadata and invalid content types; it conflicts with HASH arguments or nonblank stdin. An empty --all selection succeeds. All columns are shown by default. --columns, --column and -C select the same fields as list. Console includes each metadata's details; Raw lists metadata names only and does not decode their contents. --headers adds a Raw header. SHA-256 is read from existing meta only when selected. This command does not identify or refresh anything."
)]
pub(super) struct InfoArgs {
    #[command(flatten)]
    inputs: Inputs,
    #[command(flatten)]
    columns: Columns,
}

pub(super) async fn run(
    args: InfoArgs,
    context: &super::Context,
    out: &mut impl Write,
) -> Result<()> {
    let inputs = args.inputs.read()?;
    let columns = args.columns.selected(&Column::ALL);
    let mut stream = context
        .executor()
        .await?
        .call::<Info>(InfoInput {
            cache: context.cache.clone(),
            selection: inputs.into(),
            options: columns::read_options(&columns),
        })
        .await?;
    cache::headers(out, &columns, context.headers)?;
    let mut first = true;
    while let Some(observed) = stream.next().await {
        let observed = observed?;
        if !first && crate::terminal::stdout_is_tty() {
            writeln!(out)?;
        }
        cache::info(out, &observed, &columns)?;
        out.flush()?;
        first = false;
    }
    Ok(())
}
