//! Verify selected backups while retaining a shared lease and continuing after individual failures.

use std::io::Write;

use anyhow::{Result, ensure};
use mcpm_backend::local::mc::server::stash::{Selector, StashStore};

use super::{Context, input, options::WorkArgs, query};
use crate::display::{backup::FileProgress, stash};

pub(super) fn run(verify: VerifyArgs, context: &Context, mut out: &mut impl Write) -> Result<()> {
    let selectors = verify.selection.read()?;
    let server = context.server()?;
    let store = StashStore::read(&server)?;
    let mut progress = FileProgress::default();
    let mut failures = 0;
    // HEAD failures do not prevent independent backup verification.
    let _head = query::observed_head(&store, &mut failures);
    for (index, (label, result)) in input::resolve(&store, selectors).into_iter().enumerate() {
        let result = result.and_then(|stash| {
            store.verify(
                &Selector::Id(stash.id()),
                verify.work.parallelism,
                |event| {
                    progress.event(event);
                    Ok(())
                },
            )
        });
        progress.clear();
        match result {
            Ok(stash) => stash::operation(
                &mut out,
                "verify",
                Some(&stash),
                &label,
                "valid",
                context.headers && index == 0,
            )?,
            Err(error) => {
                query::report(&error, &mut failures);
                stash::operation(
                    &mut out,
                    "verify",
                    None,
                    &label,
                    "failed",
                    context.headers && index == 0,
                )?;
            }
        }
    }
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
