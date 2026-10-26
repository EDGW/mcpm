//! Dependency graph nodes and incoming edges.

use std::sync::{Arc, Weak};

use crate::local::mcmod::dependency::Dependency;

use super::{candidate::DependencyCandidate, identifier::DependencyIdentifier};

#[derive(Debug)]
pub struct DependencyNode {
    identifier: DependencyIdentifier,
    pub(super) candidates: Vec<Arc<DependencyCandidate>>,
    pub(super) incoming: Vec<IncomingEdge>,
}

impl DependencyNode {
    pub(super) fn empty(identifier: DependencyIdentifier) -> Self {
        Self {
            identifier,
            candidates: Vec::new(),
            incoming: Vec::new(),
        }
    }

    pub fn identifier(&self) -> DependencyIdentifier {
        self.identifier.clone()
    }

    pub fn candidates(&self) -> &[Arc<DependencyCandidate>] {
        &self.candidates
    }

    pub fn incoming(&self) -> &[IncomingEdge] {
        &self.incoming
    }
}

/// Reverse relation to a specific source candidate and its dependency declaration.
/// Weak references avoid owning candidates through incoming relations.
#[derive(Debug, Clone)]
pub struct IncomingEdge {
    pub source: Weak<DependencyCandidate>,
    pub dependency: Arc<Dependency>,
}
