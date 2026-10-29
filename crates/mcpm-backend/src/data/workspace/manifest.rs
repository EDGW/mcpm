//! Sole codec for `.mcpm/mcpm.json`, the workspace kind marker.
//!
//! A missing marker is the legacy Local layout; a malformed marker never falls back to
//! another kind.

use std::{fs, io, path::Path, path::PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Data source selected by a workspace marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Local,
    Remote,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Marker {
    version: u32,
    #[serde(rename = "type")]
    kind: Kind,
}

pub(crate) fn marker_path(root: &Path) -> PathBuf {
    root.join(".mcpm/mcpm.json")
}

/// Reads the workspace kind; a missing marker means the legacy Local layout.
pub fn kind(root: &Path) -> Result<Kind> {
    let path = marker_path(root);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // A dangling link is an existing invalid marker, not a legacy layout.
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Kind::Local),
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to inspect workspace manifest {path:?}"));
                }
                Ok(_) => {
                    return Err(error)
                        .with_context(|| format!("failed to read workspace manifest {path:?}"));
                }
            }
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read workspace manifest {path:?}"));
        }
    };
    let marker: Marker = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid workspace manifest {path:?}"))?;
    ensure!(
        marker.version == 1,
        "unsupported mcpm.json version {}; expected 1 ({path:?})",
        marker.version
    );
    Ok(marker.kind)
}

/// All host operations and the workspace resolver use this gate; it never starts a connection.
pub fn require_local(root: &Path) -> Result<()> {
    ensure!(
        kind(root)? == Kind::Local,
        WorkspaceError::RemoteTargetNotLocal {
            path: root.to_owned()
        }
    );
    Ok(())
}

pub(crate) fn marker(kind: Kind) -> Marker {
    Marker { version: 1, kind }
}

/// The workspace marker selects a Remote registration, so a local operation cannot run.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(
        "server target {path:?} is a Remote workspace; this operation requires a Local Server (chained Remote workspaces are not supported)"
    )]
    RemoteTargetNotLocal { path: PathBuf },
}
