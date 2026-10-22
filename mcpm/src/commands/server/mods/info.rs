//! Server-scoped info using shared mod inspection and presentation.

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::inspect;

use super::input::FileArgs;

#[derive(Args, Debug)]
#[command(
    after_help = "Additional inputs are read from redirected stdin, one per line; --input sets their interpretation (default: filename, modid, then name matching)."
)]
pub(super) struct InfoArgs {
    #[command(flatten)]
    files: FileArgs,
}

pub(super) fn run(args: InfoArgs) -> Result<()> {
    let Some((source, selection)) = args.files.with_stdin()?.resolve()? else {
        return Ok(());
    };
    inspect::run(
        selection,
        source.scope(),
        source.loaders(),
        true,
        inspect::Display::Info,
    )
}
