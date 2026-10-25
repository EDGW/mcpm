//! Clap dispatch for standalone remote mod source commands.

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::commands::source_support::{files, identify, search};

#[derive(Args, Debug)]
pub(super) struct SourceArgs {
    #[command(subcommand)]
    command: SourceCommand,
}

#[derive(Subcommand, Debug)]
enum SourceCommand {
    /// Search Minecraft mods; browse interactively or emit TSV with --raw.
    Search(search::SearchArgs),
    /// List published files by project ID or exact slug; stdin accepts one per line.
    #[command(visible_alias = "file")]
    Files(files::FilesArgs),
    /// Identify local files on a remote source; stdin accepts one path per line.
    Identify(identify::IdentifyArgs),
}

pub(super) fn run(args: SourceArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    if ssh.is_requested() && !matches!(args.command, SourceCommand::Identify(_)) {
        anyhow::bail!(
            "--ssh and --ssh-mcpm are supported only by {} mods list/info/deps/source identify",
            crate::program::roots().tools
        );
    }
    match args.command {
        SourceCommand::Search(args) => search::run(args),
        SourceCommand::Files(args) => files::run(args),
        SourceCommand::Identify(args) => identify::run(args, ssh),
    }
}
