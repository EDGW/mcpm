//! Typed errors owned by this module.

use std::io;

use thiserror::Error;

use super::super::identifier::DependencyIdentifier;

/// Failures while reading and decoding a graph snapshot.
/// Underlying I/O, JSON and graph validation causes remain available via source().
#[derive(Debug, Error)]
pub enum ReadGraphError {
    #[error("failed to read dependency graph JSON")]
    Read {
        #[source]
        source: io::Error,
    },
    #[error("invalid dependency graph JSON syntax or encoding")]
    InvalidJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("dependency graph JSON does not match its schema")]
    InvalidSchema {
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported dependency graph format version {actual}; expected {supported}")]
    UnsupportedFormatVersion { actual: u64, supported: u64 },
    #[error("invalid dependency graph structure")]
    InvalidGraph(#[from] GraphValidationError),
}

/// Structural errors in an otherwise well-formed graph snapshot.
/// Dependency cycles, duplicate candidates and repeated declarations are allowed.
#[derive(Debug, Error)]
pub enum GraphValidationError {
    #[error("duplicate dependency node {node:?}")]
    DuplicateNode { node: DependencyIdentifier },
    #[error("no loader instance registered for candidate on node {node:?}")]
    UnknownLoader { node: DependencyIdentifier },
    #[error("loader registry entry for node {node:?} has mismatched ID {registered:?}")]
    LoaderRegistryMismatch {
        node: DependencyIdentifier,
        registered: String,
    },
    #[error("candidate mod ID {modid:?} does not match node {node:?}")]
    CandidateIdMismatch {
        node: DependencyIdentifier,
        modid: String,
    },
    #[error("edge from {node:?} to {target:?} crosses loader namespaces")]
    CrossLoaderEdge {
        node: DependencyIdentifier,
        target: DependencyIdentifier,
    },
    #[error(
        "edge from {node:?} targets {target:?}, but its dependency declares ID {dependency_id:?}"
    )]
    EdgeTargetIdMismatch {
        node: DependencyIdentifier,
        target: DependencyIdentifier,
        dependency_id: String,
    },
    #[error("edge from {node:?} references absent target node {target:?}")]
    MissingTargetNode {
        node: DependencyIdentifier,
        target: DependencyIdentifier,
    },
}
