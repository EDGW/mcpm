//! Builds graph snapshots and registers optionally versioned external components.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::local::{
    mcmod::{
        ModDirectory,
        dependency::graph::{DependencyGraph, build, candidate::ExternalCandidate, storage},
    },
    modloader::{LoaderId, ModLoaderMeta},
};

use super::selector::ExternalSelector;
use crate::progress::Progress;

#[derive(Args, Debug)]
pub(in crate::commands) struct BuildOptions {
    /// Write JSON to this file instead of stdout; overwrite an existing file.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Add an external candidate as loader:modid[:version]; repeat to add more.
    #[arg(long, value_name = "LOADER:MODID[:VERSION]")]
    external: Vec<ExternalSelector>,
}

impl BuildOptions {
    pub(in crate::commands) fn run(
        self,
        directory: &ModDirectory,
        loaders: &HashMap<LoaderId, ModLoaderMeta>,
        external: impl IntoIterator<Item = ExternalCandidate>,
    ) -> Result<()> {
        let external = external
            .into_iter()
            .chain(
                self.external
                    .into_iter()
                    .map(|candidate| ExternalCandidate {
                        identifier: candidate.identifier,
                        version: candidate.version,
                    }),
            );
        let graph = build_graph(directory, loaders, external)?;

        if let Some(path) = self.output {
            let file = File::create(&path)
                .with_context(|| format!("failed to create dependency graph output {path:?}"))?;
            let mut output = BufWriter::new(file);
            storage::write_json(&graph, &mut output)
                .with_context(|| format!("failed to write dependency graph to {path:?}"))?;
            output
                .flush()
                .with_context(|| format!("failed to flush dependency graph output {path:?}"))?;
        } else {
            let mut output = io::stdout().lock();
            storage::write_json(&graph, &mut output)
                .context("failed to write dependency graph to stdout")?;
            output
                .flush()
                .context("failed to flush dependency graph stdout")?;
        }
        Ok(())
    }
}

/// Builds a graph and deduplicates automatic and manually supplied externals together.
pub(in crate::commands) fn build_graph(
    directory: &ModDirectory,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
    external: impl IntoIterator<Item = ExternalCandidate>,
) -> Result<DependencyGraph> {
    let progress = Progress::new("Scanning mod directory...", "Identifying mods");
    let mut graph = build::build_with_progress(
        directory,
        loaders,
        |error| progress.report(error),
        |completed, total| {
            if completed == 0 {
                progress.set_total(total);
            }
            progress.set_position(completed);
        },
    )?;
    drop(progress);
    let mut added = HashSet::new();
    for candidate in external {
        let ExternalCandidate {
            identifier,
            version,
        } = &candidate;
        if added.insert((identifier.clone(), version.clone())) {
            graph.add_external(candidate);
        }
    }

    Ok(graph)
}
