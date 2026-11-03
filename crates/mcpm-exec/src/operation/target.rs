//! Transport-neutral target data shared by operation inputs and the execution service.

use serde::{Deserialize, Serialize};

/// An SSH command and the optional remote daemon executable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub ssh: String,
    pub mcpmd: Option<String>,
}

impl Connection {
    /// Stable endpoint key; operation paths deliberately do not participate.
    pub fn key(&self) -> anyhow::Result<String> {
        let bytes = serde_json::to_vec(self)?;
        Ok(blake3::hash(&bytes).to_hex()[..16].to_owned())
    }

    /// A redacted description suitable for state files and user-visible diagnostics.
    pub fn summary(&self) -> String {
        let mut chars = self.ssh.chars();
        let prefix: String = chars.by_ref().take(3).collect();
        format!("{prefix}***")
    }
}

/// Execution host selected from workspace registration or explicit connection arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Local,
    Remote(Connection),
}
