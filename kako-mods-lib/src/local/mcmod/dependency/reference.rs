use super::DependencyQualifiers;
use crate::local::mcmod::VersionConstraint;

/// An identified dependency of a declared mod.
#[derive(Debug, Clone)]
pub struct DependencyReference {
    /// The identifier of the dependency's mod or runtime component.
    pub id: String,
    /// Parsed version conditions with raw string operands.
    /// Evaluating these conditions belongs to the supplying loader.
    pub version_requirement: VersionConstraint,
    /// Rules applying specifically to this reference.
    pub qualifiers: DependencyQualifiers,
}
