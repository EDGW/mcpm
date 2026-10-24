//! Local backup command dispatch; storage and presentation have independent owners.

use anyhow::Result;
use clap::{Args, Subcommand};

mod apply;
mod create;
mod delete;
mod input;
mod verify;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout). Backups recursively store regular files, including disabled mods and other extensions. Empty directories are omitted. All operations are local."
)]
pub(crate) struct BackupArgs {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a verified, read-only file backup (Console, Raw).
    Create(create::CreateArgs),
    /// Verify and apply a file backup to a directory (Console, Raw).
    Apply(apply::ApplyArgs),
    /// Verify one or more backups without applying them (Console, Raw).
    Verify(verify::VerifyArgs),
    /// Delete one or more backups while preserving cached content (Console, Raw).
    Delete(delete::DeleteArgs),
}

pub(crate) fn run(args: BackupArgs) -> Result<()> {
    match args.command {
        Command::Create(args) => create::run(args),
        Command::Apply(args) => apply::run(args),
        Command::Verify(args) => verify::run(args),
        Command::Delete(args) => delete::run(args),
    }
}

#[derive(Debug, Args)]
struct DisplayArgs {
    /// Include per-file fields in Console output; ignored by Raw.
    #[arg(long)]
    details: bool,
    /// Include the header in Raw TSV output; ignored by Console.
    #[arg(long)]
    headers: bool,
}
