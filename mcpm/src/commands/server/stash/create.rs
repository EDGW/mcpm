//! Save a changed workspace, publish its stash and report both publication and cleanup failures.

use std::io::Write;

use anyhow::Result;
use mcpm_backend::local::mc::server::stash::{CreateCleanupError, StashStore};

use super::{Context, options::WorkArgs};
use crate::display::{backup::FileProgress, stash};

pub(super) fn run(create: CreateArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let server = context.server()?;
    let mut store = StashStore::write(&server)?;
    let mut progress = FileProgress::default();
    let mut emit = |event| {
        progress.event(event);
        Ok(())
    };
    let result = store.create(create.name, create.work.parallelism, &mut emit);
    progress.clear();
    if let Err(error) = &result
        && let Some(failure) = error.downcast_ref::<CreateCleanupError>()
    {
        crate::diagnostics::error(failure.cleanup());
    }
    let stash = result?;
    drop(store);
    stash::operation(
        &mut out,
        "create",
        Some(&stash),
        &stash.id().to_string(),
        "created",
        context.headers,
    )?;
    Ok(())
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Modes: Console and Raw. Unchanged workspaces are refused. Names are unique, exact and case-sensitive. Creating a stash does not modify mods. The saved stash becomes the new comparison baseline. Stores a backup manifest and keeps file contents in the server-local stash cache."
)]
pub(super) struct CreateArgs {
    /// Unique exact stash name.
    name: String,
    #[command(flatten)]
    work: WorkArgs,
}
