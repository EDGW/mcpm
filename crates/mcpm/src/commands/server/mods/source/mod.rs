//! Server-scoped entry points for shared remote source commands.

use std::path::PathBuf;

use crate::ssh::SshArgs;
use anyhow::Result;
use clap::{Args, Subcommand};

mod files;
mod identify;
mod search;

#[derive(Args, Debug)]
pub(super) struct SourceArgs {
    #[command(subcommand)]
    command: SourceCommand,
}

#[derive(Subcommand, Debug)]
enum SourceCommand {
    /// Search projects compatible with the server's Minecraft version and loader.
    Search(search::ServerSearchArgs),
    /// Browse compatible files by remote project ID or exact slug.
    #[command(visible_alias = "file")]
    Files(files::ServerFilesArgs),
    /// Identify selected server mods on a remote source.
    Identify(identify::IdentifyArgs),
}

pub(super) fn run(args: SourceArgs, ssh: SshArgs) -> Result<()> {
    ssh.validate()?;
    match args.command {
        SourceCommand::Search(args) => search::run(args, ssh),
        SourceCommand::Files(args) => files::run(args, ssh),
        SourceCommand::Identify(args) => identify::run(args, ssh),
    }
}

/// Server-derived defaults for remote compatibility queries.
#[derive(Args, Debug)]
struct CompatibilityArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(long = "server", value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Disable automatic compatibility defaults without clearing explicit filters.
    #[arg(long)]
    all_versions: bool,
}

impl CompatibilityArgs {
    fn context(self, path: PathBuf) -> mcpm_backend::operations::mods::source::server::Context {
        mcpm_backend::operations::mods::source::server::Context {
            path,
            default_version: !self.all_versions,
            default_loader: !self.all_versions,
        }
    }
}
