use super::ModDependency;

/// A dependency reference or a group of alternative/conjunctive references.
#[derive(Debug, Clone)]
pub enum DependencyConstraint {
    /// One mod/runtime reference with its parsed version requirement.
    Mod(ModDependency),
    /// Every nested dependency condition must hold.
    All(Vec<Self>),
    /// At least one nested dependency condition must hold.
    AnyOf(Vec<Self>),
}
