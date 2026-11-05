//! Displays the Host graph's problem trees and distinguishes incomplete identification.

use super::graph_input::ServerArgs;
use crate::{
    commands::{execution::Runtime, supports::mods::dep_graph},
    display::style::OutputStyle,
    ssh::SshArgs,
};
use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::operations::mods::dep_graph::BuildInput;
use std::io::{self, Write};
#[derive(Args, Debug)]
pub(super) struct CheckArgs {
    #[command(flatten)]
    server: ServerArgs,
}
pub(super) fn run(args: CheckArgs, ssh: SshArgs) -> Result<()> {
    Runtime::new()?.block_on(async move {
        let (executor, scope) = args.server.resolve(ssh).await?;
        let graph = dep_graph::build_graph(
            &executor,
            BuildInput {
                scope,
                external: Vec::new(),
            },
        )
        .await?;
        let mut output = io::stdout().lock();
        if dep_graph::write_problems(&graph, &mut output)? == 0 {
            let style = OutputStyle::new();
            let result = if graph.identification_failures == 0 {
                style.success("Check passed. No problems found.")
            } else {
                style.warning("Check incomplete. Some mod files could not be identified.")
            };
            writeln!(output, "{result}").context("failed to write dependency check result")?;
        }
        output
            .flush()
            .context("failed to flush dependency check output")
    })
}
