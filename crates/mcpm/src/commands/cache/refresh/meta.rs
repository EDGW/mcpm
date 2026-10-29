//! Loader metadata refresh over cached bytes, with rendering delegated to display.

use super::super::batch;
use super::Presentation;
use crate::commands::supports::mods::loaders::LoaderArgs;
use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::local::cache::{
    hash::Blake3Hash,
    mods::{MetadataRefresh, ModsCacheDir},
};

#[derive(Debug, Args)]
#[command(
    after_help = format!("Modes: Console and Raw (--raw or redirected stdout; silent on success, exit code 0).\n\nPlace HASH arguments before meta, pipe one HASH per nonblank stdin line, or use --all for the complete cache inventory. --all conflicts with HASH arguments and nonblank stdin. With --all, --parallelism COUNT limits concurrent entries (default 10; positive integers only). Use 1 for sequential work. Explicit HASH inputs stay sequential. Results retain BLAKE3 order. --loader and --loaders accept comma-separated values and may be repeated or combined. Defaults to forge,fabric,neoforge. Current recognition and non-recognition results are reused unless --force is set. Selected Loader results are committed together after identification succeeds.\nConsole defaults to short output for more than three selected entries. --short shows only BLAKE3 and success/failed; --details shows BLAKE3, Status and File followed by Loader details. These flags conflict. --headers has no effect. Errors are reported to stderr with their cause chain.\n\nExamples:\n  {tools} cache refresh HASH1 HASH2 meta --loaders fabric,forge\n  {tools} cache refresh --all meta --details\n  printf '%s\\n' HASH1 HASH2 | {tools} cache refresh meta", tools = crate::program::roots().tools)
)]
pub(super) struct MetaArgs {
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(
    args: MetaArgs,
    ids: Vec<Blake3Hash>,
    cache: &ModsCacheDir,
    mode: MetadataRefresh,
    parallelism: usize,
    output: &mut Presentation<'_>,
) -> Result<()> {
    let loaders = args.loaders.select();
    batch::run(
        &ids,
        parallelism,
        batch::FailurePolicy::Stop,
        |id| {
            cache
                .refresh_metadata(id, &loaders, mode)
                .with_context(|| format!("failed to refresh Loader metadata for {id}"))
        },
        |id, result| {
            let entry = output.check(id, result)?;
            output.meta(&entry)
        },
    )
}
