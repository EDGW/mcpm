//! Versioned JSON snapshots; runtime loader instances and reverse indices are omitted.

use std::io::Write;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::local::mcmod::{ModInfo, dependency::Dependency};

use super::super::{
    DependencyGraph, candidate::DependencyCandidate, diagnostic::IdentificationFailure,
    identifier::DependencyIdentifier,
};

#[derive(Serialize)]
struct Snapshot<'a> {
    format_version: u64,
    identification_failures: &'a [IdentificationFailure],
    nodes: Vec<Node<'a>>,
}

#[derive(Serialize)]
struct Identifier<'a> {
    loader: &'a str,
    id: &'a str,
}

impl<'a> From<&'a DependencyIdentifier> for Identifier<'a> {
    fn from(identifier: &'a DependencyIdentifier) -> Self {
        Self {
            loader: identifier.loader().as_str(),
            id: identifier.id(),
        }
    }
}

#[derive(Serialize)]
struct Node<'a> {
    identifier: Identifier<'a>,
    candidates: Vec<Candidate<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Candidate<'a> {
    External,
    PrimaryMod {
        modinfo: ModIdentity<'a>,
        outgoing: Vec<Edge<'a>>,
    },
    NestedMod {
        modinfo: ModIdentity<'a>,
        outgoing: Vec<Edge<'a>>,
    },
}

/// Dependencies are stored on outgoing edges, once per declaration.
#[derive(Serialize)]
struct ModIdentity<'a> {
    name: &'a str,
    modid: &'a str,
    version: &'a str,
}

impl<'a> From<&'a ModInfo> for ModIdentity<'a> {
    fn from(info: &'a ModInfo) -> Self {
        Self {
            name: &info.name,
            modid: &info.modid,
            version: &info.version,
        }
    }
}

#[derive(Serialize)]
struct Edge<'a> {
    target: Identifier<'a>,
    dependency: &'a Dependency,
}

impl<'a> Edge<'a> {
    fn new(target: &'a DependencyIdentifier, dependency: &'a Dependency) -> Self {
        Self {
            target: target.into(),
            dependency,
        }
    }
}

/// Writes a deterministic, formatted JSON snapshot followed by a newline.
/// Complete declarations are stored directly on edges without positional indices.
/// Edge and declaration collection order has no semantic meaning.
/// Candidate selection, environment checks and version evaluation are not performed.
pub fn write_json(graph: &DependencyGraph, mut output: impl Write) -> Result<()> {
    let mut nodes: Vec<_> = graph.nodes.iter().collect();
    nodes.sort_by(|(left, _), (right, _)| {
        left.loader()
            .cmp(right.loader())
            .then_with(|| left.id().cmp(right.id()))
    });
    let nodes = nodes
        .into_iter()
        .map(|(identifier, node)| Node {
            identifier: identifier.into(),
            candidates: node
                .candidates()
                .iter()
                .map(|candidate| match candidate.as_ref() {
                    DependencyCandidate::External(_) => Candidate::External,
                    DependencyCandidate::PrimaryMod(candidate) => Candidate::PrimaryMod {
                        modinfo: candidate.modinfo.as_ref().into(),
                        outgoing: candidate
                            .outgoing()
                            .iter()
                            .map(|edge| Edge::new(&edge.target, &edge.dependency))
                            .collect(),
                    },
                    DependencyCandidate::NestedMod(candidate) => Candidate::NestedMod {
                        modinfo: candidate.modinfo.as_ref().into(),
                        outgoing: candidate
                            .outgoing()
                            .iter()
                            .map(|edge| Edge::new(&edge.target, &edge.dependency))
                            .collect(),
                    },
                })
                .collect(),
        })
        .collect();
    serde_json::to_writer_pretty(
        &mut output,
        &Snapshot {
            format_version: super::FORMAT_VERSION,
            identification_failures: graph.identification_failures(),
            nodes,
        },
    )
    .context("failed to serialize dependency graph")?;
    writeln!(output).context("failed to finish dependency graph JSON")?;
    Ok(())
}
