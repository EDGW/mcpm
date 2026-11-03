//! Dependency client and server side declarations.

use serde::{Deserialize, Serialize};

/// Physical side on which a dependency applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencySide {
    /// Client only.
    Client,
    /// Dedicated server only.
    Server,
    /// Both physical sides.
    Both,
}
