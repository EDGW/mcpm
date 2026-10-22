//! Shared global SSH options and non-PTY remote mcpm command construction.

use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::Args;

mod connection;
mod windows;

#[derive(Args, Debug)]
pub(crate) struct SshArgs {
    /// SSH connection command, e.g. "ssh -i key -p 2222 user@host"; supported only by list TUI/Console/Raw.
    #[arg(
        long,
        global = true,
        value_name = "COMMAND",
        help_heading = "Global options"
    )]
    ssh: Option<String>,
    /// Remote mcpm executable path (default: mcpm); requires --ssh and list TUI/Console/Raw.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help_heading = "Global options"
    )]
    ssh_mcpm: Option<String>,
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

    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    pub(crate) fn command(&self) -> Result<Command> {
        self.validate()?;
        let connection = self
            .ssh
            .as_deref()
            .context("missing --ssh connection command")?;
        let executable = self.ssh_mcpm.as_deref().unwrap_or("mcpm");
        // Only the executable and a fixed subcommand cross the shell boundary.
        // Command arguments travel as JSON over the session's stdin.
        let windows = executable.contains('\\')
            || executable.as_bytes().get(1) == Some(&b':')
            || executable.ends_with(".exe");
        tracing::debug!(executable, windows, "Preparing remote mcpm invocation");
        let remote = if windows {
            windows::command(&[executable, "remote"])?
        } else {
            shlex::try_join([executable, "remote"])
                .context("remote executable cannot contain NUL bytes")?
        };
        let mut command = connection::command(connection)?;
        command.arg(remote);
        tracing::debug!("Prepared non-PTY SSH command for persistent mcpm session");
        Ok(command)
    }
}
