//! Global SSH options and the mcpmd connection they select.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use mcpm_exec::ssh::Connection;
use crate::{progress::Progress, remote::client::Session};
#[derive(Args, Debug, Default, Clone)]
pub(crate) struct SshArgs {
    /// SSH connection command, e.g. "ssh -i key -p 2222 user@host".
    #[arg(long, global = true, value_name = "COMMAND", help = "SSH connection command for the remote mcpmd holder.", help_heading = "Global options")]
    ssh: Option<String>,
    /// Remote mcpmd executable path; requires --ssh.
    #[arg(long, global = true, value_name = "PATH", help = "Remote mcpmd executable path (default: mcpmd); requires --ssh.", help_heading = "Global options")]
    ssh_mcpmd: Option<String>,
    /// Working directory the SSH command runs in, when the settings came from a workspace.
    #[arg(skip)]
    directory: Option<PathBuf>,
}

impl SshArgs {
    pub(crate) fn is_requested(&self) -> bool { self.ssh.is_some() || self.ssh_mcpmd.is_some() }

    pub(crate) fn directory(&self) -> Option<&PathBuf> { self.directory.as_ref() }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.ssh_mcpmd.is_some() && self.ssh.is_none() { bail!("--ssh-mcpmd requires --ssh"); }
        Ok(())
    }

    pub(crate) fn settings(&self) -> Result<(String, Option<String>)> {
        self.validate()?;
        Ok((self.ssh.clone().context("missing --ssh connection command")?, self.ssh_mcpmd.clone()))
    }

    pub(crate) fn connection(&self) -> Result<Connection> {
        let (ssh, executable) = self.settings()?;
        let connection = Connection::new(ssh, executable.unwrap_or_else(|| "mcpmd".to_owned()));
        Ok(match &self.directory { Some(directory) => connection.with_directory(directory.clone()), None => connection })
    }

    pub(crate) fn from_settings(ssh: String, ssh_mcpmd: Option<String>, directory: Option<PathBuf>) -> Self {
        Self { ssh: Some(ssh), ssh_mcpmd, directory }
    }

    pub(crate) fn connect(&self) -> Result<Session> {
        let connection = self.connection()?;
        let mut command = connection.command()?;
        if let Some(directory) = &self.directory { command.current_dir(directory); }
        let message = match connection.destination() { Some(destination) => format!("Connecting {}...", destination.escape_debug()), None => "Connecting...".into() };
        let _progress = Progress::new(&message, "Connecting");
        Session::start(&mut command).context("failed to open legacy cache session")
    }
}
