use super::VersionOperator;

/// A parsed dependency version condition without evaluating version values.
///
/// Version operands remain strings. Their comparison and compatibility rules
/// belong to the loader supplying the metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    /// No version restriction.
    Any,
    /// A comparison against a raw version operand.
    Compare {
        /// The comparison operation.
        operator: VersionOperator,
        /// The version operand without conversion to a version number.
        version: String,
    },
    /// A wildcard version pattern, such as `1.20.x`.
    Wildcard(String),
    /// A caret compatibility requirement; its operand remains a raw string.
    Caret(String),
    /// A tilde compatibility requirement; its operand remains a raw string.
    Tilde(String),
    /// A native recommended version without explicit Maven range restrictions.
    Recommended(String),
    /// Every nested condition must hold. An empty list imposes no restriction.
    All(Vec<Self>),
    /// At least one nested condition must hold. An empty list matches no version.
    AnyOf(Vec<Self>),
}
