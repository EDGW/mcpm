//! Commands for inspecting local and remote mods and searching remote mod sources.

use anyhow::Result;
use clap::{Args, Subcommand};

mod backup;
mod cache;
mod dep_graph;
mod deps;
mod info;
mod inspection;
mod list;
mod source;

#[derive(Args, Debug)]
pub(crate) struct ModsArgs {
    #[command(subcommand)]
    command: ModsCommand,
}

#[derive(Subcommand, Debug)]
enum ModsCommand {
    /// Create, verify, compare, checksum, apply and delete Mod file backups (Console, Raw).
    Backup(backup::BackupArgs),
    /// Store selected mod files and identification metadata (Console, Raw).
    Cache(cache::CacheArgs),
    /// Build and export local mod dependency graphs.
    DepGraph(dep_graph::DepGraphArgs),
    /// Show direct dependency declarations (Console, Raw; Remote supported).
    Deps(deps::DepsArgs),
    /// Show metadata and dependencies (Console, Raw; Remote supported).
    Info(info::InfoArgs),
    /// Browse local mod files (TUI, Console, Raw; Remote supported).
    List(list::ListArgs),
    /// Search remote mod sources.
    Source(source::SourceArgs),
}

pub(crate) fn run(args: ModsArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    if ssh.is_requested()
        && matches!(
            args.command,
            ModsCommand::DepGraph(_) | ModsCommand::Cache(_) | ModsCommand::Backup(_)
        )
    {
        anyhow::bail!(
            "--ssh and --ssh-mcpm are currently supported only by {} mods list/info/deps/source identify",
            crate::program::roots().tools
        );
    }
    match args.command {
        ModsCommand::Cache(args) => cache::run(args),
        ModsCommand::Backup(args) => backup::run(args),
        ModsCommand::DepGraph(args) => dep_graph::run(args),
        ModsCommand::Deps(args) => deps::run(args, ssh),
        ModsCommand::Info(args) => info::run(args, ssh),
        ModsCommand::List(list) => list::run(list, ssh),
        ModsCommand::Source(args) => source::run(args, ssh),
    }
}
