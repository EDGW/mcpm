//! Resolves user-selected mod files for shared deps inspection.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, loaders::LoaderArgs, source_path::PathArgs};

#[derive(Args)]
pub(super) struct DepsArgs {
    #[command(flatten)]
    files: inspect::FileSelectionArgs,
    #[command(flatten)]
    loaders: LoaderArgs,
    #[command(flatten)]
    paths_output: PathArgs,
}

pub(super) fn run(args: DepsArgs) -> Result<()> {
    let selection = args.files.with_stdin()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    let files = selection.resolve(Path::new("."))?;
    inspect::run(
        files,
        args.loaders.select(),
        args.loaders.is_filtered(),
        inspect::Display::Dependencies(&args.paths_output),
    )
}
