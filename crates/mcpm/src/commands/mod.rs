//! CLI command dispatch and shared presentation support.

pub(crate) mod cache;
mod cache_support;
pub(crate) mod client;
mod confirm;
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

use self::{client::ClientArgs, debug::DebugArgs, mods::ModsArgs, server::ServerArgs};

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Manage explicitly stored cache objects (Console, Raw).
    Cache(crate::commands::cache::CacheArgs),
    /// Serve concurrent commands over one persistent stdin/stdout JSON session (Raw).
    Remote(RemoteArgs),
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

/// Process startup options are CLI concerns; request arguments use command-owned schemas.
#[derive(clap::Args, Debug)]
pub(crate) struct RemoteArgs {
    /// Maximum concurrent remote commands; excess requests return busy.
    #[arg(long, default_value = "8")]
    pub(crate) max_concurrency: std::num::NonZeroUsize,
}

/// A child process status remains a successful command operation with its own exit code.
pub(crate) enum CommandOutcome {
    Completed,
    Exited(u32),
}

pub(crate) fn run(command: Command, ssh: crate::ssh::SshArgs) -> Result<CommandOutcome> {
    if ssh.is_requested() && !matches!(command, Command::Mods(_) | Command::Server(_)) {
        anyhow::bail!(
            "--ssh and --ssh-mcpm are currently supported only by {} mods list/info/deps/source identify and {} init/info/mods list/info/cache/deps/source",
            crate::program::roots().tools,
            crate::program::roots().server
        );
    }
    let result = match command {
        Command::Remote(args) => {
            let registry = crate::remote::registry::registered();
            crate::remote::server::run(args.max_concurrency, registry)
        }
        Command::Customize(args) => customize::run(args),
        Command::Debug(args) => debug::run(args),
        Command::Mods(args) => mods::run(args, ssh),
        Command::Client(args) => client::run(args),
        Command::Cache(args) => cache::run(args),
        Command::Server(args) => return server::run(args, ssh),
    };
    result.map(|()| CommandOutcome::Completed)
}
