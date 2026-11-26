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
    full_command: bool,
) -> Result<Initialized> {
    let connection = ssh
        .explicit_connection()?
        .context("--remote requires global --ssh")?;
    let (executor, host_path, _) = ssh
        .create_executor(Some(PathBuf::from(&remote_path)))
        .await?;
    let host_path = host_path.context("remote server requires a path")?;
    let mut initialized = executor
        .call::<PrepareRemote>(PrepareRemoteInput {
            path: host_path.clone(),
            no_git,
            git,
        })
        .await?;
    if full_command {
        initialized.server = executor
            .call::<mcpm_backend::operations::server::info::Info>(
                mcpm_backend::operations::server::info::InfoInput {
                    path: host_path,
                    full_command: true,
                    allow_legacy: false,
                },
            )
            .await?;
    }
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
