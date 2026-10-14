use super::DependencyReference;

/// A dependency reference or a group of alternative/conjunctive references.
#[derive(Debug, Clone)]
pub enum DependencyConstraint {
    /// One mod/runtime reference with its parsed version requirement.
    Reference(DependencyReference),
    /// Every nested target condition must match; empty means true.
    All(Vec<Self>),
    /// At least one nested target condition must match; empty means false.
    AnyOf(Vec<Self>),
}
