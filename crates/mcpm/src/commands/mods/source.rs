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
    /// Identify files with source providers on the local or SSH execution host; stdin accepts one path per line.
    Identify(identify::IdentifyArgs),
}

pub(super) fn run(args: SourceArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    match args.command {
        SourceCommand::Search(args) => search::run(args, ssh),
        SourceCommand::Files(args) => files::run(args, ssh),
        SourceCommand::Identify(args) => identify::run(args, ssh),
    }
}
