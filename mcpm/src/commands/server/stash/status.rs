//! Inspect workspace changes without initializing the Mod directory or content cache.

use std::io::Write;

use anyhow::Result;
use mcpm_backend::local::mc::server::stash::StashStore;

use super::{Context, options::WorkArgs};
use crate::display::{backup::FileProgress, stash};

pub(super) fn run(args: StatusArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let server = context.server()?;
    let store = StashStore::read(&server)?;
    let mut progress = FileProgress::default();
    let mut emit = |event| {
        progress.event(event);
        Ok(())
    };
    let result = store.status(args.work.parallelism, &mut emit);
    progress.clear();
    let status = result?;
    drop(store);
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
