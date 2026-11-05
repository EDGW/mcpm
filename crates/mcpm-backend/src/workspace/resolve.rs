//! Workspace inspection and explicit-connection precedence using the workspace codec.

use super::{Kind, Remote};
use anyhow::Result;
use mcpm_exec::operation::{self, Connection, Target};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub kind: Kind,
    pub connection: Option<Connection>,
    pub remote_path: Option<String>,
}

pub fn inspect(path: &std::path::Path) -> Result<WorkspaceInfo> {
    match super::kind(path)? {
        Kind::Local => Ok(WorkspaceInfo {
            kind: Kind::Local,
            connection: None,
            remote_path: None,
        }),
        Kind::Remote => {
            let remote = Remote::load(path)?;
            Ok(WorkspaceInfo {
                kind: Kind::Remote,
                connection: Some(remote.connection()),
                remote_path: Some(remote.path),
            })
        }
    }
}

pub async fn resolve(path: PathBuf, ssh: Option<Connection>) -> Result<(Target, PathBuf)> {
    if let Some(connection) = ssh {
        return Ok((Target::Remote(connection), path));
    }
    let workspace = operation::execute::<crate::operations::workspace::Inspect>(
        crate::operations::workspace::InspectInput { path: path.clone() },
    )
    .await?;
    match (workspace.connection, workspace.remote_path) {
        (Some(connection), Some(path)) => Ok((Target::Remote(connection), PathBuf::from(path))),
        _ => Ok((Target::Local, path)),
    }
}
