//! SSH connection identity and remote process construction, without CLI parsing.
//!
//! The connection is settings plus argv construction: the caller keeps its own argument
//! type and hands over the connection command and the remote executable path.

use std::process::Command;

use thiserror::Error;

mod connection;
mod windows;

/// Remote entry point of the serve loop on the connected host.
const SUBCOMMAND: &str = "remote";

/// How to reach a host and which executable serves requests there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    ssh: String,
    executable: String,
}

impl Connection {
    pub fn new(ssh: impl Into<String>, executable: impl Into<String>) -> Self {
        Self {
            ssh: ssh.into(),
            executable: executable.into(),
        }
    }

    /// The connection command as the user wrote it.
    pub fn ssh(&self) -> &str {
        &self.ssh
    }

    /// The remote executable path, relative or absolute.
    pub fn executable(&self) -> &str {
        &self.executable
    }

    /// Best-effort destination for progress messages; never validates the command.
    pub fn destination(&self) -> Option<String> {
        connection::destination(&self.ssh)
    }

    /// Builds the non-PTY process that runs the remote serve entry point.
    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    pub fn command(&self) -> Result<Command, ConnectionError> {
        if self.executable.trim().is_empty() {
            return Err(ConnectionError::EmptyExecutable);
        }
        // Only the executable and the fixed entry point cross the shell boundary;
        // business input travels as JSON over the session's stdin.
        let windows = self.executable.contains('\\')
            || self.executable.as_bytes().get(1) == Some(&b':')
            || self.executable.ends_with(".exe");
        tracing::debug!(
            executable = self.executable,
            windows,
            "Preparing remote invocation"
        );
        let remote = if windows {
            windows::command(&self.executable)?
        } else {
            shlex::try_join([self.executable.as_str(), SUBCOMMAND]).map_err(|_| {
                ConnectionError::Executable {
                    message: "the remote executable cannot contain NUL bytes".to_owned(),
                }
            })?
        };
        let (mut command, _) = connection::command(&self.ssh)?;
        command.arg(remote);
        Ok(command)
    }
}

/// The connection settings cannot be turned into a process.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ConnectionError {
    /// The connection command is empty or cannot be split into words.
    #[error("invalid --ssh connection command: {message}")]
    Command { message: String },
    /// No remote executable was configured.
    #[error("the remote executable path must not be empty")]
    EmptyExecutable,
    /// The remote executable cannot be expressed in the remote shell invocation.
    #[error("unsupported remote executable: {message}")]
    Executable { message: String },
}
