//! Save a changed workspace, publish its stash and report both publication and cleanup failures.

use std::io::Write;

use anyhow::{Context as _, Result};
use mcpm_backend::operations::server::stash::{Create, CreateEvent, CreateInput};

use super::{Context, options::WorkArgs};
use crate::display::stash::{self, FileProgress};

pub(super) async fn run(
    create: CreateArgs,
    context: &Context,
    mut out: &mut impl Write,
) -> Result<()> {
    let mut stream = context
        .executor
        .call::<Create>(CreateInput {
            path: context.path.clone(),
            name: create.name,
            parallelism: create.work.parallelism,
        })
        .await?;
    let mut progress = FileProgress::default();
    let mut created = None;
    while let Some(event) = stream.next().await {
        match event? {
            CreateEvent::Progress(event) => progress.event(event),
            CreateEvent::Created(stash) => created = Some(stash),
        }
    }
    progress.clear();
    let stash = created.context("stash create stream ended without its result")?;
    stash::operation(
        &mut out,
        "create",
        Some(&stash),
        &stash.id.to_string(),
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
