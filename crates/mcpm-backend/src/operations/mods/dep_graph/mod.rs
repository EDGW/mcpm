//! Host graph construction and caller-side inspection using the existing graph codec.

mod model;
pub mod view;
use crate::operations::stream::blocking_stream;
use anyhow::{Context, Result};
use mcpm_exec::operation::{Operation, StreamingOutput};
use mcpm_lib::local::{
    mcmod::{
        ModDirectory,
        dependency::graph::{
            self,
            candidate::{DependencyCandidate, ExternalCandidate},
            storage,
        },
    },
    modloader::BUILTIN_LOADERS,
};
pub use model::*;
use std::{collections::HashSet, fs::File, io::Write};
pub use view::filter::{FilterInput, IgnoreSelector, Rule};

pub struct Build;
impl Operation for Build {
    const ID: &'static str = "mod.dep_graph.build";
    type Input = BuildInput;
    type Output = StreamingOutput<BuildEvent>;
    async fn run(input: BuildInput) -> Result<Self::Output> {
        Ok(blocking_stream(move |sender| {
            let resolved = input.scope.resolve()?;
            let directory = ModDirectory::new(resolved.directory)
                .context("failed to open dependency graph input directory")?;
            let send_error = std::cell::RefCell::new(None);
            let mut graph = graph::build::build_with_progress(
                &directory,
                &resolved.loaders,
                |error| {
                    if let Err(error) = sender.send(BuildEvent::Diagnostic(
                        mcpm_exec::error::ErrorReport::capture(error)
                            .into_error()
                            .into(),
                    )) {
                        *send_error.borrow_mut() = Some(error);
                    }
                },
                |completed, total| {
                    if let Some(error) = send_error.borrow_mut().take() {
                        return Err(error);
                    }
                    sender.send(BuildEvent::Progress { completed, total })
                },
                || sender.checkpoint(),
            )?;
            sender.checkpoint()?;
            let mut added = HashSet::new();
            for candidate in resolved
                .external
                .into_iter()
                .chain(
                    input
                        .external
                        .into_iter()
                        .map(|candidate| ExternalCandidate {
                            identifier: candidate.identifier.into_runtime(),
                            version: candidate.version,
                        }),
                )
            {
                sender.checkpoint()?;
                if added.insert((candidate.identifier.clone(), candidate.version.clone())) {
                    graph.add_external(candidate);
                }
            }
            let identification_failures = graph.identification_failures().len();
            let problems = view::build(
                &graph,
                ViewInput {
                    filter: FilterInput {
                        problem: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )?;
            let mut snapshot = Vec::new();
            storage::write_json(&graph, &mut snapshot)?;
            sender.send(BuildEvent::Finished(BuildOutput {
                snapshot,
                problems,
                identification_failures,
            }))
        }))
    }
}

pub struct Inspect;
impl Operation for Inspect {
    const ID: &'static str = "mod.dep_graph.inspect";
    const SHALLOW: bool = true;
    type Input = InspectInput;
    type Output = InspectOutput;
    async fn run(input: InspectInput) -> Result<Self::Output> {
        tokio::task::spawn_blocking(move || {
            let graph = match input.graph {
                GraphInput::File(path) => {
                    let file = File::open(&path)
                        .with_context(|| format!("failed to open dependency graph {path:?}"))?;
                    storage::read_json(file, &BUILTIN_LOADERS)
                        .with_context(|| format!("failed to restore dependency graph {path:?}"))?
                }
                GraphInput::Bytes(bytes) => storage::read_json(bytes.as_slice(), &BUILTIN_LOADERS)
                    .context("failed to read dependency graph from stdin")?,
            };
            match input.action {
                InspectAction::View(args) => Ok(InspectOutput::View(view::build(&graph, args)?)),
                InspectAction::Test => {
                    let mut result = Statistics {
                        identification_failures: graph.identification_failures().len(),
                        ..Default::default()
                    };
                    for node in graph.nodes() {
                        result.nodes += 1;
                        if node.candidates().is_empty() {
                            result.empty_nodes += 1;
                        }
                        for candidate in node.candidates() {
                            match candidate.as_ref() {
                                DependencyCandidate::External(_) => result.external += 1,
                                DependencyCandidate::PrimaryMod(candidate) => {
                                    result.primary += 1;
                                    result.edges += candidate.outgoing().len();
                                }
                                DependencyCandidate::NestedMod(candidate) => {
                                    result.nested += 1;
                                    result.edges += candidate.outgoing().len();
                                }
                            }
                        }
                    }
                    Ok(InspectOutput::Test(result))
                }
            }
        })
        .await
        .context("dependency graph inspection worker failed")?
    }
}

/// Writes the snapshot produced by the original storage codec on the caller.
pub fn write_export(snapshot: &[u8], mut output: impl Write) -> Result<()> {
    output
        .write_all(snapshot)
        .context("failed to write dependency graph JSON")
}
