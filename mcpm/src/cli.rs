//! Top-level CLI arguments and command selection.

use clap::{Parser, Subcommand};

use crate::commands::{client::ClientArgs, debug::DebugArgs, mods::ModArgs, server::ServerArgs};

#[derive(Parser, Debug)]
#[command(
    name = "mcpm",
    version,
    about = "Inspect and manage Minecraft clients, servers and mods"
)]
pub(crate) struct Cli {
    /// Disable TUI interaction, formatting, colors, progress, and hints; otherwise each output stream follows its own TTY status.
    #[arg(long, global = true, help_heading = "Mode options")]
    pub(crate) raw: bool,
    /// Diagnostic log level on stderr; does not filter game stdout/stderr.
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "warn",
        help_heading = "Global options"
    )]
    pub(crate) log_level: crate::logging::LogLevel,
    /// Show remote logs and remote error diagnostics on stderr; logs follow --log-level.
    #[arg(long, global = true, help_heading = "Global options")]
    pub(crate) remote_logs: bool,
    #[command(flatten)]
    pub(crate) ssh: crate::ssh::SshArgs,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
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
    #[command(name = "mod")]
    Mod(ModArgs),
}

/// Process startup options are CLI concerns; request arguments use command-owned schemas.
#[derive(clap::Args, Debug)]
pub(crate) struct RemoteArgs {
    /// Maximum concurrent remote commands; excess requests return busy.
    #[arg(long, default_value = "8")]
    pub(crate) max_concurrency: std::num::NonZeroUsize,
}
