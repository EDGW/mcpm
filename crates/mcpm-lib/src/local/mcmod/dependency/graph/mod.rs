//! Dependency declaration graph and access to its candidates, relations and diagnostics.

use std::{collections::HashMap, sync::Arc};

use crate::local::mcmod::ModMeta;

use self::{
    candidate::{DependencyCandidate, ExternalCandidate, to_candidates},
    diagnostic::IdentificationFailure,
    identifier::DependencyIdentifier,
    node::{DependencyNode, IncomingEdge},
};

pub mod build;
pub mod candidate;
pub mod diagnostic;
pub mod edge;
pub mod identifier;
pub mod node;
pub mod storage;
mod traversal;

/// A declaration graph. Candidate selection and loading-order evaluation are separate.
///
/// # Implementations
/// - [`Self::new`]: graph construction, node access and identification diagnostics.
/// - [`Self::dependency_depths`]: traversal and incoming relations (`traversal.rs`).
#[derive(Debug, Default)]
pub struct DependencyGraph {
    nodes: HashMap<DependencyIdentifier, DependencyNode>,
    identification_failures: Vec<IdentificationFailure>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    fn add_modcandidate(&mut self, candidate: DependencyCandidate) {
        let candidate = Arc::new(candidate);
        let identifier = candidate.identifier();
        let targets: Vec<_> = match candidate.as_ref() {
            DependencyCandidate::External(_) => Vec::new(),
            DependencyCandidate::PrimaryMod(candidate) => candidate
                .outgoing()
                .iter()
                .map(|edge| (edge.target.clone(), Arc::clone(&edge.dependency)))
                .collect(),
            DependencyCandidate::NestedMod(candidate) => candidate
                .outgoing()
                .iter()
                .map(|edge| (edge.target.clone(), Arc::clone(&edge.dependency)))
                .collect(),
        };
        let source = self
            .nodes
            .entry(identifier.clone())
            .or_insert_with(|| DependencyNode::empty(identifier.clone()));
        source.candidates.push(Arc::clone(&candidate));

        for (target, dependency) in targets {
            self.nodes
                .entry(target.clone())
                .or_insert_with(|| DependencyNode::empty(target))
                .incoming
                .push(IncomingEdge {
                    source: Arc::downgrade(&candidate),
                    dependency,
                });
        }
    }

    pub fn add_modfile(&mut self, modmeta: &ModMeta) {
        for candidate in to_candidates(modmeta, false) {
            self.add_modcandidate(candidate);
        }
    }

    /// Adds a known external component. Unresolved targets remain empty nodes.
    pub fn add_external(&mut self, candidate: ExternalCandidate) {
        self.add_modcandidate(DependencyCandidate::External(candidate));
    }

    pub fn node(&self, identifier: &DependencyIdentifier) -> Option<&DependencyNode> {
        self.nodes.get(identifier)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &DependencyNode> {
        self.nodes.values()
    }

    pub fn identification_failures(&self) -> &[IdentificationFailure] {
        &self.identification_failures
    }
}
