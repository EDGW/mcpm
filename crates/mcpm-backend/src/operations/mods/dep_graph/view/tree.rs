//! Iterative dependency tree expansion, folding shared nodes and cycles before display.

use super::super::*;
use super::{filter::visibility::Visibility, matching, sort_key};
use mcpm_lib::local::mcmod::dependency::{
    Dependency, DependencyKind,
    graph::{DependencyGraph, candidate::DependencyCandidate, identifier::DependencyIdentifier},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
enum Entry<'a> {
    Node {
        identifier: &'a DependencyIdentifier,
        dependency: Option<&'a Dependency>,
        source_candidate: Option<&'a DependencyCandidate>,
        target: Option<&'a DependencyIdentifier>,
        depth: usize,
    },
    Candidate {
        dependency: Option<&'a Dependency>,
        candidate: &'a DependencyCandidate,
        depth: usize,
    },
}
enum Task<'a> {
    Visit {
        entry: Entry<'a>,
        parent: Option<usize>,
        last: bool,
    },
    Leave(&'a DependencyIdentifier),
}
fn root_entry(root: &DependencyIdentifier) -> Entry<'_> {
    Entry::Node {
        identifier: root,
        dependency: None,
        source_candidate: None,
        target: None,
        depth: 0,
    }
}
pub(super) fn has_children(
    graph: &DependencyGraph,
    root: &DependencyIdentifier,
    visibility: &Visibility,
    reverse: bool,
) -> bool {
    !children(graph, root_entry(root), visibility, reverse).is_empty()
}
pub(super) fn build(
    graph: &DependencyGraph,
    root: &DependencyIdentifier,
    depths: HashMap<DependencyIdentifier, usize>,
    visibility: &Visibility,
    reverse: bool,
) -> Vec<TreeEntry> {
    let mut result = Vec::new();
    let mut expanded = HashSet::new();
    let mut active = HashSet::new();
    let mut pending = vec![Task::Visit {
        entry: root_entry(root),
        parent: None,
        last: true,
    }];
    while let Some(task) = pending.pop() {
        let Task::Visit {
            entry,
            parent,
            last,
        } = task
        else {
            if let Task::Leave(identifier) = task {
                active.remove(identifier);
            }
            continue;
        };
        let mut folded = None;
        if let Entry::Node {
            identifier, depth, ..
        } = entry
        {
            if active.contains(identifier) {
                folded = Some(Fold::Cycle);
            } else if !reverse
                && (depths[identifier] < depth || !expanded.insert(identifier.clone()))
            {
                folded = Some(Fold::Shared);
            }
        }
        let children = if folded.is_some() {
            Vec::new()
        } else {
            children(graph, entry, visibility, reverse)
        };
        let index = result.len();
        result.push(TreeEntry {
            parent,
            last,
            label: label(graph, entry, folded),
        });
        if children.is_empty() {
            continue;
        }
        if let Entry::Node { identifier, .. } = entry {
            active.insert(identifier.clone());
            pending.push(Task::Leave(identifier));
        }
        let count = children.len();
        for (index_child, child) in children.into_iter().enumerate().rev() {
            pending.push(Task::Visit {
                entry: child,
                parent: Some(index),
                last: index_child + 1 == count,
            });
        }
    }
    result
}
fn candidate_view(candidate: &DependencyCandidate) -> CandidateView {
    match candidate {
        DependencyCandidate::External(candidate) => CandidateView::External {
            version: candidate.version.clone(),
        },
        DependencyCandidate::PrimaryMod(candidate) => CandidateView::Primary {
            version: candidate.modinfo.version.clone(),
        },
        DependencyCandidate::NestedMod(candidate) => CandidateView::Nested {
            version: candidate.modinfo.version.clone(),
        },
    }
}
fn label(graph: &DependencyGraph, entry: Entry<'_>, folded: Option<Fold>) -> TreeLabel {
    match entry {
        Entry::Candidate {
            candidate,
            dependency,
            ..
        } => TreeLabel::Candidate {
            candidate: candidate_view(candidate),
            relation: dependency.cloned(),
            state: dependency.map(|dependency| {
                matching::candidate_match(candidate, candidate.identifier().loader(), dependency)
                    .into()
            }),
        },
        Entry::Node {
            identifier,
            dependency,
            source_candidate,
            target,
            depth,
        } => {
            let node = graph
                .node(identifier)
                .expect("validated target node exists");
            let candidates = if let Some(candidate) = source_candidate {
                CandidateSummary::One(candidate_view(candidate))
            } else {
                match node.candidates() {
                    [] => CandidateSummary::Missing,
                    [candidate] => CandidateSummary::One(candidate_view(candidate)),
                    candidates => CandidateSummary::Multiple(candidates.len()),
                }
            };
            TreeLabel::Node {
                identifier: identifier.into(),
                root: depth == 0,
                candidates,
                relation: dependency.cloned(),
                state: dependency.map(|dependency| {
                    matching::target(graph, target.unwrap_or(identifier), dependency).into()
                }),
                folded,
            }
        }
    }
}
fn children<'a>(
    graph: &'a DependencyGraph,
    entry: Entry<'a>,
    visibility: &Visibility,
    reverse: bool,
) -> Vec<Entry<'a>> {
    match entry {
        Entry::Candidate {
            candidate, depth, ..
        } => edges(candidate, depth, visibility),
        Entry::Node {
            identifier,
            depth,
            dependency,
            ..
        } => {
            if reverse {
                return reverse_edges(graph, identifier, depth, visibility);
            }
            let candidates = graph
                .node(identifier)
                .expect("validated target node exists")
                .candidates();
            if let [candidate] = candidates {
                return edges(candidate, depth, visibility);
            }
            let mut candidates = candidates.iter().map(|c| c.as_ref()).collect::<Vec<_>>();
            if dependency.map_or_else(
                || visibility.filtered(),
                |dependency| !visibility.own(dependency),
            ) {
                candidates.retain(|candidate| visibility.candidate(candidate));
            }
            candidates.sort_by_key(|candidate| sort_key::candidate(candidate));
            candidates
                .into_iter()
                .map(|candidate| Entry::Candidate {
                    candidate,
                    depth,
                    dependency,
                })
                .collect()
        }
    }
}
fn edges<'a>(
    candidate: &'a DependencyCandidate,
    depth: usize,
    visibility: &Visibility,
) -> Vec<Entry<'a>> {
    let mut edges = candidate
        .outgoing()
        .filter(|(_, dependency)| visibility.edge(dependency))
        .collect::<Vec<_>>();
    edges.sort_by_key(|(target, dependency)| {
        (
            relation_order(dependency.kind),
            target.id(),
            sort_key::details(dependency),
        )
    });
    edges
        .into_iter()
        .map(|(identifier, dependency)| Entry::Node {
            identifier,
            dependency: Some(dependency),
            source_candidate: None,
            target: None,
            depth: depth + 1,
        })
        .collect()
}
fn reverse_edges<'a>(
    graph: &'a DependencyGraph,
    identifier: &'a DependencyIdentifier,
    depth: usize,
    visibility: &Visibility,
) -> Vec<Entry<'a>> {
    let mut edges = graph.incoming_relations(identifier);
    edges.retain(|(_, _, dependency)| visibility.edge(dependency));
    edges.sort_by_key(|(source, candidate, dependency)| {
        (
            relation_order(dependency.kind),
            source.id(),
            sort_key::candidate(candidate),
            sort_key::details(dependency),
        )
    });
    edges
        .into_iter()
        .map(|(source, candidate, dependency)| Entry::Node {
            identifier: source,
            dependency: Some(dependency),
            source_candidate: Some(candidate),
            target: Some(identifier),
            depth: depth + 1,
        })
        .collect()
}
fn relation_order(kind: DependencyKind) -> usize {
    match kind {
        DependencyKind::Required => 0,
        DependencyKind::Optional => 1,
        DependencyKind::Recommended => 2,
        DependencyKind::Suggested => 3,
        DependencyKind::Breaks => 4,
        DependencyKind::Conflicts => 5,
    }
}
