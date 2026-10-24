//! Ordered source refresh with per-source outcomes and the resulting stored snapshot.

use crate::commands::{
    execution::Runtime,
    source_support::{self, SourcesArgs},
};
use anyhow::{Context, Result, ensure};
use clap::Args;
use kako_mods_lib::local::cache::{
    hash::Blake3Hash,
    mods::{MetadataRefresh, ModsCacheDir},
};
use std::collections::HashSet;

use super::super::batch;
use super::Presentation;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout; silent on success, exit code 0).\n\nPlace HASH arguments before ident, pipe one HASH per nonblank stdin line, or use --all for the complete cache inventory. --all conflicts with HASH arguments and nonblank stdin. With --all, --parallelism COUNT limits concurrent entries (default 10; positive integers only). Use 1 for sequential work. Explicit HASH inputs stay sequential. Results retain BLAKE3 order. --sources accepts comma-separated providers; --source may be repeated. Defaults to modrinth,curseforge. curseforge@KEY uses the official API. Select each source once. Valid cached matches are reused unless --force is set. Only verified matches are stored; unmatched requests and errors preserve earlier matches. Sources run in selection order.\nConsole defaults to short output for more than three selected entries. --short shows only BLAKE3 and success/failed; --details shows BLAKE3, Status and File, then each source as reused, matched or unmatched alongside stored matches. These flags conflict. An unmatched result is successful and does not create metadata. --headers has no effect. Errors are reported to stderr with their cause chain.\n\nExamples:\n  mcpm cache mods refresh HASH1 HASH2 ident --sources modrinth,curseforge\n  mcpm cache mods refresh --all ident --short\n  printf '%s\\n' HASH1 HASH2 | mcpm cache mods refresh ident --source modrinth"
)]
pub(super) struct IdentArgs {
    #[command(flatten)]
    sources: SourcesArgs,
}

pub(super) fn run(
    args: IdentArgs,
    ids: Vec<Blake3Hash>,
    cache: &ModsCacheDir,
    mode: MetadataRefresh,
    parallelism: usize,
    output: &mut Presentation<'_>,
) -> Result<()> {
    Runtime::new()?.with_workers(|runtime| {
        let selected = args.sources.selected()?;
        let sources = selected
            .iter()
            .map(|spec| source_support::build_source(spec))
            .collect::<Result<Vec<_>>>()?;
        let mut seen = HashSet::new();
        for source in &sources {
            ensure!(
                seen.insert(source.identification_cache_id()),
                "select each identification source only once"
            );
        }
        batch::run(
            &ids,
            parallelism,
            |id| {
                runtime.block_on(async {
                    let mut results = Vec::with_capacity(sources.len());
                    for source in &sources {
                        let outcome = cache
                            .refresh_ident_info_with_outcome(id, source.as_ref(), mode)
                            .await
                            .with_context(|| {
                                format!("failed to refresh source identification for {id}")
                            })?;
                        results.push((source.identification_cache_id(), outcome.status));
                    }
                    Ok(results)
                })
            },
            |id, result| {
                let results = output.check(id, result)?;
                output.ident(id, &results)
            },
        )
    })
}
