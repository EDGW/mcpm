//! Dependency declarations; supplying loaders retain responsibility for evaluation.

use serde::{Deserialize, Serialize};

use super::ModInfo;
use crate::local::modloader::ModLoader;

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

impl Dependency {
    /// Tests whether a candidate's identity and version match this declaration.
    ///
    /// `loader` must be the loader that supplied this dependency. The caller
    /// supplies a candidate interpreted in that loader's context. Matching is
    /// delegated without imposing common identity or version comparison rules.
    /// Relation kind, physical side, loading order and reason do not affect this
    /// predicate. In particular, matching a `Breaks` or `Conflicts` declaration
    /// means the candidate falls within the declared incompatibility range.
    ///
    /// This performs no I/O and does not select among candidates or determine
    /// whether a complete mod set can load. Unsupported evaluation or insufficient
    /// information returns [`DependencyMatch::Unknown`] and emits a warning log.
    pub fn matches(&self, candidate: &ModInfo, loader: &dyn ModLoader) -> DependencyMatch {
        let result = loader.matches_dependency(candidate, self);
        if result == DependencyMatch::Unknown {
            tracing::warn!(
                loader = %loader.metadata().id().as_str(),
                candidate_id = %candidate.modid,
                candidate_version = %candidate.version,
                dependency_id = %self.id,
                constraint = ?self.version_requirement,
                "Dependency match could not be determined"
            );
        }
        result
    }
}

/// Whether one candidate matches the target and version conditions of a dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyMatch {
    /// The candidate is known to match the declaration.
    Matched,
    /// The candidate is known not to match the declaration.
    Unmatched,
    /// Matching cannot be determined from available information or supported rules.
    Unknown,
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
