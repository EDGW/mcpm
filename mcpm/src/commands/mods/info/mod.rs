//! Resolves user-selected mod files for shared info inspection.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, loaders::LoaderArgs};

#[derive(Args)]
pub(super) struct InfoArgs {
    /// Paths to .jar or .jar.disabled mod files.
    #[arg(value_name = "FILE", required = true, num_args = 1..)]
    files: Vec<PathBuf>,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: InfoArgs) -> Result<()> {
    inspect::run(
        args.files,
        args.loaders.select(),
        args.loaders.is_filtered(),
        inspect::Display::Info,
    )
}
