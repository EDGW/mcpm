//! Resolves user-selected mod files for shared deps inspection.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, loaders::LoaderArgs, source_path::PathArgs};

#[derive(Args)]
pub(super) struct DepsArgs {
    /// Paths to .jar or .jar.disabled mod files.
    #[arg(value_name = "FILE", required = true, num_args = 1..)]
    files: Vec<PathBuf>,
    #[command(flatten)]
    loaders: LoaderArgs,
    #[command(flatten)]
    paths_output: PathArgs,
}

pub(super) fn run(args: DepsArgs) -> Result<()> {
    inspect::run(
        args.files,
        args.loaders.select(),
        args.loaders.is_filtered(),
        inspect::Display::Dependencies(&args.paths_output),
    )
}
