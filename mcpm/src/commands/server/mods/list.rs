//! Resolves ordered server mod queries for the shared CLI listing pipeline.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{
    list::{self, ListDisplayArgs, ListLoadingArgs},
    selection::SelectionArgs,
};

use super::super::mod_source::ServerMods;

#[derive(Args, Debug)]
#[command(
    defer(|command| crate::help::grouped(command, &[
        "Global options", "Mode options", "Loading options", "Input options", "Filter", "Style options",
        "TUI-only options", "Console-only options",
    ])),
    after_help = "Additional inputs are read from redirected stdin, one per line, using filename, modid, then name matching. Paths are relative to the server's mods directory; mods/ prefixes are also accepted. With no inputs, list all mods.\n\nModes:\n  TUI (default in a terminal): browse, search and inspect mods; identify outer JARs on Modrinth and CurseForge (default parallelism: 10).\n  Console (--no-ui): print a formatted list.\n  Raw (--raw or redirected stdout): tab-separated output."
)]
pub(super) struct ListArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(
        long = "server",
        value_name = "PATH",
        default_value = ".",
        help_heading = "Input options"
    )]
    path: PathBuf,
    #[command(flatten, next_help_heading = "Input options")]
    selection: SelectionArgs,
    #[command(flatten)]
    display: ListDisplayArgs,
    #[command(flatten)]
    loading: ListLoadingArgs,
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let selection = args.selection.with_stdin()?;
    let source = ServerMods::load(args.path)?;
    list::run_selection(
        selection,
        source.scope(),
        source.loaders(),
        args.loading.parallelism(),
        args.display,
    )
}
