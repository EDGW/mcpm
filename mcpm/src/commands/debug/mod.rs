//! Debug command arguments and dispatch, isolated from operational commands.

use anyhow::Result;
use clap::{Args, Subcommand};

mod palette;

#[derive(Args)]
pub(crate) struct DebugArgs {
    #[command(subcommand)]
    command: DebugCommand,
}

#[derive(Subcommand)]
enum DebugCommand {
    /// Preview the 16 terminal colors and default foreground (TUI, Console, Raw).
    Palette(palette::PaletteArgs),
}

pub(crate) fn run(args: DebugArgs) -> Result<()> {
    match args.command {
        DebugCommand::Palette(args) => palette::run(args),
    }
}
