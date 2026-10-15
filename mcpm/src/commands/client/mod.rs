mod check;
mod input;
mod list;
mod output;

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
    /// List normal and broken client profiles with loader versions.
    List(list::ListArgs),
}

pub(crate) fn run(args: ClientArgs) -> Result<()> {
    match args.command {
        ClientCommand::Check(args) => check::run(args),
        ClientCommand::List(args) => list::run(args),
    }
}
