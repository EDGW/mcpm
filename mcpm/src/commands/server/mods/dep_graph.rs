//! Adds registered Minecraft and loader versions to the shared dependency graph build pipeline.

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::dep_graph::{self, BuildOptions};

use super::graph_input::ServerArgs;

pub(super) type DepGraphArgs = dep_graph::DepGraphArgs<BuildArgs>;

#[derive(Args)]
pub(super) struct BuildArgs {
    #[command(flatten)]
    server: ServerArgs,
    #[command(flatten)]
    options: BuildOptions,
}

pub(super) fn run(args: DepGraphArgs) -> Result<()> {
    args.run(|args| {
        let (source, external) = args.server.resolve()?;
        args.options
            .run(source.container().directory(), &source.loaders(), external)
    })
}
