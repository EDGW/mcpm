//! CLI adapter for selecting and explicitly caching whole mod archives and metadata.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::source_support::SourcesArgs;
use crate::commands::supports::mods::{
    cache::{
        self,
        args::WriteArgs,
        identification::Completion,
        output::{DisplayArgs, Presentation},
    },
    loaders::LoaderArgs,
    selection::{ItemArgs, Scope},
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout; one JSON object per selected file). Console shows progress and a summary by default; --details adds per-file details. Local only.\n\nITEM matches a filename, then exact modid, then exact name. Redirected stdin appends one item per line; --input controls stdin interpretation. --all cannot be combined with queries.\n\nStores the whole outer JAR even when a nested mod matches. Imports deduplicate by BLAKE3. The default mode copies; hard-link modes share content protection with the source. Existing objects are reused without replacing or linking the source.\n\nMissing/outdated metadata is identified on the cached bytes. --metas/--meta select meta and/or ident (default both); --no-metas imports content only. Explicit selections replace defaults. --sources/--source select providers (default both); explicit selections replace defaults. Console shows separate import, meta and ident progress. --force refreshes requested results. Source matches are stored only on success; unmatched requests are not persisted. Successful non-recognition is cached. File failures are reported and later files continue; a failure returns a nonzero exit status. Completed writes are retained."
)]
pub(super) struct CacheArgs {
    #[command(flatten)]
    items: ItemArgs,
    /// Source directory for relative filenames, modid/name lookup and --all.
    #[arg(long, value_name = "PATH", default_value = ".")]
    directory: PathBuf,
    #[command(flatten)]
    writing: WriteArgs,
    #[command(flatten)]
    sources: SourcesArgs,
    #[command(flatten)]
    display: DisplayArgs,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: CacheArgs) -> Result<()> {
    let selection = args.items.read()?;
    let mut completion = Completion::new(&args.writing, &args.sources.selected()?)?;
    let mut output = Presentation::new(args.display);
    cache::run(
        selection,
        Scope::directory(args.directory),
        args.writing.options(args.loaders.select())?,
        || Ok(()),
        |event| completion.accept(event, &mut |event| output.event(event)),
    )?;
    completion.finish(
        crate::commands::source_support::identify::Location::Local,
        |event| output.event(event),
    )?;
    output.finish()
}
