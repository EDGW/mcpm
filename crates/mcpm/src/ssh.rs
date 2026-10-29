//! Global SSH options and the connection they select.
//!
//! Only the command-line shape lives here: the connection identity, its argv and the remote
//! executable belong to `mcpm-exec`. Remote settings parsed from a workspace registration are
//! converted into the same types.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;

pub(crate) use mcpm_exec::ssh::Connection;

use crate::{progress::Progress, remote::client::Session};

#[derive(Args, Debug, Default)]
pub(crate) struct SshArgs {
    /// SSH connection command, e.g. "ssh -i key -p 2222 user@host".
    #[arg(
        long,
        global = true,
        value_name = "COMMAND",
        help = format!("SSH connection command, e.g. \"ssh -i key -p 2222 user@host\"; supported by {} init/info/mods list/info/cache/deps/source and {} mods list/info/deps/source identify.", crate::program::roots().server, crate::program::roots().tools),
        help_heading = "Global options"
    )]
    ssh: Option<String>,
    /// Remote tools executable path; requires --ssh. Windows SSH must use cmd as its default shell.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = format!("Remote tools executable path (default: {}); requires --ssh. Windows SSH must use cmd as its default shell.", crate::program::Program::McpmTools.name()),
        help_heading = "Global options"
    )]
    ssh_mcpm: Option<String>,
    /// Working directory the SSH command runs in, when the settings came from a workspace.
    #[arg(skip)]
    directory: Option<PathBuf>,
}

impl SshArgs {
    pub(crate) fn is_requested(&self) -> bool {
        self.ssh.is_some() || self.ssh_mcpm.is_some()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.ssh_mcpm.is_some() && self.ssh.is_none() {
            bail!("--ssh-mcpm requires --ssh");
        }
        Ok(())
    }

    /// Connection settings as stored, including whether the executable was overridden.
    pub(crate) fn settings(&self) -> Result<(String, Option<String>)> {
        self.validate()?;
        Ok((
            self.ssh
                .clone()
                .context("missing --ssh connection command")?,
            self.ssh_mcpm.clone(),
        ))
    }

    /// Connection identity with the executable resolved to its default.
    pub(crate) fn connection(&self) -> Result<Connection> {
        let (ssh, executable) = self.settings()?;
        let executable =
            executable.unwrap_or_else(|| crate::program::Program::McpmTools.name().to_owned());
        Ok(Connection::new(ssh, executable))
    }

    /// Settings of a workspace registration; `directory` becomes the connection's working directory.
    pub(crate) fn from_settings(
        ssh: String,
        ssh_mcpm: Option<String>,
        directory: Option<PathBuf>,
    ) -> Self {
        Self {
            ssh: Some(ssh),
            ssh_mcpm,
            directory,
        }
    }

    /// Display connection progress until the remote handshake is complete.
    pub(crate) fn connect(&self) -> Result<Session> {
        let connection = self.connection()?;
        let mut command = connection.command()?;
        if let Some(directory) = &self.directory {
            command.current_dir(directory);
        }
        let message = match connection.destination() {
            Some(destination) => format!("Connecting {}...", destination.escape_debug()),
            None => "Connecting...".into(),
        };
        let _progress = Progress::new(&message, "Connecting");
        Session::start(&mut command).context("failed to open SSH mcpm session")
    }
}
