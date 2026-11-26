//! Debug command arguments and dispatch, isolated from operational commands.

use anyhow::Result;
use clap::{Args, Subcommand};

mod all_help;
mod lock;
mod palette;

#[derive(Args, Debug)]
pub(crate) struct DebugArgs {
    #[command(subcommand)]
    command: DebugCommand,
}

impl DebugArgs {
    pub(crate) fn supports_ssh(&self) -> bool {
        matches!(self.command, DebugCommand::Lock(_))
    }
}

#[derive(Subcommand, Debug)]
enum DebugCommand {
    /// Preview the 16 terminal colors and default foreground (TUI, Console, Raw).
    Palette(palette::PaletteArgs),
    /// Print long help for the root, every command group, and every command (Console, Raw).
    AllHelp,
    /// Acquire and hold mcpm locks for concurrency debugging.
    Lock(lock::LockArgs),
}

pub(crate) fn run(
    args: DebugArgs,
    ssh: crate::ssh::SshArgs,
    program: crate::program::Program,
) -> Result<()> {
    match args.command {
        DebugCommand::Palette(args) => palette::run(args),
        DebugCommand::AllHelp => all_help::run(program),
        DebugCommand::Lock(args) => lock::run(args, ssh),
    }
}
