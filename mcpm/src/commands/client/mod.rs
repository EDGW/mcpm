mod check;
mod input;
mod launch;
mod list;
mod output;
mod platform;
mod selector;

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub(crate) struct ClientArgs {
    #[command(subcommand)]
    command: ClientCommand,
}

#[derive(Subcommand)]
enum ClientCommand {
    /// Check local client libraries and assets, optionally selecting a version interactively.
    Check(check::CheckArgs),
    /// Launch local clients; omit IDs to select one interactively.
    Launch(Box<launch::LaunchArgs>),
    /// List normal and broken client profiles with loader versions.
    List(list::ListArgs),
}

pub(crate) fn run(args: ClientArgs) -> Result<()> {
    match args.command {
        ClientCommand::Check(args) => check::run(args),
        ClientCommand::Launch(args) => launch::run(*args),
        ClientCommand::List(args) => list::run(args),
    }
}
