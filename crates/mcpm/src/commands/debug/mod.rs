//! Debug command arguments and dispatch, isolated from operational commands.

use anyhow::Result;
use clap::{Args, Subcommand};

mod all_help;
mod palette;

#[derive(Args, Debug)]
pub(crate) struct DebugArgs {
    #[command(subcommand)]
    command: DebugCommand,
}

#[derive(Subcommand, Debug)]
enum DebugCommand {
    /// Preview the 16 terminal colors and default foreground (TUI, Console, Raw).
    Palette(palette::PaletteArgs),
    /// Print long help for the root, every command group, and every command (Console, Raw).
    AllHelp,
}

pub(crate) fn run(args: DebugArgs, program: crate::program::Program) -> Result<()> {
    match args.command {
        DebugCommand::Palette(args) => palette::run(args),
        DebugCommand::AllHelp => all_help::run(program),
    }
}
