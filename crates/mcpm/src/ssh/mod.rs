//! Shared SSH target options; transport construction belongs to mcpmd link.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Args;

mod connection;

#[derive(Args, Debug, Default)]
pub(crate) struct SshArgs {
    /// SSH connection command, e.g. "ssh -i key -p 2222 user@host".
    #[arg(
        long,
        global = true,
        value_name = "COMMAND",
        help = format!("SSH connection command, e.g. \"ssh -i key -p 2222 user@host\"; supported by {} init/info/launch/attach/stash/mods and {} cache/mods. Source queries run on the selected Host.", crate::program::roots().server, crate::program::roots().tools),
        help_heading = "Global options"
    )]
    ssh: Option<String>,
    /// Remote mcpmd executable path; requires --ssh. Windows SSH must use cmd as its default shell.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Remote mcpmd executable path (default: mcpmd); requires --ssh. Windows SSH must use cmd as its default shell.",
        help_heading = "Global options"
    )]
    ssh_mcpmd: Option<String>,
}

impl SshArgs {
    pub(crate) fn is_requested(&self) -> bool {
        self.ssh.is_some() || self.ssh_mcpmd.is_some()
    }

    /// Best-effort display identity without revealing the full connection command.
    pub(crate) fn destination(&self) -> Option<String> {
        self.ssh.as_deref().and_then(connection::destination)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.ssh_mcpmd.is_some() && self.ssh.is_none() {
            bail!("--ssh-mcpmd requires --ssh");
        }
        Ok(())
    }

    pub(crate) fn explicit_connection(&self) -> Result<Option<mcpm_exec::service::Connection>> {
        self.validate()?;
        Ok(self.ssh.as_ref().map(|ssh| mcpm_exec::service::Connection {
            ssh: ssh.clone(),
            mcpmd: self.ssh_mcpmd.clone(),
        }))
    }

    /// Explicit --ssh takes precedence. A missing path never triggers workspace discovery.
    pub(crate) async fn create_executor(
        &self,
        path: Option<PathBuf>,
    ) -> Result<(
        mcpm_exec::service::Executor,
        Option<PathBuf>,
        Option<crate::server::RemoteInfo>,
    )> {
        use mcpm_exec::service::{Executor, Target};
        let explicit = self.explicit_connection()?;
        let from_workspace = explicit.is_none() && path.is_some();
        let (target, path) = match path {
            Some(path) => {
                let (target, path) = mcpm_backend::workspace::resolve(path, explicit).await?;
                (target, Some(path))
            }
            None => (explicit.map(Target::Remote).unwrap_or(Target::Local), None),
        };
        let remote = match &target {
            Target::Local => None,
            Target::Remote(connection) if from_workspace => Some(crate::server::RemoteInfo {
                path: path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                ssh: connection.ssh.clone(),
                destination: connection::destination(&connection.ssh),
                executable: connection.mcpmd.clone().unwrap_or_else(|| "mcpmd".into()),
            }),
            Target::Remote(_) => None,
        };
        let executor = Executor::open_with_idle_timeout(
            target,
            crate::theme::get().config.link_idle_timeout(),
        )
        .await?;
        crate::logging::subscribe_executor(&executor).await?;
        Ok((executor, path, remote))
    }
}
