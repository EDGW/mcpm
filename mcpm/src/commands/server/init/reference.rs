//! Validate a remote Local Server, then register its connection in an existing local workspace.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, ensure};

use super::super::{
    metadata::{self, ServerInfo},
    workspace::{Remote, Writer},
};
use crate::ssh::SshArgs;

pub(super) fn create(
    path: PathBuf,
    remote_path: String,
    ssh: SshArgs,
    force: bool,
) -> Result<ServerInfo> {
    ensure!(ssh.is_requested(), "--remote requires global --ssh");
    let directory = fs::canonicalize(&path)
        .context("Remote workspace PATH must be an existing local directory")?;
    let remote = Remote::new(ssh.connection()?, remote_path)?;
    tracing::info!(workspace = %path.display(), "Checking Local Server before registering Remote workspace");
    let connection = SshArgs::from_connection(remote.connection(), directory);
    // The server.info endpoint only opens Local Servers, so this also rejects chaining.
    let info = metadata::load_ssh(connection, PathBuf::from(&remote.path))?;
    Writer::acquire(&path)?.remote(&remote, force)?;
    Ok(info)
}
