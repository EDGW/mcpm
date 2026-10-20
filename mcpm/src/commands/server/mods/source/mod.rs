//! Server-scoped entry points for shared remote source commands.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};

use super::super::mod_source::ServerMods;

mod files;
mod identify;
mod search;

#[derive(Args)]
pub(super) struct SourceArgs {
    #[command(subcommand)]
    command: SourceCommand,
}

#[derive(Subcommand)]
enum SourceCommand {
    /// Search projects compatible with the server's Minecraft version and loader.
    Search(search::ServerSearchArgs),
    /// Browse compatible files by remote project ID or exact slug.
    #[command(visible_alias = "file")]
    Files(files::ServerFilesArgs),
    /// Identify selected local server mods on a remote source.
    Identify(identify::IdentifyArgs),
}

pub(super) fn run(args: SourceArgs) -> Result<()> {
    match args.command {
        SourceCommand::Search(args) => search::run(args),
        SourceCommand::Files(args) => files::run(args),
        SourceCommand::Identify(args) => identify::run(args),
    }
}

/// Server-derived defaults for remote compatibility queries.
#[derive(Args)]
struct CompatibilityArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(long = "server", value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Disable both Minecraft version and loader compatibility filters.
    #[arg(long, conflicts_with_all = ["game_version", "loader"])]
    all_versions: bool,
}

impl CompatibilityArgs {
    fn defaults(self) -> Result<Option<(String, String)>> {
        let server = ServerMods::load(self.path)?;
        Ok((!self.all_versions).then(|| {
            (
                server.minecraft_version().to_owned(),
                server.container().loader().loader.id().as_str().to_owned(),
            )
        }))
    }
}
