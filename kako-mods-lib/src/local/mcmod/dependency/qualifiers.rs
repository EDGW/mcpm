use super::DependencyConstraint;

/// Rules attached to an individual reference, including within a condition group.
#[derive(Debug, Clone, Default)]
pub struct DependencyQualifiers {
    /// Check this reference only when its target is present (Quilt optional rule).
    pub optional: bool,
    /// Target conditions under which this reference can be ignored.
    pub unless: Option<Box<DependencyConstraint>>,
    /// Declared physical side; absent means no explicit side restriction.
    pub side: Option<DependencySide>,
    /// Declared ordering of the declaring mod relative to the target.
    pub ordering: Option<DependencyOrdering>,
    /// Human-readable explanation from the metadata.
    pub reason: Option<String>,
}

/// Physical side on which a dependency applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencySide {
    /// Client only.
    Client,
    /// Dedicated server only.
    Server,
    /// Both physical sides.
    Both,
}

/// Ordering of the declaring mod relative to a dependency target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyOrdering {
    /// No relative ordering is requested.
    None,
    /// The declaring mod loads before the target.
    BeforeTarget,
    /// The declaring mod loads after the target.
    AfterTarget,
}
