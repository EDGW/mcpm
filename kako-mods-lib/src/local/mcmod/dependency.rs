use super::VersionConstraint;

/// An identified dependency of a declared mod.
#[derive(Debug, Clone)]
pub struct ModDependency {
    /// The identifier of the dependency's mod or runtime component.
    pub modid: String,
    /// Parsed version conditions with raw string operands.
    /// Evaluating these conditions belongs to the supplying loader.
    pub version_requirement: VersionConstraint,
}
