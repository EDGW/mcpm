//! Builds filtered forward/reverse tree models without terminal formatting.

pub mod filter;
mod matching;
mod sort_key;
mod tree;
use super::{ViewInput, ViewOutput};
use anyhow::Result;
use mcpm_lib::local::mcmod::dependency::graph::{
    DependencyGraph, identifier::DependencyIdentifier,
};
use std::collections::HashMap;

pub(super) fn build(graph: &DependencyGraph, args: ViewInput) -> Result<ViewOutput> {
    let ignore_isolated_root = args.ignore_isolated_root || args.filter.problem;
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
        args.targets
            .into_iter()
            .map(|target| target.into_runtime())
            .collect()
    };
    let mut trees = Vec::new();
    for identifier in targets {
        let depths = if args.reverse {
            graph.node(&identifier).map(|_| HashMap::new())
        } else {
            graph.dependency_depths_with(&identifier, |target, dependency| {
                filter.includes(target, dependency) && visibility.edge(dependency)
            })
        }
        .ok_or_else(|| UnknownMod {
            identifier: identifier.clone(),
        })?;
        if ignore_isolated_root
            && !tree::has_children(graph, &identifier, &visibility, args.reverse)
        {
            continue;
        }
        trees.push(tree::build(
            graph,
            &identifier,
            depths,
            &visibility,
            args.reverse,
        ));
    }
    Ok(ViewOutput {
        trees,
        identification_failures: graph.identification_failures().len(),
        reverse: args.reverse,
    })
}
#[derive(Debug, thiserror::Error)]
#[error("dependency graph has no node {identifier:?}")]
struct UnknownMod {
    identifier: DependencyIdentifier,
}
