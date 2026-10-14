use clap::{Parser, Subcommand};

use crate::commands::mods::ModArgs;

#[derive(Parser)]
#[command(name = "mcpm", version, about = "Inspect and manage Minecraft mods")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Inspect and manage local mod files.
    #[command(name = "mod")]
    Mod(ModArgs),
}
