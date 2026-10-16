//! Resolves user-selected mod files for shared deps inspection.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, loaders::LoaderArgs, source_path::PathArgs};

#[derive(Args)]
#[command(
    after_help = "Additional inputs are read from redirected stdin, one per line; --input sets their interpretation (default: filename, modid, then name matching)."
)]
pub(super) struct DepsArgs {
    #[command(flatten)]
    files: inspect::SelectionArgs,
    #[command(flatten)]
    stdin: crate::commands::mod_support::selection::StdinArgs,
    /// Directory searched by modid/name and --all; relative filenames use this directory.
    #[arg(long, value_name = "PATH", default_value = ".")]
    directory: PathBuf,
    #[command(flatten)]
    loaders: LoaderArgs,
    #[command(flatten)]
    paths_output: PathArgs,
}

pub(super) fn run(args: DepsArgs) -> Result<()> {
    let selection = args.stdin.read(args.files)?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    inspect::run(
        selection,
        crate::commands::mod_support::selection::Scope::directory(args.directory),
        args.loaders.select(),
        args.loaders.is_filtered(),
        inspect::Display::Dependencies(&args.paths_output),
    )
}
