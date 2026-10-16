//! Server-scoped mod commands that resolve inputs through registered server metadata.

use anyhow::Result;
use clap::{Args, Subcommand};

mod list;

#[derive(Args)]
pub(super) struct ModsArgs {
    #[command(subcommand)]
    command: ModsCommand,
}

#[derive(Subcommand)]
enum ModsCommand {
    /// List mods using the server's declared loader.
    List(list::ListArgs),
}

pub(super) fn run(args: ModsArgs) -> Result<()> {
    match args.command {
        ModsCommand::List(args) => list::run(args),
    }
}
