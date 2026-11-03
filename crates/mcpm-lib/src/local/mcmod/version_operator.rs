//! Comparison operators used by dependency version constraints.

use serde::{Deserialize, Serialize};

/// A comparison used by a parsed dependency version requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionOperator {
    /// Equality to the supplied version string.
    Equal,
    /// Inequality to the supplied version string.
    NotEqual,
    /// Strictly greater than the supplied version.
    Greater,
    /// Greater than or equal to the supplied version.
    GreaterOrEqual,
    /// Strictly less than the supplied version.
    Less,
    /// Less than or equal to the supplied version.
    LessOrEqual,
}
