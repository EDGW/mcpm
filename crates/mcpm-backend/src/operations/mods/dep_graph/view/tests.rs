//! Regression coverage for node-level duplicate problems and relation-filter composition.

use super::build;
use crate::operations::mods::dep_graph::{
    CandidateView, FilterInput, Identifier, TreeEntry, TreeLabel, ViewInput, ViewOutput,
};
use mcpm_lib::local::{
    mcmod::{
        ModInfo, ModMeta, NestedModMeta, VersionConstraint,
        dependency::{
            Dependency, DependencyKind,
            graph::{
                DependencyGraph, candidate::ExternalCandidate, identifier::DependencyIdentifier,
                storage,
            },
        },
    },
    modloader::{BUILTIN_LOADERS, LoaderId},
};

fn metadata(loader: &str, id: &str, dependencies: &[&str]) -> ModMeta {
    ModMeta {
        loader: BUILTIN_LOADERS.get(loader).unwrap().clone(),
        mods: vec![ModInfo {
            name: id.into(),
            modid: id.into(),
            version: "1.0.0".into(),
            dependencies: dependencies
                .iter()
                .map(|id| Dependency {
                    kind: DependencyKind::Required,
                    id: (*id).into(),
                    version_requirement: VersionConstraint::Any,
                    side: None,
                    ordering: None,
                    reason: None,
                })
                .collect(),
        }],
        nested: Vec::new(),
    }
}

fn inspect(graph: &DependencyGraph, filter: FilterInput, reverse: bool) -> ViewOutput {
    build(
        graph,
        ViewInput {
            filter,
            reverse,
            ..Default::default()
        },
    )
    .unwrap()
}

fn roots(view: &ViewOutput) -> Vec<&str> {
    view.trees
        .iter()
        .map(|tree| match &tree[0].label {
            TreeLabel::Node { identifier, .. } => identifier.id.as_str(),
            _ => panic!("tree root must be a node"),
        })
        .collect()
}

fn root<'a>(view: &'a ViewOutput, id: &str) -> &'a [TreeEntry] {
    view.trees
        .iter()
        .find(|tree| {
            matches!(&tree[0].label,
                TreeLabel::Node { identifier, .. } if identifier.id == id
            )
        })
        .unwrap()
}

fn duplicate_graph() -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    let mut first = metadata("fabric", "duplicate", &["healthy"]);
    first.nested.push(NestedModMeta {
        path: "child.jar".into(),
        meta: metadata("fabric", "duplicate", &[]),
    });
    graph.add_modfile(&first);
    // Same-version Primary candidates are still distinct conflicting declarations.
    graph.add_modfile(&metadata("fabric", "duplicate", &[]));
    graph.add_modfile(&metadata("fabric", "parent", &["duplicate"]));
    graph.add_modfile(&metadata("fabric", "healthy", &[]));
    graph.add_modfile(&metadata("fabric", "broken", &["absent"]));
    graph
}

#[test]
fn only_multiple_primary_candidates_in_one_loader_identity_are_duplicates() {
    let mut graph = DependencyGraph::new();
    let mut primary = metadata("fabric", "mixed", &[]);
    for index in 0..2 {
        primary.nested.push(NestedModMeta {
            path: format!("child-{index}.jar"),
            meta: metadata("fabric", "mixed", &[]),
        });
    }
    graph.add_modfile(&primary);
    graph.add_modfile(&metadata("forge", "mixed", &[]));
    let identifier = DependencyIdentifier::from_parts(LoaderId::new("fabric"), "mixed".into());
    graph.add_external(ExternalCandidate {
        identifier: identifier.clone(),
        version: None,
    });
    graph.add_external(ExternalCandidate {
        identifier: identifier.clone(),
        version: Some("2".into()),
    });
    assert_eq!(
        graph.node(&identifier).unwrap().duplicate_primary_count(),
        None
    );
    assert!(
        inspect(
            &graph,
            FilterInput {
                problem: true,
                ..Default::default()
            },
            false
        )
        .trees
        .is_empty()
    );
    graph.add_modfile(&metadata("fabric", "mixed", &[]));
    assert_eq!(
        graph.node(&identifier).unwrap().duplicate_primary_count(),
        Some(2)
    );
    let view = inspect(
        &graph,
        FilterInput {
            problem: true,
            ..Default::default()
        },
        false,
    );
    assert_eq!(view.trees.len(), 1);
    assert!(matches!(&view.trees[0][0].label,
        TreeLabel::Node { identifier, duplicate_primary: Some(2), .. } if identifier.loader == "fabric"
    ));
    assert_eq!(
        view.trees[0]
            .iter()
            .filter(|entry| matches!(
                entry.label,
                TreeLabel::Candidate {
                    candidate: CandidateView::Primary { .. },
                    ..
                }
            ))
            .count(),
        2
    );
}

#[test]
fn duplicate_selection_retains_paths_and_all_primary_evidence_without_healthy_edges() {
    let graph = duplicate_graph();
    let only = inspect(
        &graph,
        FilterInput {
            duplicate_primary: Some(true),
            ..Default::default()
        },
        false,
    );
    assert_eq!(roots(&only), ["duplicate", "parent"]);
    let tree = root(&only, "parent");
    assert!(tree.iter().any(|entry| matches!(&entry.label,
        TreeLabel::Node { identifier, duplicate_primary: Some(2), .. } if identifier.id == "duplicate"
    )));
    assert_eq!(
        tree.iter()
            .filter(|entry| matches!(
                entry.label,
                TreeLabel::Candidate {
                    candidate: CandidateView::Primary { .. },
                    ..
                }
            ))
            .count(),
        2
    );
    assert!(!tree.iter().any(|entry| matches!(
        entry.label,
        TreeLabel::Candidate {
            candidate: CandidateView::Nested { .. },
            ..
        }
    )));
    let union = inspect(
        &graph,
        FilterInput {
            duplicate_primary: Some(true),
            rules: vec!["required:unmatched".parse().unwrap()],
            ..Default::default()
        },
        false,
    );
    assert_eq!(roots(&union), ["broken", "duplicate", "parent"]);
    let disabled = inspect(
        &graph,
        FilterInput {
            problem: true,
            duplicate_primary: Some(false),
            ..Default::default()
        },
        false,
    );
    assert_eq!(roots(&disabled), ["broken"]);
    let problem = inspect(
        &graph,
        FilterInput {
            problem: true,
            ..Default::default()
        },
        false,
    );
    assert_eq!(roots(&problem), roots(&union));
}

#[test]
fn reverse_and_isolated_filters_preserve_node_problems_and_respect_traversal_cuts() {
    let graph = duplicate_graph();
    let reverse = inspect(
        &graph,
        FilterInput {
            duplicate_primary: Some(true),
            ..Default::default()
        },
        true,
    );
    assert_eq!(roots(&reverse), ["duplicate", "healthy"]);
    assert_eq!(
        root(&reverse, "duplicate")
            .iter()
            .filter(|entry| matches!(
                entry.label,
                TreeLabel::Candidate {
                    candidate: CandidateView::Primary { .. },
                    ..
                }
            ))
            .count(),
        2
    );
    // A type cut removes context paths, not the duplicate node itself.
    let cut = build(
        &graph,
        ViewInput {
            reverse: true,
            ignore_isolated_root: true,
            filter: FilterInput {
                duplicate_primary: Some(true),
                kinds: vec![DependencyKind::Optional],
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(roots(&cut), ["duplicate"]);
    let ignored = inspect(
        &graph,
        FilterInput {
            duplicate_primary: Some(true),
            ignores: vec!["fabric:duplicate".parse().unwrap()],
            ..Default::default()
        },
        false,
    );
    assert!(ignored.trees.is_empty());
    let targeted = build(
        &graph,
        ViewInput {
            targets: vec![Identifier {
                loader: "fabric".into(),
                id: "duplicate".into(),
            }],
            ignore_isolated_root: true,
            filter: FilterInput {
                problem: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(roots(&targeted), ["duplicate"]);
}

#[test]
fn snapshot_roundtrip_keeps_duplicate_evidence_and_does_not_change_dependency_matching() {
    let graph = duplicate_graph();
    let mut snapshot = Vec::new();
    storage::write_json(&graph, &mut snapshot).unwrap();
    let restored = storage::read_json(snapshot.as_slice(), &BUILTIN_LOADERS).unwrap();
    let problem = inspect(
        &restored,
        FilterInput {
            problem: true,
            ..Default::default()
        },
        false,
    );
    assert_eq!(roots(&problem), ["broken", "duplicate", "parent"]);
    let required = inspect(
        &restored,
        FilterInput {
            kinds: vec![DependencyKind::Required],
            rules: vec!["required:unmatched".parse().unwrap()],
            ..Default::default()
        },
        false,
    );
    // Duplicate candidates do not turn an otherwise matched required relation into unmatched.
    assert!(
        !root(&required, "parent")
            .iter()
            .any(|entry| matches!(&entry.label,
                TreeLabel::Node { identifier, root: false, .. } if identifier.id == "duplicate"
            ))
    );
    let normal = inspect(&restored, FilterInput::default(), false);
    assert!(matches!(
        &root(&normal, "duplicate")[0].label,
        TreeLabel::Node {
            duplicate_primary: Some(2),
            ..
        }
    ));
}
