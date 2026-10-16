//! Commands for inspecting user-selected mod files and directories.

use super::{
    mod_support::{ignored_output, loaders, mod_records, source_path},
    output_style,
};

use anyhow::Result;
use clap::{Args, Subcommand};

mod dep_graph;
mod dependency_output;
mod deps;
mod info;
mod list;

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
