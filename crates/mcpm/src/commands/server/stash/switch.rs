//! Verify and switch a local workspace; force and HEAD behavior stay in the backend.

use std::io::Write;

use anyhow::Result;
use mcpm_backend::local::mc::server::stash::StashStore;

use super::{Context, input, options::WorkArgs};
use crate::display::{backup::FileProgress, stash};

pub(super) fn run(switch: SwitchArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let server = context.server()?;
    let mut store = StashStore::write(&server)?;
    let mut progress = FileProgress::default();
    let mut emit = |event| {
        progress.event(event);
        Ok(())
    };
    let result = store.switch(
        &switch.selection.selector(),
        switch.force,
        switch.work.parallelism,
        &mut emit,
    );
    progress.clear();
    let outcome = result?;
    drop(store);
    stash::operation(
        &mut out,
        "switch",
        Some(&outcome.stash),
        &outcome.stash.id().to_string(),
        if outcome.changed {
            "switched"
        } else {
            "unchanged"
        },
        context.headers,
    )?;
    Ok(())
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Modes: Console and Raw. Always fully verifies the target backup and any required server-local stash cache contents before checking the workspace, including with --force and when selecting HEAD. Modified or untracked workspaces are refused unless --force discards their contents. Copies replace the entire mods directory, including removal of extra files. No automatic backup of discarded modifications. HEAD changes only after successful application."
)]
pub(super) struct SwitchArgs {
    #[command(flatten)]
    selection: input::Single,
    /// Discard modified/untracked contents; never bypass verification or check errors.
    #[arg(long)]
    force: bool,
    #[command(flatten)]
    work: WorkArgs,
}
