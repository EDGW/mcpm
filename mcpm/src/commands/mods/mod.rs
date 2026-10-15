mod dep_graph;
mod dependency_output;
mod deps;
mod ignored_output;
mod info;
mod list;
mod loaders;
mod mod_records;
mod output_style;
mod source_path;

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub(crate) struct ModArgs {
    #[command(subcommand)]
    command: ModCommand,
}

#[derive(Subcommand)]
enum ModCommand {
    /// Build and export local mod dependency graphs.
    DepGraph(dep_graph::DepGraphArgs),
    /// Show direct dependency declarations from local mod files.
    Deps(deps::DepsArgs),
    /// Show metadata and dependencies from a local mod file.
    Info(info::InfoArgs),
    /// List local mod files with loader colors.
    List(list::ListArgs),
}

pub(crate) fn run(args: ModArgs) -> Result<()> {
    match args.command {
        ModCommand::DepGraph(args) => dep_graph::run(args),
        ModCommand::Deps(args) => deps::run(args),
        ModCommand::Info(args) => info::run(args),
        ModCommand::List(args) => list::run(args),
    }
}
