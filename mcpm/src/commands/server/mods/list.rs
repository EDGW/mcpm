//! Resolves a server's mod container for the shared CLI mod listing pipeline.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::list::{self, ListDisplayArgs};

use super::input::ServerMods;

#[derive(Args)]
pub(super) struct ListArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(value_name = "SERVER_DIR_PATH", default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    display: ListDisplayArgs,
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let source = ServerMods::load(args.path)?;
    list::run([source.entries()], source.loaders(), true, args.display)
}
