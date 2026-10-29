//! Frontend-only workspace routing; remote command handlers never call this resolver.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, ensure};

use super::workspace::{self, Kind, Remote};
use crate::ssh::SshArgs;

/// Effective data source plus the local registration used to select it, if any.
pub(super) struct Target {
    pub path: PathBuf,
    pub ssh: SshArgs,
    pub remote: Option<Remote>,
}

pub(super) fn resolve(path: PathBuf, ssh: SshArgs) -> Result<Target> {
    ssh.validate()?;
    // Explicit SSH paths belong to the remote host; do not inspect local files.
    if ssh.is_requested() || workspace::kind(&path)? == Kind::Local {
        return Ok(Target {
            path,
            ssh,
            remote: None,
        });
    }
    let remote = Remote::load(&path)?;
    let directory =
        fs::canonicalize(&path).context("failed to resolve Remote workspace directory")?;
    tracing::info!(workspace = %path.display(), "Using saved SSH connection for Remote workspace");
    Ok(Target {
        path: PathBuf::from(&remote.path),
        ssh: SshArgs::from_connection(remote.connection(), directory),
        remote: Some(remote),
    })
}

pub(super) fn require_supported_local(path: &std::path::Path, command: &str) -> Result<()> {
    ensure!(
        workspace::kind(path)? == Kind::Local,
        "{command} does not support Remote workspaces"
    );
    Ok(())
}
