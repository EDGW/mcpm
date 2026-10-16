//! Server-scoped deps using shared mod inspection and presentation.

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, source_path::PathArgs};

use super::input::FileArgs;

#[derive(Args)]
#[command(
    after_help = "Additional mod filenames or mods/FILE paths are read from redirected stdin, one per line."
)]
pub(super) struct DepsArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    paths_output: PathArgs,
}

pub(super) fn run(args: DepsArgs) -> Result<()> {
    let Some((source, paths)) = args.files.with_stdin()?.resolve()? else {
        return Ok(());
    };
    inspect::run(
        paths,
        source.loaders(),
        true,
        inspect::Display::Dependencies(&args.paths_output),
    )
}
