//! Host source refresh event consumption; credentials and all provider requests go to Host.

use super::Presentation;
use crate::commands::source_support::SourcesArgs;
use anyhow::{Context, Result, ensure};
use clap::Args;
use mcpm_backend::operations::{
    cache::{
        Selection,
        refresh::{Ident, IdentEvent, IdentInput},
    },
    mods::identification::QueryStatus,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout; silent on success). Cache files and provider requests belong to the selected Host.\n\nPlace HASH arguments before ident, pipe one BLAKE3 per line, or use --all. Explicit keys stay sequential; --all uses bounded --parallelism and BLAKE3 order. The first failure stops subsequent work; already in-flight files may have completed. Select providers with --sources or repeated --source; defaults to modrinth,curseforge. curseforge@KEY uses the official API. Each provider may be selected once.\n\nOnly missing providers are queried. Verified cache hits work without valid request credentials or provider connectivity. --force disables reuse. Only verified positive matches are saved; unmatched and failed queries retain existing records. Partial failures retain cache hits and report failure. Console details include current outcomes and the final saved snapshot."
)]
pub(super) struct IdentArgs {
    #[command(flatten)]
    sources: SourcesArgs,
}

pub(super) async fn run(
    args: IdentArgs,
    selection: Selection,
    force: bool,
    parallelism: usize,
    context: &super::super::Context,
    output: &mut Presentation<'_>,
) -> Result<()> {
    let mut stream = context
        .executor()
        .await?
        .call::<Ident>(IdentInput {
            cache: context.cache.clone(),
            selection,
            sources: args.sources.selected()?,
            force,
            parallelism,
        })
        .await?;
    let mut done = None;
    let mut failed = false;
    let mut publication_failed = 0;
    while let Some(event) = stream.next().await {
        match event? {
            IdentEvent::Selected(total) => output.selected(total),
            IdentEvent::Entry { id, result } => match result {
                Ok(receipt) => {
                    for error in receipt.errors() {
                        crate::diagnostics::error(error.as_inner());
                    }
                    failed |= receipt.overall_status() != QueryStatus::Success;
                    publication_failed += usize::from(
                        !receipt.publication_errors.is_empty() || receipt.observation.is_err(),
                    );
                    output.ident(id, &receipt)?;
                }
                Err(error) => {
                    crate::diagnostics::error(error.as_inner());
                    output.failed(id)?;
                    failed = true;
                }
            },
            IdentEvent::Done(summary) => done = Some(summary),
        }
    }
    let summary = done.context("missing source refresh completion")?;
    ensure!(
        !failed && !summary.has_failures(),
        "source refresh completed with {} failed and {} partially failed identification(s), {} preparation failure(s), {} publication/observation failure(s)",
        summary.failed,
        summary.partial_failed,
        summary.preparation_failed,
        publication_failed
    );
    Ok(())
}
