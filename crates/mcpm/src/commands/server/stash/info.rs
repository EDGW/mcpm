//! Resolve selected stash details and capture manifests only when their fields are requested.

use std::io::Write;

use anyhow::{Result, ensure};
use mcpm_backend::operations::server::stash::{Info, InfoInput};

use super::{Context, columns, input, query};
use crate::display::stash;

pub(super) async fn run(info: InfoArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let selectors = info.selection.read()?;
    let columns = info.columns.selected()?;
    let include_manifest = (info.details && crate::terminal::stdout_is_tty())
        || columns.iter().any(|column| column.needs_manifest());
    let result = context
        .executor
        .call::<Info>(InfoInput {
            path: context.path.clone(),
            selectors,
            include_manifest,
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
                manifest: entry.manifest,
            }),
            Err(error) => query::report(error, &mut failures),
        }
    }
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
