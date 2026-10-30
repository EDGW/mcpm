//! CLI command dispatch and shared presentation support.

pub(crate) mod cache;
mod cache_support;
pub(crate) mod client;
mod confirm;
pub(crate) mod customize;
pub(crate) mod debug;
pub(crate) mod execution;
pub(crate) mod mods;
pub(crate) mod server;
mod source_support;
mod stdin;
mod supports;

use anyhow::Result;

use clap::Subcommand;

use self::{client::ClientArgs, debug::DebugArgs, mods::ModsArgs, server::ServerArgs};

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Manage explicitly stored cache objects (Console, Raw).
    Cache(crate::commands::cache::CacheArgs),
    /// Customize terminal themes and loader colors (TUI, Console, Raw).
    Customize(crate::commands::customize::CustomizeArgs),
    /// Inspect mcpm's terminal presentation and diagnostics.
    Debug(DebugArgs),
    /// Inspect and launch local Minecraft clients.
    Client(ClientArgs),
    /// Inspect local Minecraft servers.
    Server(ServerArgs),
    /// Inspect local mod files and search remote sources.
    Mods(ModsArgs),
}

/// A child process status remains a successful command operation with its own exit code.
pub(crate) enum CommandOutcome {
    Completed,
    Exited(u32),
}


pub(crate) fn run(command: Command, ssh: crate::ssh::SshArgs) -> Result<CommandOutcome> {
    let result = match command {
        Command::Customize(args) => {
            anyhow::ensure!(!ssh.is_requested(), "customize does not support SSH yet; this command has not been migrated");
            customize::run(args)
        }
        Command::Debug(args) => {
            anyhow::ensure!(!ssh.is_requested(), "debug does not support SSH yet; this command has not been migrated");
            debug::run(args)
        }
        Command::Mods(args) => mods::run(args, ssh),
        Command::Client(args) => {
            anyhow::ensure!(!ssh.is_requested(), "client does not support SSH yet; this command has not been migrated");
            client::run(args)
        }
        Command::Cache(args) => {
            anyhow::ensure!(!ssh.is_requested(), "cache does not support SSH yet; this command has not been migrated");
            cache::run(args)
        }
        Command::Server(args) => return server::run(args, ssh),
    };
    result.map(|()| CommandOutcome::Completed)
}
