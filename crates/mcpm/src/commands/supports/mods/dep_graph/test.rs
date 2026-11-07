//! Displays statistics from caller-side saved graph inspection.

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::operations::mods::dep_graph::{
    GraphInput, Inspect, InspectAction, InspectInput, InspectOutput,
};
use std::{
    io::{self, Write},
    path::PathBuf,
};
#[derive(Args, Debug)]
pub(super) struct TestArgs {
    /// Locally saved dependency graph JSON file.
    #[arg(value_name = "FILE")]
    file: PathBuf,
}
pub(super) async fn run(args: TestArgs) -> Result<()> {
    let executor = mcpm_exec::service::Executor::open(mcpm_exec::service::Target::Local).await?;
    let InspectOutput::Test(result) = executor
        .call::<Inspect>(InspectInput {
            graph: GraphInput::File(args.file),
            action: InspectAction::Test,
        })
        .await?
    else {
        anyhow::bail!("unexpected dependency graph inspection result");
    };
    let mut output = io::stdout().lock();
    writeln!(output, "Nodes: {}\nCandidates: {} (external: {}, primary: {}, nested: {})\nEdges: {}\nEmpty nodes: {}\nIdentification failures: {}", result.nodes, result.external + result.primary + result.nested, result.external, result.primary, result.nested, result.edges, result.empty_nodes, result.identification_failures).context("failed to write dependency graph statistics")?;
    output
        .flush()
        .context("failed to flush dependency graph statistics")?;
    Ok(())
}
