//! Consume host switch progress and render the committed result; force and HEAD stay in the backend.

use std::io::Write;

use anyhow::{Context as _, Result};
use mcpm_backend::operations::server::stash::{Switch, SwitchEvent, SwitchInput};

use super::{Context, input, options::WorkArgs};
use crate::display::stash::{self, FileProgress};

pub(super) async fn run(
    switch: SwitchArgs,
    context: &Context,
    mut out: &mut impl Write,
) -> Result<()> {
    let mut stream = context
        .executor
        .call::<Switch>(SwitchInput {
            path: context.path.clone(),
            selector: switch.selection.selector(),
            force: switch.force,
            parallelism: switch.work.parallelism,
        })
        .await?;
    let mut progress = FileProgress::default();
    let mut outcome = None;
    while let Some(event) = stream.next().await {
        match event? {
            SwitchEvent::Progress(event) => progress.event(event),
            SwitchEvent::Switched { stash, changed } => outcome = Some((stash, changed)),
        }
    }
    progress.clear();
    let (snapshot, changed) = outcome.context("stash switch stream ended without its result")?;
    stash::operation(
        &mut out,
        "switch",
        Some(&snapshot),
        &snapshot.id.to_string(),
        if changed { "switched" } else { "unchanged" },
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
