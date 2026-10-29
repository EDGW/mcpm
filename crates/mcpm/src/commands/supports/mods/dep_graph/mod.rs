//! Shared dependency graph commands, with input-specific build arguments supplied by each scope.

use anyhow::Result;
use clap::{Args, Subcommand};

mod build;
mod selector;
mod test;
mod view;

pub(in crate::commands) use build::{BuildOptions, build_graph};
pub(in crate::commands) use view::write_problems;

#[derive(Args, Debug)]
pub(in crate::commands) struct DepGraphArgs<B: Args> {
    #[command(subcommand)]
    command: DepGraphCommand<B>,
}

#[derive(Subcommand, Debug)]
enum DepGraphCommand<B: Args> {
    /// Build a dependency graph and export JSON.
    Build(B),
    /// Validate a saved graph, restore its loaders, and show graph statistics.
    Test(test::TestArgs),
    /// Show a mod declaration graph as a colored dependency tree.
    View(view::ViewArgs),
}

impl<B: Args> DepGraphArgs<B> {
    pub(in crate::commands) fn run(self, build: impl FnOnce(B) -> Result<()>) -> Result<()> {
        match self.command {
            DepGraphCommand::Build(args) => build(args),
            DepGraphCommand::Test(args) => test::run(args),
            DepGraphCommand::View(args) => view::run(args),
        }
    }
}
