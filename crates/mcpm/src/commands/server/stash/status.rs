//! Inspect workspace changes without initializing the Mod directory or content cache.

use std::io::Write;

use anyhow::{Context as _, Result};
use mcpm_backend::operations::server::stash::{Status, StatusEvent, StatusInput};

use super::{Context, options::WorkArgs};
use crate::display::stash::{self, FileProgress};

pub(super) async fn run(
    args: StatusArgs,
    context: &Context,
    mut out: &mut impl Write,
) -> Result<()> {
    let mut stream = context
        .executor
        .call::<Status>(StatusInput {
            path: context.path.clone(),
            parallelism: args.work.parallelism,
        })
        .await?;
    let mut progress = FileProgress::default();
    let mut result = None;
    while let Some(event) = stream.next().await {
        match event? {
            StatusEvent::Progress(event) => progress.event(event),
            StatusEvent::Status(status) => result = Some(status),
        }
    }
    progress.clear();
    let status = result.context("stash status stream ended without its result")?;
    stash::status(&mut out, &status, context.headers)?;
    Ok(())
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Modes: Console and Raw. Compares all regular files, without verifying backup data. Modified results include added, removed and changed paths. Raw repeats the workspace state and baseline fields per changed file; clean and untracked each emit one row."
)]
pub(super) struct StatusArgs {
    #[command(flatten)]
    work: WorkArgs,
}
