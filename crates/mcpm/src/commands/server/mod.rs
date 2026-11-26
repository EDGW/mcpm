//! Commands for creating, registering, and managing Minecraft servers.

use anyhow::Result;
use clap::{Args, Subcommand};

mod after_help;
mod attach;
mod git_output;
mod info;
mod init;
mod launch;
mod mods;
mod new;
mod stash;

#[derive(Args, Debug)]
pub(crate) struct ServerArgs {
    #[command(subcommand)]
    command: ServerCommand,
}

#[derive(Subcommand, Debug)]
enum ServerCommand {
    /// Create a vanilla server in a new or empty directory (Console, Raw).
    New(new::NewArgs),
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
    /// Inspect command help and terminal presentation.
    Debug(crate::commands::debug::DebugArgs),
}

pub(crate) fn configure_launch(command: clap::Command, inherited: &[clap::Arg]) -> clap::Command {
    let mut globals = inherited.to_vec();
    globals.extend(
        command
            .get_arguments()
            .filter(|arg| arg.is_global_set())
            .cloned(),
    );
    command.mut_subcommand("launch", |launch| {
        launch::configure_shortcuts(launch, &globals)
    })
}

pub(crate) fn run(
    args: ServerArgs,
    ssh: crate::ssh::SshArgs,
    program: crate::program::Program,
) -> Result<crate::commands::CommandOutcome> {
    ssh.validate()?;
    let result = match args.command {
        ServerCommand::Debug(args) => {
            anyhow::ensure!(
                !ssh.is_requested() || args.supports_ssh(),
                "this debug command runs locally and does not accept --ssh"
            );
            crate::commands::debug::run(args, ssh, program)
        }
        ServerCommand::Launch(args) => return launch::run(args, ssh),
        ServerCommand::Attach(args) => return attach::run(args, ssh),
        ServerCommand::New(args) => new::run(args, ssh),
        ServerCommand::Init(args) => init::run(args, ssh),
        ServerCommand::Info(args) => return info::run(args, ssh),
        ServerCommand::Mods(args) => mods::run(args, ssh),
        ServerCommand::Stash(args) => stash::run(args, ssh),
    };
    result.map(|()| crate::commands::CommandOutcome::Completed)
}
