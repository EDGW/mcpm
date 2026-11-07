//! Commands for registering and inspecting Minecraft servers.

use anyhow::Result;
use clap::{Args, Subcommand};

mod after_help;
mod attach;
mod info;
mod init;
mod launch;
mod mods;
mod stash;

#[derive(Args, Debug)]
pub(crate) struct ServerArgs {
    #[command(subcommand)]
    command: ServerCommand,
}

#[derive(Subcommand, Debug)]
enum ServerCommand {
    /// Register a Local Server or a Remote workspace.
    Init(init::InitArgs),
    /// Launch Java in an independent shim; attach by default (TUI, Console, Raw).
    Launch(launch::LaunchArgs),
    /// Attach to an existing Java instance without starting it (TUI, Console, Raw).
    Attach(attach::AttachArgs),
    /// Show server metadata from .mcpm/server.json.
    Info(info::InfoArgs),
    /// Inspect mods associated with a registered server.
    Mods(mods::ModsArgs),
    /// Save, inspect and switch complete Mod snapshots (Console, Raw).
    Stash(stash::StashArgs),
}

pub(crate) fn run(
    args: ServerArgs,
    ssh: crate::ssh::SshArgs,
) -> Result<crate::commands::CommandOutcome> {
    ssh.validate()?;
    let result = match args.command {
        ServerCommand::Launch(args) => return launch::run(args, ssh),
        ServerCommand::Attach(args) => return attach::run(args, ssh),
        ServerCommand::Init(args) => init::run(args, ssh),
        ServerCommand::Info(args) => info::run(args, ssh),
        ServerCommand::Mods(args) => mods::run(args, ssh),
        ServerCommand::Stash(args) => stash::run(args, ssh),
    };
    result.map(|()| crate::commands::CommandOutcome::Completed)
}
