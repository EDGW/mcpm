//! Compose host preparation with shallow Remote stub registration; no filesystem or Git policy.

use crate::ssh::SshArgs;
use anyhow::{Context, Result};
use mcpm_backend::operations::{
    server::init::{
        Initialized,
        prepare_remote::{PrepareRemote, PrepareRemoteInput},
    },
    workspace::{RegisterRemote, RegisterRemoteInput},
};
use std::path::PathBuf;

pub(super) async fn create(
    path: PathBuf,
    remote_path: String,
    ssh: SshArgs,
    force: bool,
    no_git: bool,
    git: bool,
) -> Result<Initialized> {
    let connection = ssh
        .explicit_connection()?
        .context("--remote requires global --ssh")?;
    let (executor, host_path, _) = ssh
        .create_executor(Some(PathBuf::from(&remote_path)))
        .await?;
    let initialized = executor
        .call::<PrepareRemote>(PrepareRemoteInput {
            path: host_path.context("remote server requires a path")?,
            no_git,
            git,
        })
        .await?;
    executor
        .call::<RegisterRemote>(RegisterRemoteInput {
            path,
            connection,
            remote_path,
            force,
        })
        .await?;
    Ok(initialized)
}
