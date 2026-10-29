//! Ordered source refresh through the shared identification pipeline.

use crate::{
    commands::{
        execution::Runtime,
        source_support::SourcesArgs,
    },
    flows::{
        cache_ident_refresh::{self, CacheObject, Status},
        identify::{self, Target},
    },
};
use anyhow::{Result, ensure};
use clap::Args;
use mcpm_backend::data::cache::{
    hash::Blake3Hash,
    mods::{MetadataRefresh, ModsCacheDir, ModsCacheSelection, ident::RefreshStatus},
};
use mcpm_backend::source::types::SourceId;
use std::collections::HashSet;

use super::super::batch;
use super::Presentation;

#[derive(Debug, Args)]
#[command(
    after_help = format!("Modes: Console and Raw (--raw or redirected stdout; silent on success, exit code 0).\n\nPlace HASH arguments before ident, pipe one HASH per nonblank stdin line, or use --all for the complete cache inventory. --all conflicts with HASH arguments and nonblank stdin. With --all, --parallelism COUNT limits concurrent entries (default 10; positive integers only). Use 1 for sequential work. Explicit HASH inputs stay sequential. Results retain BLAKE3 order. --sources accepts comma-separated providers; --source may be repeated. Defaults to modrinth,curseforge. curseforge@KEY uses the official API. Select each source once. Valid cached matches are reused unless --force is set. Only verified matches are stored; unmatched requests and errors preserve earlier matches. Sources run in selection order.\nConsole defaults to short output for more than three selected entries. --short shows only BLAKE3 and success/failed; --details shows BLAKE3, Status and File, then each source as reused, matched or unmatched alongside stored matches. These flags conflict. An unmatched result is successful and does not create metadata. --headers has no effect. Errors are reported to stderr with their cause chain.\n\nExamples:\n  {tools} cache refresh HASH1 HASH2 ident --sources modrinth,curseforge\n  {tools} cache refresh --all ident --short\n  printf '%s\\n' HASH1 HASH2 | {tools} cache refresh ident --source modrinth", tools = crate::program::roots().tools)
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
        let specifications = args.sources.selected()?;
        let selection = ModsCacheSelection::Directory {
            path: cache.path().to_owned(),
        };
        let target = Target {
            session: crate::commands::execution::local_session()?,
            cache: Some(selection),
        };
        let mut seen = HashSet::new();
        for profile in
            runtime.block_on(identify::profiles(&target.session, &specifications))?
        {
            ensure!(
                seen.insert(profile.cache_key),
                "select each identification source only once"
            );
        }
        let force = mode == MetadataRefresh::Force;
        batch::run(
            &ids,
            parallelism,
            batch::FailurePolicy::Stop,
            |id| {
                runtime.block_on(async {
                    let object = CacheObject {
                        key: id.to_string(),
                        path: cache.content_path(&id),
                    };
                    let results = cache_ident_refresh::refresh(
                        target.clone(),
                        force,
                        specifications.clone(),
                        object,
                    )
                    .await?;
                    let results: Vec<(SourceId, RefreshStatus)> = results
                        .into_iter()
                        .map(|(source, status)| (source, status.into()))
                        .collect();
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

impl From<Status> for RefreshStatus {
    fn from(status: Status) -> Self {
        match status {
            Status::Reused => Self::Reused,
            Status::Matched => Self::Matched,
            Status::Unmatched => Self::Unmatched,
        }
    }
}
