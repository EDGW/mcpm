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
    on_error: impl FnMut(&anyhow::Error),
) -> Result<DependencyGraph> {
    build_with_progress(directory, loaders, on_error, |_, _| {})
}

/// Builds a graph with file-level progress. Reports `(0, total)` after enumeration,
/// then `(completed, total)` after each file, including unrecognized files.
/// Callbacks carry no terminal or presentation policy.
pub fn build_with_progress(
    directory: &ModDirectory,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
    mut on_error: impl FnMut(&anyhow::Error),
    mut on_progress: impl FnMut(usize, usize),
) -> Result<DependencyGraph> {
    let mut graph = DependencyGraph::new();
    let entries = directory.entries()?;
    let total = entries.len();
    on_progress(0, total);
    for (index, entry) in entries.into_iter().enumerate() {
        let path = entry.path().to_owned();
        let mut file = mcmod::open(
            entry,
            loaders.clone(),
            &crate::local::runtime::RuntimeEnvironment::new().with_mods_cache(None),
            |entry, loader, _, error| {
                on_error(error);
                graph.identification_failures.push(IdentificationFailure {
                    path: entry.path().to_owned(),
                    loader: loader.id().as_str().to_owned(),
                    error: crate::error::ErrorReport::capture(error),
                });
                IdentificationErrorAction::Skip
            },
        )
        .with_context(|| format!("failed to build dependency graph from {path:?}"))?
        .file;
        file.metadata
            .sort_by(|left, right| left.loader.id().cmp(right.loader.id()));
        for metadata in &file.metadata {
            graph.add_modfile(metadata);
        }
        on_progress(index + 1, total);
    }
    graph.identification_failures.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.loader.cmp(&right.loader))
    });
    Ok(graph)
}
