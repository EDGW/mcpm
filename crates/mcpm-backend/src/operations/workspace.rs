//! Local-only workspace inspection and Remote registration operations.

use crate::workspace::{self, Remote, WorkspaceInfo, Writer};
use anyhow::Result;
use mcpm_exec::operation::{Connection, Operation};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct Inspect;
impl Operation for Inspect {
    const ID: &'static str = "workspace.inspect";
    const SHALLOW: bool = true;
    type Input = InspectInput;
    type Output = WorkspaceInfo;
    async fn run(input: InspectInput) -> Result<WorkspaceInfo> {
        tokio::task::spawn_blocking(move || workspace::inspect(&input.path)).await?
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct InspectInput {
    pub path: PathBuf,
}

pub struct RegisterRemote;
impl Operation for RegisterRemote {
    const ID: &'static str = "workspace.register_remote";
    const SHALLOW: bool = true;
    type Input = RegisterRemoteInput;
    type Output = ();
    async fn run(input: RegisterRemoteInput) -> Result<()> {
        tokio::task::spawn_blocking(move || {
            let path = std::fs::canonicalize(&input.path)
                .map_err(anyhow::Error::from)
                .map_err(|error| {
                    error.context("Remote workspace PATH must be an existing local directory")
                })?;
            let remote = Remote::new(input.connection, input.remote_path)?;
            Writer::acquire(&path)?.remote(&remote, input.force)
        })
        .await?
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterRemoteInput {
    pub path: PathBuf,
    pub connection: Connection,
    pub remote_path: String,
    pub force: bool,
}
