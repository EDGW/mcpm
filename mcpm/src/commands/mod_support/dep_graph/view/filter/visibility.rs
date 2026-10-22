//! Rule matching and ancestor retention over eligible edges, including cyclic graphs.

use std::collections::{HashMap, HashSet, VecDeque};

use kako_mods_lib::local::mcmod::dependency::{
    Dependency,
    graph::{DependencyGraph, candidate::DependencyCandidate, identifier::DependencyIdentifier},
};

use super::super::matching;
use super::ViewFilter;

#[derive(Clone, PartialEq, Eq, Hash)]
enum Vertex {
    Node(DependencyIdentifier),
    Candidate(usize),
}

#[derive(Debug)]
pub(in crate::commands::mod_support::dep_graph::view) struct Visibility {
    filtered: bool,
    edges: HashSet<usize>,
    own: HashSet<usize>,
    candidates: HashSet<usize>,
}

impl Visibility {
    pub fn build(graph: &DependencyGraph, filter: &ViewFilter, reverse: bool) -> Self {
        let mut predecessors: HashMap<Vertex, Vec<Vertex>> = HashMap::new();
        let mut relevant = HashSet::new();
        let mut queue = VecDeque::new();
        let mut edges = Vec::new();
        for node in graph.nodes() {
            for candidate in node.candidates() {
                let source = candidate.identifier();
                let candidate_vertex = Vertex::Candidate(candidate_key(candidate));
                if !reverse {
                    predecessors
                        .entry(candidate_vertex.clone())
                        .or_default()
                        .push(Vertex::Node(source.clone()));
                }
                for (target, dependency) in candidate.outgoing() {
                    let neighbor = if reverse { &source } else { target };
                    // Ignore and disabled types cut traversal before applying match rules.
                    if !filter.includes(neighbor, dependency) {
                        continue;
                    }
                    let (from, to) = if reverse {
                        (Vertex::Node(target.clone()), Vertex::Node(source.clone()))
                    } else {
                        (candidate_vertex.clone(), Vertex::Node(target.clone()))
                    };
                    predecessors
                        .entry(to.clone())
                        .or_default()
                        .push(from.clone());
                    let matches = filter.unrestricted(dependency.kind)
                        || filter
                            .accepts(dependency.kind, matching::target(graph, target, dependency));
                    if matches && relevant.insert(from.clone()) {
                        queue.push_back(from.clone());
                    }
                    edges.push((dependency_key(dependency), from, to, matches));
                }
            }
        }
        // Backward propagation reaches a fixed point even through cycles; no recursive walk.
        while let Some(vertex) = queue.pop_front() {
            if let Some(parents) = predecessors.get(&vertex) {
                for parent in parents {
                    if relevant.insert(parent.clone()) {
                        queue.push_back(parent.clone());
                    }
                }
            }
        }
        let own = edges
            .iter()
            .filter(|(_, _, _, own)| *own)
            .map(|(key, _, _, _)| *key)
            .collect();
        Self {
            filtered: filter.has_rules(),
            own,
            edges: edges
                .into_iter()
                .filter(|(_, _, to, own)| *own || relevant.contains(to))
                .map(|(key, _, _, _)| key)
                .collect(),
            candidates: relevant
                .into_iter()
                .filter_map(|v| match v {
                    Vertex::Candidate(key) => Some(key),
                    Vertex::Node(_) => None,
                })
                .collect(),
        }
    }

    pub fn filtered(&self) -> bool {
        self.filtered
    }

    pub fn edge(&self, dependency: &Dependency) -> bool {
        self.edges.contains(&dependency_key(dependency))
    }

    pub fn own(&self, dependency: &Dependency) -> bool {
        self.own.contains(&dependency_key(dependency))
    }

    pub fn candidate(&self, candidate: &DependencyCandidate) -> bool {
        self.candidates.contains(&candidate_key(candidate))
    }
}

fn dependency_key(dependency: &Dependency) -> usize {
    dependency as *const Dependency as usize
}

fn candidate_key(candidate: &DependencyCandidate) -> usize {
    candidate as *const DependencyCandidate as usize
}
