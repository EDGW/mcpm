//! Commands for registering and inspecting Minecraft servers.

use anyhow::Result;
use clap::{Args, Subcommand};

mod info;
mod init;
mod metadata;
mod mod_source;
mod mods;
mod output;

#[derive(Args, Debug)]
pub(crate) struct ServerArgs {
    #[command(subcommand)]
    command: ServerCommand,
}

#[derive(Subcommand, Debug)]
enum ServerCommand {
    /// Initialize .mcpm/server.json from a verified start script or explicit metadata.
    Init(init::InitArgs),
    /// Show server metadata from .mcpm/server.json.
    Info(info::InfoArgs),
    /// Inspect mods associated with a registered server.
    Mods(mods::ModsArgs),
}

pub(crate) fn run(args: ServerArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    ssh.validate()?;
    match args.command {
        ServerCommand::Init(args) => init::run(args, ssh),
        ServerCommand::Info(args) => info::run(args, ssh),
        ServerCommand::Mods(args) => mods::run(args, ssh),
    }
}
