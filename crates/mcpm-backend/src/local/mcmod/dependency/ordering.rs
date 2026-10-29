use serde::{Deserialize, Serialize};

/// Ordering of the declaring mod relative to a dependency target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyOrdering {
    /// No relative ordering is requested.
    None,
    /// The declaring mod loads before the target.
    BeforeTarget,
    /// The declaring mod loads after the target.
    AfterTarget,
}
