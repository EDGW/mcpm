use clap::{Parser, Subcommand};

use crate::commands::{client::ClientArgs, mods::ModArgs};

#[derive(Parser)]
#[command(
    name = "mcpm",
    version,
    about = "Inspect and manage Minecraft clients and mods"
)]
pub(crate) struct Cli {
    /// Disable terminal formatting, colors, progress indicators, and hints.
    #[arg(long, global = true)]
    pub(crate) raw: bool,
    /// Diagnostic log level on stderr; does not filter game stdout/stderr.
    #[arg(long, global = true, value_enum, default_value = "warn")]
    pub(crate) log_level: crate::logging::LogLevel,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Inspect and launch local Minecraft clients.
    Client(ClientArgs),
    /// Inspect and manage local mod files.
    #[command(name = "mod")]
    Mod(ModArgs),
}
