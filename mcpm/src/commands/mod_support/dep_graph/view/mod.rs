//! Reads a saved dependency graph once and renders the requested root trees in order.

mod filter;
mod matching;
mod output;

use std::{
    collections::HashMap,
    fs::File,
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mcmod::dependency::graph::{DependencyGraph, identifier::DependencyIdentifier, storage},
    modloader::BUILTIN_LOADERS,
};
use thiserror::Error;

use super::selector::ModSelector;

#[derive(Args)]
pub(super) struct ViewArgs {
    /// Graph JSON input; omit or use - to read stdin.
    #[arg(short, long, value_name = "FILE")]
    input: Option<PathBuf>,
    /// Root nodes as loader:modid; defaults to all nodes with candidates, sorted by loader/modid.
    #[arg(value_name = "LOADER:MODID", num_args = 0..)]
    targets: Vec<ModSelector>,
    /// Show mods declaring relations to the target, following incoming edges.
    #[arg(short = 'R', long)]
    reverse: bool,
    /// Hide roots with no displayed children after applying all filters.
    #[arg(short = 'S', long, visible_alias = "iir")]
    ignore_isolated_root: bool,
    #[command(flatten)]
    filter: filter::FilterArgs,
}

#[derive(Debug, Error)]
enum ViewError {
    #[error("no graph input: specify -i FILE or pipe graph JSON on stdin")]
    MissingInput,
    #[error("dependency graph has no node {identifier:?}")]
    UnknownMod { identifier: DependencyIdentifier },
}

pub(super) fn run(args: ViewArgs) -> Result<()> {
    let graph = if let Some(path) = args.input.as_ref().filter(|path| path.as_os_str() != "-") {
        let input = File::open(path)
            .with_context(|| format!("failed to open dependency graph {path:?}"))?;
        storage::read_json(input, &BUILTIN_LOADERS)
            .with_context(|| format!("failed to restore dependency graph {path:?}"))?
    } else {
        let input = io::stdin();
        if input.is_terminal() {
            return Err(ViewError::MissingInput.into());
        }
        storage::read_json(input.lock(), &BUILTIN_LOADERS)
            .context("failed to read dependency graph from stdin")?
    };
    write_graph(&graph, args, &mut io::stdout().lock())?;
    Ok(())
}

/// Renders exactly the default --problem view without serializing an intermediate snapshot.
pub(in crate::commands) fn write_problems(
    graph: &DependencyGraph,
    output: &mut impl Write,
) -> Result<usize> {
    write_graph(
        graph,
        ViewArgs {
            input: None,
            targets: Vec::new(),
            reverse: false,
            ignore_isolated_root: false,
            filter: filter::FilterArgs::problem(),
        },
        output,
    )
}

fn write_graph(graph: &DependencyGraph, args: ViewArgs, output: &mut impl Write) -> Result<usize> {
    let ignore_isolated_root = args.ignore_isolated_root || args.filter.hides_isolated_roots();
    let filter = args.filter.into_filter();
    let visibility = filter::visibility::Visibility::build(graph, &filter, args.reverse);
    let targets = if args.targets.is_empty() {
        let mut targets = graph
            .nodes()
            .filter(|node| !node.candidates().is_empty())
            .map(|node| node.identifier())
            .collect::<Vec<_>>();
        targets.sort_by(|a, b| (a.loader().as_str(), a.id()).cmp(&(b.loader().as_str(), b.id())));
        targets
    } else {
        args.targets.into_iter().map(|target| target.0).collect()
    };
    let mut views = Vec::new();
    for identifier in targets {
        let depths = if args.reverse {
            graph.node(&identifier).map(|_| HashMap::new())
        } else {
            graph.dependency_depths_with(&identifier, |target, dependency| {
                filter.includes(target, dependency) && visibility.edge(dependency)
            })
        }
        .ok_or_else(|| ViewError::UnknownMod {
            identifier: identifier.clone(),
        })?;
        if ignore_isolated_root
            && !output::has_children(graph, &identifier, &visibility, args.reverse)
        {
            continue;
        }
        views.push((identifier, depths));
    }
    let count = views.len();
    for (index, (identifier, depths)) in views.into_iter().enumerate() {
        if index > 0 {
            writeln!(output).context("failed to separate dependency graph views")?;
        }
        output::write_view(
            output,
            graph,
            &identifier,
            depths,
            &visibility,
            args.reverse,
        )
        .context("failed to write dependency graph view")?;
    }
    output
        .flush()
        .context("failed to flush dependency graph view")?;
    Ok(count)
}
