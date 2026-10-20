//! Lists user-supplied mod paths using the shared CLI listing pipeline.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use crate::commands::mod_support::{
    list::{self, ListDisplayArgs},
    loaders::LoaderArgs,
    stdin,
};

mod input;

#[derive(Args)]
#[command(
    after_help = "Modes:\n  TUI (default in a terminal): browse, search and inspect mods; identify outer JARs on Modrinth and CurseForge (default parallelism: 10).\n  Console (--no-ui): print a formatted list.\n  Raw (--raw or redirected stdout): tab-separated output.\n\nTUI keys: Up/Down, PageUp/PageDown, Home/End; F opens Filter (@name, @modid, @file, @loaders; Esc/Q closes); Left/Right or Space expands/collapses one level; e expands all; Tab switches cards; c copies the selected Details field; Enter opens files for a matched source Details field, or toggles full details; i shows skipped files; q/Esc quits."
)]
pub(super) struct ListArgs {
    /// Mod files or directories; append paths from stdin, one per line. Defaults to . when empty.
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    display: ListDisplayArgs,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let mut paths = stdin::append(args.paths)?;
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let inputs = paths.into_iter().map(|path| {
        let context = format!("failed to read list input {path:?}");
        input::read(path).context(context)
    });
    list::run(
        inputs,
        args.loaders.select(),
        args.loaders.is_filtered(),
        args.display,
    )
}
