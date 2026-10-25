//! Commands for registering and inspecting Minecraft servers.

use anyhow::Result;
use clap::{Args, Subcommand};

mod info;
mod init;
mod metadata;
mod mod_source;
mod mods;
mod output;
mod stash;
mod target;
mod workspace;

#[derive(Args, Debug)]
pub(crate) struct ServerArgs {
    #[command(subcommand)]
    command: ServerCommand,
}

#[derive(Subcommand, Debug)]
enum ServerCommand {
    /// Register a Local Server or a Remote workspace.
    Init(init::InitArgs),
    /// Show server metadata from .mcpm/server.json.
    Info(info::InfoArgs),
    /// Inspect mods associated with a registered server.
    Mods(mods::ModsArgs),
    /// Save, inspect and switch complete Mod snapshots (Console, Raw; local only).
    Stash(stash::StashArgs),
}

pub(crate) fn run(args: ServerArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    ssh.validate()?;
    match args.command {
        ServerCommand::Init(args) => init::run(args, ssh),
        ServerCommand::Info(args) => info::run(args, ssh),
        ServerCommand::Mods(args) => mods::run(args, ssh),
        ServerCommand::Stash(args) => stash::run(args, ssh),
    }
}
