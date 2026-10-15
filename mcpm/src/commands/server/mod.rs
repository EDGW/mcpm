mod info;
mod init;
mod output;

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub(crate) struct ServerArgs {
    #[command(subcommand)]
    command: ServerCommand,
}

#[derive(Subcommand)]
enum ServerCommand {
    /// Initialize .mcpm/server.json from a verified start script or explicit metadata.
    Init(init::InitArgs),
    /// Show local server metadata from .mcpm/server.json.
    Info(info::InfoArgs),
}

pub(crate) fn run(args: ServerArgs) -> Result<()> {
    match args.command {
        ServerCommand::Init(args) => init::run(args),
        ServerCommand::Info(args) => info::run(args),
    }
}
