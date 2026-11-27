//! CLI command dispatch and shared presentation support.

pub(crate) mod cache;
mod cache_support;
pub(crate) mod client;
mod confirm;
pub(crate) mod crash_report;
pub(crate) mod customize;
pub(crate) mod debug;
mod execution;
pub(crate) mod mods;
pub(crate) mod server;
mod source_support;
mod stdin;
mod supports;

use anyhow::Result;

use clap::Subcommand;

use self::{
    client::ClientArgs, crash_report::CrashReportArgs, debug::DebugArgs, mods::ModsArgs,
    server::ServerArgs,
};

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Manage explicitly stored cache objects (Console, Raw).
    Cache(crate::commands::cache::CacheArgs),
    /// Inspect Minecraft crash report files (Console, Raw).
    CrashReport(CrashReportArgs),
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

pub(crate) fn run(
    command: Command,
    ssh: crate::ssh::SshArgs,
    program: crate::program::Program,
) -> Result<CommandOutcome> {
    ssh.validate()?;
    let supports_ssh = match &command {
        Command::Mods(_) | Command::Server(_) | Command::Cache(_) | Command::CrashReport(_) => true,
        Command::Debug(args) => args.supports_ssh(),
        Command::Customize(_) | Command::Client(_) => false,
    };
    if ssh.is_requested() && !supports_ssh {
        anyhow::bail!(
            "--ssh and --ssh-mcpmd are supported by {} cache/mods/crash-report/debug lock and {} init/info/stash/mods/crash-report",
            crate::program::roots().tools,
            crate::program::roots().server
        );
    }
    let result = match command {
        Command::Customize(args) => customize::run(args),
        Command::CrashReport(args) => crash_report::run(args, ssh),
        Command::Debug(args) => debug::run(args, ssh, program),
        Command::Mods(args) => mods::run(args, ssh),
        Command::Client(args) => client::run(args),
        Command::Cache(args) => cache::run(args, ssh),
        Command::Server(args) => return server::run(args, ssh, program),
    };
    result.map(|()| CommandOutcome::Completed)
}
