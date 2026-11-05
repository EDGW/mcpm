//! Workspace resolution for command families that still use the legacy remote transport.

use crate::{
    commands::execution::Runtime,
    ssh::{Connection, SshArgs},
};
use anyhow::{Result, ensure};
use mcpm_exec::service::Target as ExecutionTarget;
use std::path::PathBuf;

pub(super) struct Target {
    pub path: PathBuf,
    pub ssh: SshArgs,
}

pub(super) fn resolve(path: PathBuf, ssh: SshArgs) -> Result<Target> {
    let directory = std::env::current_dir()?;
    let (target, path) = Runtime::new()?.block_on(mcpm_backend::workspace::resolve(
        path,
        ssh.explicit_connection()?,
    ))?;
    let ssh = match target {
        ExecutionTarget::Local => SshArgs::default(),
        ExecutionTarget::Remote(connection) => SshArgs::from_connection(
            Connection {
                ssh: connection.ssh,
                ssh_mcpmd: connection.mcpmd,
            },
            directory,
        ),
    };
    Ok(Target { path, ssh })
}

pub(super) fn require_supported_local(path: &std::path::Path, command: &str) -> Result<()> {
    ensure!(
        mcpm_backend::workspace::kind(path)? == mcpm_backend::workspace::Kind::Local,
        "{command} does not support Remote workspaces"
    );
    Ok(())
}
