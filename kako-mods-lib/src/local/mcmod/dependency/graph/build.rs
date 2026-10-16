//! Builds dependency graphs from directories, retaining skipped identification failures.

use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::local::{
    mcmod::{self, IdentificationErrorAction, ModDirectory},
    modloader::{LoaderId, ModLoaderMeta},
};

use super::DependencyGraph;

// Preserve the original public path for graph diagnostics.
pub use super::diagnostic::IdentificationFailure;

/// Builds a declaration graph from a directory and its declared nested archives.
/// Files not recognized by the supplied loaders are ignored. Identification
/// failures use Skip: successful records survive and later loaders/files are tried.
/// Skipped failures are retained on the graph to expose incomplete identification.
/// The callback receives each original error chain before it is recorded and skipped.
/// No external candidates are inferred from unresolved references.
pub fn build(
    directory: &ModDirectory,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
    mut on_error: impl FnMut(&anyhow::Error),
) -> Result<DependencyGraph> {
    let mut graph = DependencyGraph::new();
    for entry in directory.entries()? {
        let path = entry.path().to_owned();
        let mut file = mcmod::open(entry, loaders.clone(), |entry, loader, _, error| {
            on_error(error);
            graph.identification_failures.push(IdentificationFailure {
                path: entry.path().to_owned(),
                loader: loader.id().as_str().to_owned(),
                error: format!("{error:#}"),
            });
            IdentificationErrorAction::Skip
        })
        .with_context(|| format!("failed to build dependency graph from {path:?}"))?;
        file.metadata
            .sort_by(|left, right| left.loader.id().cmp(right.loader.id()));
        for metadata in &file.metadata {
            graph.add_modfile(metadata);
        }
    }
    graph.identification_failures.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.loader.cmp(&right.loader))
    });
    Ok(graph)
}
