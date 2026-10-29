//! Server-scoped mod commands that resolve inputs through registered server metadata.

use anyhow::Result;
use clap::{Args, Subcommand};

mod cache;
mod check;
mod dep_graph;
mod deps;
mod graph_input;
mod info;
mod input;
mod inspection;
mod list;
mod source;

#[derive(Args, Debug)]
pub(super) struct ModsArgs {
    #[command(subcommand)]
    command: ModsCommand,
}

#[derive(Subcommand, Debug)]
enum ModsCommand {
    /// Build and inspect dependency graphs for a registered server.
    DepGraph(dep_graph::DepGraphArgs),
    /// Check server mod dependencies and report problems.
    Check(check::CheckArgs),
    /// List mods using the server's declared loader.
    List(list::ListArgs),
    /// Cache server mod archives and metadata using the declared loader.
    Cache(cache::CacheArgs),
    /// Show metadata and dependencies of server mod files.
    Info(info::InfoArgs),
    /// Show direct dependency declarations from server mod files.
    Deps(deps::DepsArgs),
    /// Search remote sources, browse compatible files, and identify server mods.
    Source(source::SourceArgs),
}

pub(super) fn run(args: ModsArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    match args.command {
        ModsCommand::DepGraph(args) => {
            anyhow::ensure!(
                !ssh.is_requested(),
                "server mods dep-graph does not support --ssh"
            );
            dep_graph::run(args)
        }
        ModsCommand::Check(args) => {
            anyhow::ensure!(
                !ssh.is_requested(),
                "server mods check does not support --ssh"
            );
            check::run(args)
        }
        ModsCommand::Cache(args) => cache::run(args, ssh),
        ModsCommand::List(args) => list::run(args, ssh),
        ModsCommand::Info(args) => info::run(args, ssh),
        ModsCommand::Deps(args) => deps::run(args, ssh),
        ModsCommand::Source(args) => source::run(args, ssh),
    }
}
