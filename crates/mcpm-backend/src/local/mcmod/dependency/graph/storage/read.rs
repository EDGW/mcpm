//! Reads and validates versioned snapshots, restoring graph data and diagnostics.

use std::{collections::HashMap, io::Read, sync::Arc};

use serde::{Deserialize, de::DeserializeOwned};

use crate::local::{
    mcmod::{ModInfo, dependency::Dependency},
    modloader::{LoaderId, ModLoaderMeta},
};

use super::super::{
    DependencyGraph,
    candidate::{DependencyCandidate, ExternalCandidate, NestedModCandidate, PrimaryModCandidate},
    diagnostic::IdentificationFailure,
    edge::{NestedEdge, PrimaryEdge},
    identifier::DependencyIdentifier,
    node::DependencyNode,
};
use super::{
    FORMAT_VERSION,
    error::{GraphValidationError, ReadGraphError},
};

// region: read_json

/// Reads a version-1 or version-2 JSON snapshot and restores the complete declaration graph.
///
/// Mod candidates resolve their loader metadata and shared runtime instances from
/// `loaders`. Empty nodes and external candidates need no runtime loader instance.
/// Reverse relations are rebuilt; edge and dependency order is not significant.
/// No mod archives are opened and no dependency/version/loading checks are run.
///
/// # Errors
///
/// Returns a typed [`ReadGraphError`] for I/O, syntax, schema, version or structural
/// failures. Unknown fields are rejected rather than silently discarded. The
/// graph is returned only after every node, candidate and edge has been validated.
pub fn read_json(
    mut input: impl Read,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<DependencyGraph, ReadGraphError> {
    let mut bytes = Vec::new();
    input
        .read_to_end(&mut bytes)
        .map_err(|source| ReadGraphError::Read { source })?;
    let header: VersionHeader = decode(&bytes)?;
    check_version(header.format_version)?;
    let snapshot: Snapshot = decode(&bytes)?;
    check_version(snapshot.format_version)?;

    let mut graph = DependencyGraph::new();
    // Register every node first, including placeholders and isolated nodes.
    let mut nodes = Vec::with_capacity(snapshot.nodes.len());
    for node in snapshot.nodes {
        let identifier = node.identifier.into_identifier();
        if graph.nodes.contains_key(&identifier) {
            return Err(GraphValidationError::DuplicateNode { node: identifier }.into());
        }
        graph.nodes.insert(
            identifier.clone(),
            DependencyNode::empty(identifier.clone()),
        );
        nodes.push((identifier, node.candidates));
    }
    for (identifier, candidates) in nodes {
        for candidate in candidates {
            let candidate = restore_candidate(candidate, &identifier, &graph.nodes, loaders)?;
            graph.add_modcandidate(candidate);
        }
    }
    graph.identification_failures = snapshot.identification_failures;
    Ok(graph)
}

// Read the version before interpreting the rest of the version-specific schema.
#[derive(Deserialize, Debug)]
struct VersionHeader {
    format_version: u64,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    format_version: u64,
    identification_failures: Vec<IdentificationFailure>,
    nodes: Vec<Node>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Identifier {
    loader: String,
    id: String,
}

impl Identifier {
    fn into_identifier(self) -> DependencyIdentifier {
        DependencyIdentifier::from_parts(LoaderId::new(self.loader), self.id)
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Node {
    identifier: Identifier,
    candidates: Vec<Candidate>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Candidate {
    External {
        #[serde(default)]
        version: Option<String>,
    },
    PrimaryMod {
        modinfo: ModIdentity,
        outgoing: Vec<Edge>,
    },
    NestedMod {
        modinfo: ModIdentity,
        outgoing: Vec<Edge>,
    },
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ModIdentity {
    name: String,
    modid: String,
    version: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Edge {
    target: Identifier,
    dependency: Dependency,
}

// endregion

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ReadGraphError> {
    serde_json::from_slice(bytes).map_err(|source| {
        if source.is_data() {
            ReadGraphError::InvalidSchema { source }
        } else {
            ReadGraphError::InvalidJson { source }
        }
    })
}

fn check_version(actual: u64) -> Result<(), ReadGraphError> {
    if actual != 1 && actual != FORMAT_VERSION {
        return Err(ReadGraphError::UnsupportedFormatVersion {
            actual,
            supported: FORMAT_VERSION,
        });
    }
    Ok(())
}

fn restore_candidate(
    candidate: Candidate,
    identifier: &DependencyIdentifier,
    nodes: &HashMap<DependencyIdentifier, DependencyNode>,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<DependencyCandidate, GraphValidationError> {
    let (nested, modinfo, outgoing) = match candidate {
        Candidate::External { version } => {
            return Ok(DependencyCandidate::External(ExternalCandidate {
                identifier: identifier.clone(),
                version,
            }));
        }
        Candidate::PrimaryMod { modinfo, outgoing } => (false, modinfo, outgoing),
        Candidate::NestedMod { modinfo, outgoing } => (true, modinfo, outgoing),
    };
    if modinfo.modid != identifier.id() {
        return Err(GraphValidationError::CandidateIdMismatch {
            node: identifier.clone(),
            modid: modinfo.modid,
        });
    }
    let loader =
        loaders
            .get(identifier.loader())
            .ok_or_else(|| GraphValidationError::UnknownLoader {
                node: identifier.clone(),
            })?;
    if loader.id() != identifier.loader() {
        return Err(GraphValidationError::LoaderRegistryMismatch {
            node: identifier.clone(),
            registered: loader.id().as_str().to_owned(),
        });
    }

    let mut dependencies = Vec::with_capacity(outgoing.len());
    let mut edges = Vec::with_capacity(outgoing.len());
    for edge in outgoing {
        let target = edge.target.into_identifier();
        if target.loader() != identifier.loader() {
            return Err(GraphValidationError::CrossLoaderEdge {
                node: identifier.clone(),
                target,
            });
        }
        if edge.dependency.id != target.id() {
            return Err(GraphValidationError::EdgeTargetIdMismatch {
                node: identifier.clone(),
                target,
                dependency_id: edge.dependency.id,
            });
        }
        if !nodes.contains_key(&target) {
            return Err(GraphValidationError::MissingTargetNode {
                node: identifier.clone(),
                target,
            });
        }
        dependencies.push(edge.dependency.clone());
        edges.push((target, Arc::new(edge.dependency)));
    }
    let modinfo = Arc::new(ModInfo {
        name: modinfo.name,
        modid: modinfo.modid,
        version: modinfo.version,
        dependencies,
    });
    if nested {
        Ok(DependencyCandidate::NestedMod(NestedModCandidate {
            loader: loader.clone(),
            modinfo,
            outgoing: edges
                .into_iter()
                .map(|(target, dependency)| NestedEdge { target, dependency })
                .collect(),
        }))
    } else {
        Ok(DependencyCandidate::PrimaryMod(PrimaryModCandidate {
            loader: loader.clone(),
            modinfo,
            outgoing: edges
                .into_iter()
                .map(|(target, dependency)| PrimaryEdge { target, dependency })
                .collect(),
        }))
    }
}
