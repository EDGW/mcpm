//! Dependency declarations; supplying loaders retain responsibility for evaluation.

use serde::{Deserialize, Serialize};

mod ordering;
mod side;

pub mod graph;

pub use ordering::DependencyOrdering;
pub use side::DependencySide;

/// One declared relation to a single mod or runtime component.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    /// The relation's category. Its exact behavior belongs to the supplying loader.
    pub kind: DependencyKind,
    /// Identifier of the target mod or runtime component.
    pub id: String,
    /// Version conditions interpreted by the supplying loader.
    pub version_requirement: super::VersionConstraint,
    /// Physical side on which this relation applies, when declared.
    pub side: Option<DependencySide>,
    /// Declaring mod's loading order relative to the target, when declared.
    pub ordering: Option<DependencyOrdering>,
    /// Human-readable explanation from the metadata.
    pub reason: Option<String>,
}

/// Categories of declared relations, without imposing cross-loader evaluation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// A required dependency.
    Required,
    /// An optional dependency that can still constrain a present target.
    Optional,
    /// A recommended dependency.
    Recommended,
    /// A suggested dependency.
    Suggested,
    /// A declared incompatibility that prevents loading under the loader's rules.
    Breaks,
    /// A declared conflict that the loader may report as a warning.
    Conflicts,
}
