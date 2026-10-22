//! Server-scoped deps using shared mod inspection and presentation.

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, source_path::PathArgs};

use super::input::FileArgs;

#[derive(Args, Debug)]
#[command(
    after_help = "Additional inputs are read from redirected stdin, one per line; --input sets their interpretation (default: filename, modid, then name matching)."
)]
pub(super) struct DepsArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    paths_output: PathArgs,
}

pub(super) fn run(args: DepsArgs) -> Result<()> {
    let Some((source, selection)) = args.files.with_stdin()?.resolve()? else {
        return Ok(());
    };
    inspect::run(
        selection,
        source.scope(),
        source.loaders(),
        true,
        crate::commands::execution::default_parallelism().get(),
        inspect::Display::Dependencies(&args.paths_output),
    )
}
