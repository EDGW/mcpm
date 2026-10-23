//! Frontend-only workspace routing; remote command handlers never call this resolver.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, ensure};

use super::workspace::{self, Kind, Remote};
use crate::ssh::SshArgs;

pub(super) fn resolve(path: PathBuf, ssh: SshArgs) -> Result<(PathBuf, SshArgs)> {
    ssh.validate()?;
    // Explicit SSH paths belong to the remote host; do not inspect local files.
    if ssh.is_requested() || workspace::kind(&path)? == Kind::Local {
        return Ok((path, ssh));
    }
    let remote = Remote::load(&path)?;
    let directory =
        fs::canonicalize(&path).context("failed to resolve Remote workspace directory")?;
    tracing::info!(workspace = %path.display(), "Using saved SSH connection for Remote workspace");
    Ok((
        PathBuf::from(&remote.path),
        SshArgs::from_connection(remote.connection(), directory),
    ))
}

pub(super) fn require_supported_local(path: &std::path::Path, command: &str) -> Result<()> {
    ensure!(
        workspace::kind(path)? == Kind::Local,
        "{command} does not support Remote workspaces"
    );
    Ok(())
}
