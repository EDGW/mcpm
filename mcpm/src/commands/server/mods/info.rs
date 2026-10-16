//! Server-scoped info using shared mod inspection and presentation.

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::inspect;

use super::input::FileArgs;

#[derive(Args)]
#[command(after_help = "Additional mod file paths are read from redirected stdin, one per line.")]
pub(super) struct InfoArgs {
    #[command(flatten)]
    files: FileArgs,
}

pub(super) fn run(args: InfoArgs) -> Result<()> {
    let Some((source, paths)) = args.files.with_stdin()?.resolve()? else {
        return Ok(());
    };
    inspect::run(paths, source.loaders(), true, inspect::Display::Info)
}
