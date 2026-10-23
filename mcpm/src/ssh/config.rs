//! Reusable SSH connection settings and non-PTY mcpm process construction, without CLI parsing.

use std::process::Command;

use anyhow::{Context, Result, ensure};

use super::{connection, windows};

#[derive(Debug, Clone)]
pub(crate) struct Connection {
    pub ssh: String,
    pub ssh_mcpm: Option<String>,
}

impl Connection {
    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    pub(crate) fn command(&self) -> Result<Command> {
        Ok(self.prepare()?.0)
    }

    pub(crate) fn destination(&self) -> Option<String> {
        connection::destination(&self.ssh)
    }

    pub(super) fn prepare(&self) -> Result<(Command, Option<String>)> {
        let executable = self.ssh_mcpm.as_deref().unwrap_or("mcpm");
        ensure!(
            !executable.trim().is_empty(),
            "remote mcpm executable must not be empty"
        );
        // Only the executable and fixed subcommand cross the shell boundary.
        // Business arguments travel as JSON over the session's stdin.
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
        let (mut command, destination) = connection::command(&self.ssh)?;
        command.arg(remote);
        Ok((command, destination))
    }
}
