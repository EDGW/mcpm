//! Applies registered server compatibility defaults to remote file browsing.

use anyhow::Result;
use clap::Args;

use crate::commands::source_support::files;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: TUI (default in terminals), Console (--no-ui), Raw (--raw or redirected stdout).\n\nPROJECT accepts remote project IDs or exact slugs; redirected stdin appends one per line.\nMinecraft version and loader default to the registered server; explicit filters override each default.\nUse --all-versions to disable both compatibility filters. Multiple projects automatically use list output.\n\nExamples:\n  mcpm server mods source files create --server ./my-server --source modrinth\n  mcpm server mods source files 688231 --source curseforge --all-versions\n  mcpm server mods source files create --source modrinth -N --columns id,filename\n  printf '%s\\n' create ZzjhlDgM | mcpm server mods source files --source modrinth"
)]
pub(super) struct ServerFilesArgs {
    #[command(flatten)]
    server: super::CompatibilityArgs,
    #[command(flatten)]
    files: files::FilesArgs,
}

pub(super) fn run(mut args: ServerFilesArgs) -> Result<()> {
    if let Some((version, loader)) = args.server.defaults()? {
        args.files.apply_defaults(&version, &loader);
    }
    files::run(args.files)
}
