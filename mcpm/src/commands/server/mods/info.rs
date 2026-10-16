//! Server-scoped info using shared mod inspection and presentation.

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::inspect;

use super::input::FileArgs;

#[derive(Args)]
pub(super) struct InfoArgs {
    #[command(flatten)]
    files: FileArgs,
}

pub(super) fn run(args: InfoArgs) -> Result<()> {
    let (source, paths) = args.files.resolve()?;
    inspect::run(paths, source.loaders(), true, inspect::Display::Info)
}
