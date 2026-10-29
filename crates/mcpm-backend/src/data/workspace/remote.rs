//! Sole codec for a Remote workspace registration stored in `.mcpm/server.json`.
//!
//! The registration carries connection data only; remote Minecraft and loader metadata is
//! never copied into it. The file is always located on the machine presenting the
//! workspace, never on the host it points to.

use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 2;

/// Connection settings and remote directory of one Remote workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteRegistration {
    version: u32,
    ssh: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ssh_mcpm: Option<String>,
    path: String,
}

impl RemoteRegistration {
    /// Builds and validates a registration; connection validation remains with the caller.
    pub fn new(ssh: String, executable: Option<String>, path: String) -> Result<Self> {
        let registration = Self {
            version: VERSION,
            ssh,
            ssh_mcpm: executable,
            path,
        };
        registration.validate()?;
        Ok(registration)
    }

    pub fn ssh(&self) -> &str {
        &self.ssh
    }

    /// The stored executable override, if the registration pinned one.
    pub fn ssh_executable(&self) -> Option<&str> {
        self.ssh_mcpm.as_deref()
    }

    /// Remote server directory as stored.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Reads and validates a registration from a workspace root.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".mcpm/server.json");
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read Remote registration {path:?}"))?;
        let registration: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid Remote registration {path:?}"))?;
        registration.validate()?;
        Ok(registration)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported Remote server.json version {}; expected {VERSION}",
            self.version
        );
        ensure!(
            !self.path.trim().is_empty() && !self.path.contains('\0'),
            "remote server directory must be nonempty and contain no NUL"
        );
        ensure!(
            !self.ssh.trim().is_empty() && !self.ssh.contains('\0'),
            "SSH connection command must be nonempty and contain no NUL"
        );
        Ok(())
    }
}
