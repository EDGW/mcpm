//! Present sequential host verification and report independent failures without interrupting later items.

use std::io::Write;

use anyhow::{Result, ensure};
use mcpm_backend::operations::server::stash::{Verify, VerifyEvent, VerifyInput};

use super::{Context, input, options::WorkArgs, query};
use crate::display::stash::{self, FileProgress};

pub(super) async fn run(
    verify: VerifyArgs,
    context: &Context,
    mut out: &mut impl Write,
) -> Result<()> {
    let selectors = verify.selection.read()?;
    let mut stream = context
        .executor
        .call::<Verify>(VerifyInput {
            path: context.path.clone(),
            selectors,
            parallelism: verify.work.parallelism,
        })
        .await?;
    let mut progress = FileProgress::default();
    let mut failures = 0;
    let mut printed = false;
    while let Some(event) = stream.next().await {
        match event? {
            VerifyEvent::Progress(event) => progress.event(event),
            VerifyEvent::HeadFailed(error) => {
                progress.clear();
                query::report(error, &mut failures);
            }
            VerifyEvent::Item { selector, result } => {
                progress.clear();
                let label = input::label(&selector);
                match result {
                    Ok(snapshot) => stash::operation(
                        &mut out,
                        "verify",
                        Some(&snapshot),
                        &label,
                        "valid",
                        context.headers && !printed,
                    )?,
                    Err(error) => {
                        query::report(error, &mut failures);
                        stash::operation(
                            &mut out,
                            "verify",
                            None,
                            &label,
                            "failed",
                            context.headers && !printed,
                        )?;
                    }
                }
                printed = true;
            }
        }
    }
    progress.clear();
    ensure!(failures == 0, "{failures} stash checks failed");
    Ok(())
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Modes: Console and Raw. Accepts repeated --name/--id, plural lists and typed stdin. Checks the manifest, file set, sizes and complete BLAKE3 hashes through backup verification, using the server-local stash cache for missing backup files. Never initializes a missing cache or uses the global cache. Does not compare current mods. Failures do not stop later selections; any failure returns nonzero."
)]
pub(super) struct VerifyArgs {
    #[command(flatten)]
    selection: input::Multiple,
    #[command(flatten)]
    work: WorkArgs,
}
