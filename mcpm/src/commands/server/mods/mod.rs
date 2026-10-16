//! Server-scoped mod commands that resolve inputs through registered server metadata.

use anyhow::Result;
use clap::{Args, Subcommand};

mod deps;
mod info;
mod input;
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
    /// Show metadata and dependencies of server mod files.
    Info(info::InfoArgs),
    /// Show direct dependency declarations from server mod files.
    Deps(deps::DepsArgs),
}

pub(super) fn run(args: ModsArgs) -> Result<()> {
    match args.command {
        ModsCommand::List(args) => list::run(args),
        ModsCommand::Info(args) => info::run(args),
        ModsCommand::Deps(args) => deps::run(args),
    }
}
