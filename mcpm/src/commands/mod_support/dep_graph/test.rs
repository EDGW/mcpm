//! Validates saved dependency graphs and prints their statistics.

use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mcmod::dependency::graph::{candidate::DependencyCandidate, storage},
    modloader::BUILTIN_LOADERS,
};

#[derive(Args, Debug)]
pub(super) struct TestArgs {
    /// Saved dependency graph JSON file.
    #[arg(value_name = "FILE")]
    file: PathBuf,
}

pub(super) fn run(args: TestArgs) -> Result<()> {
    let file = File::open(&args.file)
        .with_context(|| format!("failed to open dependency graph {:?}", args.file))?;
    let graph = storage::read_json(file, &BUILTIN_LOADERS)
        .with_context(|| format!("failed to restore dependency graph {:?}", args.file))?;
    let mut nodes = 0;
    let mut empty_nodes = 0;
    let mut external = 0;
    let mut primary = 0;
    let mut nested = 0;
    let mut edges = 0;
    for node in graph.nodes() {
        nodes += 1;
        if node.candidates().is_empty() {
            empty_nodes += 1;
        }
        for candidate in node.candidates() {
            match candidate.as_ref() {
                DependencyCandidate::External(_) => external += 1,
                DependencyCandidate::PrimaryMod(candidate) => {
                    primary += 1;
                    edges += candidate.outgoing().len();
                }
                DependencyCandidate::NestedMod(candidate) => {
                    nested += 1;
                    edges += candidate.outgoing().len();
                }
            }
        }
    }
    let mut output = io::stdout().lock();
    writeln!(
        output,
        "Nodes: {nodes}\nCandidates: {} (external: {external}, primary: {primary}, nested: {nested})\nEdges: {edges}\nEmpty nodes: {empty_nodes}\nIdentification failures: {}",
        external + primary + nested,
        graph.identification_failures().len(),
    )
    .context("failed to write dependency graph statistics")?;
    output
        .flush()
        .context("failed to flush dependency graph statistics")?;
    Ok(())
}
