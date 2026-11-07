//! Loader metadata refresh over cached bytes, with rendering delegated to display.

use super::Presentation;
use crate::commands::supports::mods::loaders::LoaderArgs;
use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
#[command(
    after_help = format!("Modes: Console and Raw (--raw or redirected stdout; silent on success, exit code 0).\n\nPlace HASH arguments before meta, pipe one HASH per nonblank stdin line, or use --all for the complete cache inventory. --all conflicts with HASH arguments and nonblank stdin. With --all, --parallelism COUNT limits concurrent entries (default 10; positive integers only). Use 1 for sequential work. Explicit HASH inputs stay sequential. Results retain BLAKE3 order. --loader and --loaders accept comma-separated values and may be repeated or combined. Defaults to forge,fabric,neoforge. Current recognition and non-recognition results are reused unless --force is set. Selected Loader results are committed together after identification succeeds.\nConsole defaults to short output for more than three selected entries. --short shows only BLAKE3 and success/failed; --details shows BLAKE3, Status and File followed by Loader details. These flags conflict. --headers has no effect. Errors are reported to stderr with their cause chain.\n\nExamples:\n  {tools} cache refresh HASH1 HASH2 meta --loaders fabric,forge\n  {tools} cache refresh --all meta --details\n  printf '%s\\n' HASH1 HASH2 | {tools} cache refresh meta", tools = crate::program::roots().tools)
)]
pub(super) struct MetaArgs {
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) async fn run(
    args: MetaArgs,
    selection: mcpm_backend::operations::cache::Selection,
    force: bool,
    parallelism: usize,
    context: &super::super::Context,
    output: &mut Presentation<'_>,
) -> Result<()> {
    use mcpm_backend::operations::cache::refresh::{Meta, MetaEvent, MetaInput};
    let loaders = args.loaders.ids();
    let mut stream = context
        .executor()
        .await?
        .call::<Meta>(MetaInput {
            cache: context.cache.clone(),
            selection,
            loaders,
            force,
            parallelism,
        })
        .await?;
    let mut done = false;
    while let Some(event) = stream.next().await {
        match event? {
            MetaEvent::Selected(count) => output.selected(count),
            MetaEvent::Entry { id, result } => {
                let receipt = output.check(id, result.map_err(|error| error.into_inner()))?;
                output.meta(&receipt)?;
            }
            MetaEvent::Done => done = true,
        }
    }
    anyhow::ensure!(done, "missing metadata refresh completion");
    Ok(())
}
