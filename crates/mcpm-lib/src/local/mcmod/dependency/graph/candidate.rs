//! Dependency graph candidate identities and declarations.

use std::sync::Arc;

use crate::local::{
    mcmod::{ModInfo, ModMeta, dependency::Dependency},
    modloader::ModLoaderMeta,
};

use super::{
    edge::{self, NestedEdge, PrimaryEdge},
    identifier::DependencyIdentifier,
};

/// A supplied external component. It has no outgoing edges.
#[derive(Debug)]
pub struct ExternalCandidate {
    pub identifier: DependencyIdentifier,
    /// Version interpreted by this component's loader; None bypasses version checking.
    pub version: Option<String>,
}

#[derive(Debug)]
pub struct PrimaryModCandidate {
    pub loader: ModLoaderMeta,
    pub modinfo: Arc<ModInfo>,
    pub(super) outgoing: Vec<PrimaryEdge>,
}

impl PrimaryModCandidate {
    pub fn identifier(&self) -> DependencyIdentifier {
        DependencyIdentifier::new(&self.loader, self.modinfo.modid.clone())
    }

    /// Returns declared edges; their collection order has no semantic meaning.
    pub fn outgoing(&self) -> &[PrimaryEdge] {
        &self.outgoing
    }
}

#[derive(Debug)]
pub struct NestedModCandidate {
    pub loader: ModLoaderMeta,
    pub modinfo: Arc<ModInfo>,
    pub(super) outgoing: Vec<NestedEdge>,
}

impl NestedModCandidate {
    pub fn identifier(&self) -> DependencyIdentifier {
        DependencyIdentifier::new(&self.loader, self.modinfo.modid.clone())
    }

    /// Returns declared edges; their collection order has no semantic meaning.
    pub fn outgoing(&self) -> &[NestedEdge] {
        &self.outgoing
    }
}

#[derive(Debug)]
pub enum DependencyCandidate {
    External(ExternalCandidate),
    PrimaryMod(PrimaryModCandidate),
    NestedMod(NestedModCandidate),
}

impl DependencyCandidate {
    /// Iterates all declared outgoing relations without imposing an order.
    pub fn outgoing(&self) -> impl Iterator<Item = (&DependencyIdentifier, &Dependency)> {
        let (primary, nested): (&[PrimaryEdge], &[NestedEdge]) = match self {
            Self::External(_) => (&[], &[]),
            Self::PrimaryMod(candidate) => (candidate.outgoing(), &[]),
            Self::NestedMod(candidate) => (&[], candidate.outgoing()),
        };
        primary
            .iter()
            .map(|edge| (&edge.target, edge.dependency.as_ref()))
            .chain(
                nested
                    .iter()
                    .map(|edge| (&edge.target, edge.dependency.as_ref())),
            )
    }

    pub fn identifier(&self) -> DependencyIdentifier {
        match self {
            Self::External(candidate) => candidate.identifier.clone(),
            Self::PrimaryMod(candidate) => candidate.identifier(),
            Self::NestedMod(candidate) => candidate.identifier(),
        }
    }
}

pub(super) fn to_candidates(modmeta: &ModMeta, is_nested: bool) -> Vec<DependencyCandidate> {
    let mut candidates: Vec<_> = modmeta
        .mods
        .iter()
        .map(|modinfo| {
            let modinfo = Arc::new(modinfo.clone());
            let edges = edge::declarations(&modinfo.dependencies, &modmeta.loader);
            if is_nested {
                DependencyCandidate::NestedMod(NestedModCandidate {
                    loader: modmeta.loader.clone(),
                    modinfo,
                    outgoing: edges
                        .into_iter()
                        .map(|(target, dependency)| NestedEdge { target, dependency })
                        .collect(),
                })
            } else {
                DependencyCandidate::PrimaryMod(PrimaryModCandidate {
                    loader: modmeta.loader.clone(),
                    modinfo,
                    outgoing: edges
                        .into_iter()
                        .map(|(target, dependency)| PrimaryEdge { target, dependency })
                        .collect(),
                })
            }
        })
        .collect();

    for nested in &modmeta.nested {
        candidates.extend(to_candidates(&nested.meta, true));
    }
    candidates
}
