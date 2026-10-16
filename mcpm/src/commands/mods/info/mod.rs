//! Resolves user-selected mod files for shared info inspection.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, loaders::LoaderArgs};

#[derive(Args)]
#[command(after_help = "Additional mod file paths are read from redirected stdin, one per line.")]
pub(super) struct InfoArgs {
    #[command(flatten)]
    files: inspect::FileSelectionArgs,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: InfoArgs) -> Result<()> {
    let selection = args.files.with_stdin()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    let files = selection.resolve(Path::new("."))?;
    inspect::run(
        files,
        args.loaders.select(),
        args.loaders.is_filtered(),
        inspect::Display::Info,
    )
}
