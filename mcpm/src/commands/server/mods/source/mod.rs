//! Server-scoped entry points for shared remote source commands.

use std::path::PathBuf;

use crate::ssh::SshArgs;
use anyhow::Result;
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use super::super::mod_source::ServerMods;

mod files;
mod identify;
mod remote;
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
    /// Disable both Minecraft version and loader compatibility filters.
    #[arg(long, conflicts_with_all = ["game_version", "loader"])]
    all_versions: bool,
}

impl CompatibilityArgs {
    fn defaults(self, ssh: SshArgs, command: &'static str) -> Result<Option<Compatibility>> {
        let (path, ssh) = crate::commands::server::target::resolve(self.path, ssh)?;
        let defaults = if ssh.is_requested() {
            remote::defaults(ssh, path, command)?
        } else {
            Compatibility::load(path)?
        };
        Ok((!self.all_versions).then_some(defaults))
    }
}

/// Server-owned compatibility data shared by local resolution and remote transport.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Compatibility {
    version: String,
    loader: String,
}
impl Compatibility {
    fn load(path: PathBuf) -> Result<Self> {
        let server = ServerMods::load(path)?;
        Ok(Self {
            version: server.minecraft_version().to_owned(),
            loader: server.container().loader().loader.id().as_str().to_owned(),
        })
    }
}
