mod info;

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub(crate) struct ServerArgs {
    #[command(subcommand)]
    command: ServerCommand,
}

#[derive(Subcommand)]
enum ServerCommand {
    /// Show local server metadata from .mcpm/server.json.
    Info(info::InfoArgs),
}

pub(crate) fn run(args: ServerArgs) -> Result<()> {
    match args.command {
        ServerCommand::Info(args) => info::run(args),
    }
}
