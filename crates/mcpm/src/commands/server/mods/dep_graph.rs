//! Builds server graphs using Host-resolved loaders and automatic runtime candidates.

use super::graph_input::ServerArgs;
use crate::{
    commands::{
        execution::Runtime,
        supports::mods::dep_graph::{self, BuildOptions},
    },
    ssh::SshArgs,
};
use anyhow::Result;
use clap::Args;
pub(super) type DepGraphArgs = dep_graph::DepGraphArgs<BuildArgs>;
#[derive(Args, Debug)]
pub(super) struct BuildArgs {
    #[command(flatten)]
    server: ServerArgs,
    #[command(flatten)]
    options: BuildOptions,
}
pub(super) fn run(args: DepGraphArgs, ssh: SshArgs) -> Result<()> {
    Runtime::new()?.block_on(args.run(ssh, |args, ssh| async move {
        let (executor, scope) = args.server.resolve(ssh).await?;
        args.options.run(scope, &executor).await
    }))
}
