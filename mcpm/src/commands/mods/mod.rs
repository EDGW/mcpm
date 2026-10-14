mod info;
mod list;
mod loaders;

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub(crate) struct ModArgs {
    #[command(subcommand)]
    command: ModCommand,
}

#[derive(Subcommand)]
enum ModCommand {
    /// Show metadata and dependencies from a local mod file.
    Info(info::InfoArgs),
    /// List local mod files with loader colors.
    List(list::ListArgs),
}

pub(crate) fn run(args: ModArgs) -> Result<()> {
    match args.command {
        ModCommand::Info(args) => info::run(args),
        ModCommand::List(args) => list::run(args),
    }
}
