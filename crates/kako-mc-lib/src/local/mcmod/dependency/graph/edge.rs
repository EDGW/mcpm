//! Dependency graph edge types.

use std::sync::Arc;

use crate::local::{mcmod::dependency::Dependency, modloader::ModLoaderMeta};

use super::identifier::DependencyIdentifier;

/// A complete declared relation from a primary candidate to a target node.
/// Edges have no positional identity; dependency ordering is only expressed
/// through the declaration's explicit ordering field.
#[derive(Debug)]
pub struct PrimaryEdge {
    pub target: DependencyIdentifier,
    pub dependency: Arc<Dependency>,
}

/// A complete declared relation from a nested candidate to a target node.
#[derive(Debug)]
pub struct NestedEdge {
    pub target: DependencyIdentifier,
    pub dependency: Arc<Dependency>,
}

pub(super) fn declarations(
    dependencies: &[Dependency],
    loader: &ModLoaderMeta,
) -> Vec<(DependencyIdentifier, Arc<Dependency>)> {
    dependencies
        .iter()
        .map(|dependency| {
            (
                DependencyIdentifier::new(loader, dependency.id.clone()),
                Arc::new(dependency.clone()),
            )
        })
        .collect()
}
