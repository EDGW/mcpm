//! Server-scoped mod commands that resolve inputs through registered server metadata.

use anyhow::Result;
use clap::{Args, Subcommand};

mod check;
mod dep_graph;
mod deps;
mod graph_input;
mod info;
mod input;
mod list;
mod source;

#[derive(Args)]
pub(super) struct ModsArgs {
    #[command(subcommand)]
    command: ModsCommand,
}

#[derive(Subcommand)]
enum ModsCommand {
    /// Build and inspect dependency graphs for a registered server.
    DepGraph(dep_graph::DepGraphArgs),
    /// Check server mod dependencies and report problems.
    Check(check::CheckArgs),
    /// List mods using the server's declared loader.
    List(list::ListArgs),
    /// Show metadata and dependencies of server mod files.
    Info(info::InfoArgs),
    /// Show direct dependency declarations from server mod files.
    Deps(deps::DepsArgs),
    /// Search remote sources, browse compatible files, and identify server mods.
    Source(source::SourceArgs),
}

pub(super) fn run(args: ModsArgs) -> Result<()> {
    match args.command {
        ModsCommand::DepGraph(args) => dep_graph::run(args),
        ModsCommand::Check(args) => check::run(args),
        ModsCommand::List(args) => list::run(args),
        ModsCommand::Info(args) => info::run(args),
        ModsCommand::Deps(args) => deps::run(args),
        ModsCommand::Source(args) => source::run(args),
    }
}
