//! Dependency graph argument dispatch and shared Executor flows.

use crate::ssh::SshArgs;
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
    /// Build a dependency graph on the target and export JSON locally.
    Build(B),
    /// Validate a locally saved graph, restore loaders, and show graph statistics.
    Test(test::TestArgs),
    /// Show a locally saved mod declaration graph as a colored dependency tree.
    View(view::ViewArgs),
}
impl<B: Args> DepGraphArgs<B> {
    pub(in crate::commands) async fn run<F: Future<Output = Result<()>>>(
        self,
        ssh: SshArgs,
        build: impl FnOnce(B, SshArgs) -> F,
    ) -> Result<()> {
        match self.command {
            DepGraphCommand::Build(args) => build(args, ssh).await,
            DepGraphCommand::Test(args) => test::run(args).await,
            DepGraphCommand::View(args) => view::run(args).await,
        }
    }
}
