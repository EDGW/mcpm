//! Commands for registering and inspecting local Minecraft servers.

use anyhow::Result;
use clap::{Args, Subcommand};

mod info;
mod init;
mod mods;
mod output;

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
    /// Inspect mods associated with a registered local server.
    Mods(mods::ModsArgs),
}

pub(crate) fn run(args: ServerArgs) -> Result<()> {
    match args.command {
        ServerCommand::Init(args) => init::run(args),
        ServerCommand::Info(args) => info::run(args),
        ServerCommand::Mods(args) => mods::run(args),
    }
}
