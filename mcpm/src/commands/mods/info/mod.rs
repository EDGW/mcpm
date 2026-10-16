//! Resolves user-selected mod files for shared info inspection.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, loaders::LoaderArgs};

#[derive(Args)]
#[command(
    after_help = "Additional inputs are read from redirected stdin, one per line, using filename, modid, then name matching."
)]
pub(super) struct InfoArgs {
    #[command(flatten)]
    files: inspect::SelectionArgs,
    /// Directory searched by modid/name and --all; relative filenames use this directory.
    #[arg(long, value_name = "PATH", default_value = ".")]
    directory: PathBuf,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: InfoArgs) -> Result<()> {
    let selection = args.files.with_stdin()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    inspect::run(
        selection,
        crate::commands::mod_support::selection::Scope::directory(args.directory),
        args.loaders.select(),
        args.loaders.is_filtered(),
        inspect::Display::Info,
    )
}
