//! CLI adapter for selecting and explicitly caching whole mod archives and metadata.

use mcpm_lib::local::cache::mods::ModsCacheSelection;
use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::source_support::SourcesArgs;
use crate::commands::supports::mods::{
    cache::{
        self,
        args::WriteArgs,
        output::{DisplayArgs, Presentation},
    },
    loaders::LoaderArgs,
    selection::ItemArgs,
};
use mcpm_backend::operations::mods::loading::selection::SelectionInput;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout; one JSON object per selected file). Console shows progress and a summary by default; --details adds per-file details. Mod paths, cache paths and provider requests belong to the selected Host.\n\nITEM matches a filename, then exact modid, then exact name. Redirected stdin appends one item per line; --input controls stdin interpretation. --all cannot be combined with queries.\n\nStores the whole outer JAR even when a nested mod matches. Imports deduplicate by BLAKE3. The default mode copies; hard-link modes share content protection with the source. Existing objects are reused without replacing or linking the source.\n\nMissing/outdated metadata is identified on the cached bytes. --metas/--meta select meta and/or ident (default both); --no-metas imports content only. Explicit selections replace defaults. --sources/--source select providers (default both); explicit selections replace defaults. Console shows separate import, meta and ident progress. --force refreshes requested results. Valid source matches are reused independently; only missing sources are requested. A full cache hit needs no network or usable request credentials. Positive source matches are stored independently; unmatched provider responses are successful but not persisted. Successful local-format non-recognition is cached. Failed and partially failed files are counted separately and return a nonzero exit status. Completed writes and usable matches are retained."
)]
pub(super) struct CacheArgs {
    #[command(flatten)]
    items: ItemArgs,
    /// Source directory for relative filenames, modid/name lookup and --all.
    #[arg(long, value_name = "PATH", default_value = ".")]
    directory: PathBuf,
    #[command(flatten)]
    writing: WriteArgs,
    /// Target cache; explicit directories must already be initialized.
    #[arg(long, value_name = "PATH")]
    cache_dir: Option<PathBuf>,
    #[command(flatten)]
    sources: SourcesArgs,
    #[command(flatten)]
    display: DisplayArgs,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: CacheArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    let selection = SelectionInput::from_selection(args.items.read()?)?;
    let mut scope = mcpm_backend::operations::mods::loading::ScopeInput::Directory {
        directory: args.directory,
        loaders: args.loaders.ids(),
        cache: Some(
            args.cache_dir
                .map(|path| ModsCacheSelection::Directory { path })
                .unwrap_or(ModsCacheSelection::Default),
        ),
    };
    let specifications = args.sources.selected()?;
    crate::commands::execution::Runtime::new()?.block_on(async {
        let executor = crate::commands::supports::mods::target::executor(&mut scope, ssh).await?;
        let output = Presentation::new(args.display);
        cache::run(
            &executor,
            scope,
            selection,
            args.writing,
            &specifications,
            output,
        )
        .await
    })
}
