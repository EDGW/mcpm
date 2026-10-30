//! Commands for registering and inspecting Minecraft servers.

use anyhow::Result;
use clap::{Args, Subcommand};

mod info;
mod init;
mod launch;
mod mod_source;
mod mods;
mod stash;
mod target;

#[derive(Args, Debug)]
pub(crate) struct ServerArgs {
    #[command(subcommand)]
    command: ServerCommand,
}

#[derive(Subcommand, Debug)]
enum ServerCommand {
    /// Register a server workspace or host path.
    Init(init::InitArgs),
    /// Start the server shim; attach to its console unless --detach is used.
    Launch(launch::LaunchArgs),
    /// Show server metadata from .mcpm/server.json.
    Info(info::InfoArgs),
    /// Inspect mods associated with a registered server.
    Mods(mods::ModsArgs),
    /// Save, inspect and switch complete Mod snapshots (Console, Raw; not migrated to SSH).
    Stash(stash::StashArgs),
}

pub(crate) fn run(
    args: ServerArgs,
    ssh: crate::ssh::SshArgs,
) -> Result<crate::commands::CommandOutcome> {
    ssh.validate()?;
    let result = match args.command {
        ServerCommand::Launch(args) => return launch::run(args, ssh),
        ServerCommand::Info(args) => info::run(args, ssh),
        ServerCommand::Mods(args) => mods::run(args, ssh),
        ServerCommand::Init(args) => init::run(args, ssh),
        ServerCommand::Stash(args) => {
            anyhow::ensure!(!ssh.is_requested(), "server stash does not support SSH yet; this command has not been migrated");
            stash::run(args, ssh)
        }
    };
    result.map(|()| crate::commands::CommandOutcome::Completed)
}
