//! Applies registered server compatibility defaults to remote project searches.

use anyhow::Result;
use clap::Args;

use crate::commands::source_support::search;

#[derive(Args)]
#[command(
    after_help = "Modes: TUI (default in terminals), Console (--no-ui), Raw (--raw or redirected stdout).\n\nQUERY accepts keywords followed by @version VALUE and @loader VALUE.\nUse (A | B) for alternatives and * to clear a field; quote the whole expression.\nFields override explicit options and server defaults. Omit QUERY to browse.\nMinecraft version and loader default to the registered server; explicit filters override each default.\nUse --all-versions to disable both compatibility filters. Enter on a result opens files with the same filters; Esc returns.\nTUI: / edits the top search field; Enter submits, Esc discards edits.\nValue suggestions: Up/Down selects, Enter/Tab inserts, Esc closes; Enter again searches.\nLoader suggestions: Forge, NeoForge, Fabric. Versions load from the current source.\nVersion catalogs are cached for this session; Ctrl-R retries failed loads.\nTab switches Search / Results / Details.\nDetails: Up/Down selects a field, c copies its full value, Enter expands/collapses.\n\nExamples:\n  mcpm server mods source search create --server ./my-server --source modrinth\n  mcpm server mods source search \"Steam 'n' Rails\" --source curseforge -N\n  mcpm server mods source search create --source modrinth --all-versions\n  mcpm server mods source search create --source modrinth --game-version 1.21.1 --columns name,id"
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
