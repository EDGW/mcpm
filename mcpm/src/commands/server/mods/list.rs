//! Resolves a server's mod container for the shared CLI mod listing pipeline.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use kako_mods_lib::local::mcmod::ModEntry;

use crate::commands::mod_support::{
    list::{self, ListDisplayArgs},
    stdin,
};

use super::input::ServerMods;

#[derive(Args)]
#[command(
    after_help = "Mod filenames or mods/FILE paths may be read from redirected stdin, one per line, to select files within this server."
)]
pub(super) struct ListArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(value_name = "SERVER_DIR_PATH", default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    display: ListDisplayArgs,
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let source = ServerMods::load(args.path)?;
    let files = stdin::append(Vec::new())?;
    if files.is_empty() {
        list::run([source.entries()], source.loaders(), true, args.display)
    } else {
        let inputs = source
            .files(files)?
            .into_iter()
            .map(|path| ModEntry::new(path).map(|entry| vec![entry]));
        list::run(inputs, source.loaders(), true, args.display)
    }
}
