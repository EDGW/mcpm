//! Connection and registration views shared by workspace operations.

use std::{path::PathBuf, process::Command};

use anyhow::Result;
use mcpm_exec::ssh::Connection;
use serde::{Deserialize, Serialize};

/// Caller-supplied SSH settings, with the executable resolved when omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionSpec {
    pub ssh: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Working directory of the local SSH process, set for saved Remote workspaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<PathBuf>,
}

impl ConnectionSpec {
    /// Builds the executor connection, validating the command without running it.
    pub fn connection(&self, default_executable: &str) -> Result<Connection> {
        let connection = Connection::new(
            self.ssh.clone(),
            self.executable
                .clone()
                .unwrap_or_else(|| default_executable.to_owned()),
        );
        let command: Command = connection.command()?;
        drop(command);
        Ok(match &self.directory {
            Some(directory) => connection.with_directory(directory.clone()),
            None => connection,
        })
    }
}

/// The connection part of a Remote registration, for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteView {
    pub path: String,
    pub ssh: String,
    pub destination: Option<String>,
    pub executable: String,
}

/// Where a workspace command should run and what registration selected it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedWorkspace {
    /// Data-source path: the supplied local path, or the registered remote path.
    pub path: PathBuf,
    /// Present for an explicit `--ssh` or a Remote registration.
    pub connection: Option<ConnectionSpec>,
    pub remote: Option<RemoteView>,
}
