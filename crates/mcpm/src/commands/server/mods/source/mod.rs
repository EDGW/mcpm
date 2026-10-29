//! Server-scoped entry points for shared remote source commands.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use mcpm_backend::server::types::ModsContext;
use mcpm_backend::server::{ContextInput, ModsContextOperation};

use crate::commands::execution::{failure as operation_failure, Runtime};
use crate::ssh::SshArgs;

use super::super::target;

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
    /// Disable both Minecraft version and loader compatibility filters.
    #[arg(long, conflicts_with_all = ["game_version", "loader"])]
    all_versions: bool,
}

impl CompatibilityArgs {
    fn defaults(self, ssh: SshArgs) -> Result<Option<ModsContext>> {
        let target = target::resolve(self.path, ssh)?;
        let remote = target.session.target().is_remote();
        let runtime = Runtime::new()?;
        let context = runtime.block_on(async {
            let handle = target
                .session
                .start::<ModsContextOperation>(ContextInput { path: target.path })
                .await
                .map_err(|failure| operation_failure(failure, remote))?;
            handle
                .finish()
                .await
                .map_err(|failure| operation_failure(failure, remote))
        })?;
        crate::commands::execution::finish(&runtime, &target.session)?;
        Ok((!self.all_versions).then_some(context))
    }
}
