//! Commands for inspecting local mods and searching remote mod sources.

use anyhow::Result;
use clap::{Args, Subcommand};

mod dep_graph;
mod deps;
mod info;
mod list;
mod source;

#[derive(Args, Debug)]
pub(crate) struct ModArgs {
    #[command(subcommand)]
    command: ModCommand,
}

#[derive(Subcommand, Debug)]
enum ModCommand {
    /// Build and export local mod dependency graphs.
    DepGraph(dep_graph::DepGraphArgs),
    /// Show direct dependency declarations from local mod files.
    Deps(deps::DepsArgs),
    /// Show metadata and dependencies from a local mod file.
    Info(info::InfoArgs),
    /// Browse local mod files (TUI, Console, Raw; Remote supported).
    List(list::ListArgs),
    /// Search remote mod sources.
    Source(source::SourceArgs),
}

pub(crate) fn run(args: ModArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    if ssh.is_requested() && !matches!(args.command, ModCommand::List(_) | ModCommand::Source(_)) {
        anyhow::bail!(
            "--ssh and --ssh-mcpm are currently supported only by mcpm mod list and mod source identify"
        );
    }
    match args.command {
        ModCommand::DepGraph(args) => dep_graph::run(args),
        ModCommand::Deps(args) => deps::run(args),
        ModCommand::Info(args) => info::run(args),
        ModCommand::List(list) => list::run(list, ssh),
        ModCommand::Source(args) => source::run(args, ssh),
    }
}
