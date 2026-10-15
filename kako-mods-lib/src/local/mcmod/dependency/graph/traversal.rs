use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use super::{
    super::Dependency, DependencyGraph, candidate::DependencyCandidate,
    identifier::DependencyIdentifier,
};

impl DependencyGraph {
    /// Returns the shortest declaration-reference distance from `root` to each
    /// reachable node, across all candidates and relation kinds. Candidate choice,
    /// versions, physical side and loading order are not evaluated.
    /// Returns None if the root is absent. Cycles are supported.
    pub fn dependency_depths(
        &self,
        root: &DependencyIdentifier,
    ) -> Option<HashMap<DependencyIdentifier, usize>> {
        self.dependency_depths_with(root, |_, _| true)
    }

    /// Like dependency_depths, but traverses only references accepted by include.
    /// Filters prune the edge and traversal through it; other paths may still
    /// reach the same node. The root is always included if it exists.
    pub fn dependency_depths_with(
        &self,
        root: &DependencyIdentifier,
        mut include: impl FnMut(&DependencyIdentifier, &Dependency) -> bool,
    ) -> Option<HashMap<DependencyIdentifier, usize>> {
        self.node(root)?;
        let mut depths = HashMap::from([(root.clone(), 0)]);
        let mut pending = VecDeque::from([root.clone()]);
        while let Some(identifier) = pending.pop_front() {
            let depth = depths[&identifier];
            let Some(node) = self.node(&identifier) else {
                continue;
            };
            for candidate in node.candidates() {
                for (target, dependency) in candidate.outgoing() {
                    if include(target, dependency) && !depths.contains_key(target) {
                        depths.insert(target.clone(), depth + 1);
                        pending.push_back(target.clone());
                    }
                }
            }
        }
        Some(depths)
    }

    /// Returns incoming declarations as (source node ID, exact source candidate,
    /// dependency). References borrow the graph's owning nodes and candidates,
    /// rather than a temporary upgrade of the incoming weak reference.
    pub fn incoming_relations(
        &self,
        target: &DependencyIdentifier,
    ) -> Vec<(&DependencyIdentifier, &DependencyCandidate, &Dependency)> {
        let Some(node) = self.node(target) else {
            return Vec::new();
        };
        node.incoming()
            .iter()
            .filter_map(|edge| {
                let source = edge.source.upgrade()?;
                let (identifier, source_node) = self.nodes.get_key_value(&source.identifier())?;
                let candidate = source_node
                    .candidates()
                    .iter()
                    .find(|candidate| Arc::ptr_eq(candidate, &source))?;
                Some((identifier, candidate.as_ref(), edge.dependency.as_ref()))
            })
            .collect()
    }
}
