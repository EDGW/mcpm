//! Dependency declarations; supplying loaders retain responsibility for evaluation.

mod constraint;
mod qualifiers;
mod reference;

pub use constraint::DependencyConstraint;
pub use qualifiers::{DependencyOrdering, DependencyQualifiers, DependencySide};
pub use reference::DependencyReference;

/// One declared relation to a target or a group of targets.
#[derive(Debug, Clone)]
pub struct Dependency {
    /// The relation's category. Its exact behavior belongs to the supplying loader.
    pub kind: DependencyKind,
    /// Target matching conditions, kept separate from the relation's polarity.
    pub constraint: DependencyConstraint,
}

/// Categories of declared relations, without imposing cross-loader evaluation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
