mod build;
mod selector;
mod test;
mod view;

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub(super) struct DepGraphArgs {
    #[command(subcommand)]
    command: DepGraphCommand,
}

#[derive(Subcommand)]
enum DepGraphCommand {
    /// Build a dependency graph from a mod directory and export JSON.
    Build(build::BuildArgs),
    /// Validate a saved graph, restore its loaders, and show graph statistics.
    Test(test::TestArgs),
    /// Show a mod declaration graph as a colored dependency tree.
    View(view::ViewArgs),
}

pub(super) fn run(args: DepGraphArgs) -> Result<()> {
    match args.command {
        DepGraphCommand::Build(args) => build::run(args),
        DepGraphCommand::Test(args) => test::run(args),
        DepGraphCommand::View(args) => view::run(args),
    }
}
