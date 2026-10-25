//! Shared global SSH options and non-PTY remote mcpm command construction.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::{progress::Progress, remote::client::Session};

mod config;
pub(crate) use config::Connection;

mod connection;
mod windows;

#[derive(Args, Debug, Default)]
pub(crate) struct SshArgs {
    /// SSH connection command, e.g. "ssh -i key -p 2222 user@host".
    #[arg(
        long,
        global = true,
        value_name = "COMMAND",
        help = format!("SSH connection command, e.g. \"ssh -i key -p 2222 user@host\"; supported by {} init/info/mods list/info/cache/deps/source and {} mod list/info/deps/source identify.", crate::program::roots().server, crate::program::roots().tools),
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

    pub(crate) fn connection(&self) -> Result<Connection> {
        self.validate()?;
        Ok(Connection {
            ssh: self
                .ssh
                .clone()
                .context("missing --ssh connection command")?,
            ssh_mcpm: self.ssh_mcpm.clone(),
        })
    }

    pub(crate) fn from_connection(connection: Connection, directory: PathBuf) -> Self {
        Self {
            ssh: Some(connection.ssh),
            ssh_mcpm: connection.ssh_mcpm,
            directory: Some(directory),
        }
    }

    /// Display connection progress until the remote handshake and log subscription are ready.
    pub(crate) fn connect(&self) -> Result<Session> {
        let (mut command, destination) = self.connection()?.prepare()?;
        if let Some(directory) = &self.directory {
            command.current_dir(directory);
        }
        let message = match destination {
            Some(destination) => format!("Connecting {}...", destination.escape_debug()),
            None => "Connecting...".into(),
        };
        let _progress = Progress::new(&message, "Connecting");
        Session::start(&mut command).context("failed to open SSH mcpm session")
    }
}
