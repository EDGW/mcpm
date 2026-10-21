//! Applies registered server compatibility defaults to remote project searches.

use anyhow::Result;
use clap::Args;

use crate::commands::source_support::search;

#[derive(Args)]
#[command(
    after_help = "Modes: TUI (default in terminals), Console (--no-ui), Raw (--raw or redirected stdout).\n\nQUERY is plain-text keywords; quote multiple words as one argument. Omit it to browse.\nMinecraft version and loader default to the registered server; explicit filters override each default.\nUse --all-versions to disable both compatibility filters. Enter on a result opens files with the same filters; Esc returns.\nTUI: / edits the top search field; Enter submits, Esc discards edits.\nTab switches Search / Results / Details; Up/Down scrolls focused Details.\n\nExamples:\n  mcpm server mods source search create --server ./my-server --source modrinth\n  mcpm server mods source search \"Steam 'n' Rails\" --source curseforge -N\n  mcpm server mods source search create --source modrinth --all-versions\n  mcpm server mods source search create --source modrinth --game-version 1.21.1 --columns name,id"
)]
pub(super) struct ServerSearchArgs {
    #[command(flatten)]
    server: super::CompatibilityArgs,
    #[command(flatten)]
    search: search::SearchArgs,
}

pub(super) fn run(mut args: ServerSearchArgs) -> Result<()> {
    if let Some((version, loader)) = args.server.defaults()? {
        args.search.apply_defaults(&version, &loader);
    }
    search::run(args.search)
}
