//! Top-level explicit cache command dispatch.

use anyhow::Result;
use clap::{Args, Subcommand};

mod mods;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout). Cache management is local and explicit."
)]
pub(crate) struct CacheArgs {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage cached Mod content and named metadata (Console, Raw).
    Mods(mods::ModsArgs),
}

pub(crate) fn run(args: CacheArgs) -> Result<()> {
    match args.command {
        Command::Mods(args) => mods::run(args),
    }
}
