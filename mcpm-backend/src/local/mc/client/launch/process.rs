//! Java process lifecycle, stream handling and launch diagnostics.

use std::{
    io,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Output, Stdio},
};

use thiserror::Error;

use super::{LaunchPlan, PreparedLaunch, ProcessOptions, ProcessStream};

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("cannot launch target OS {target:?} on host OS {host:?}")]
    Platform { target: String, host: &'static str },
    #[error("failed to start Java {executable:?} in {working_directory:?}")]
    Io {
        executable: PathBuf,
        working_directory: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Error)]
#[error("failed to {operation} Java process {pid}")]
struct ProcessIoError {
    operation: &'static str,
    pid: u32,
    #[source]
    source: io::Error,
}

fn process_error(operation: &'static str, pid: u32, source: io::Error) -> io::Error {
    io::Error::new(
        source.kind(),
        ProcessIoError {
            operation,
            pid,
            source,
        },
    )
}

/// Owns the child and launch plan. Native files remain on disk after exit.
/// Dropping a running child does not kill it.
#[derive(Debug)]
pub struct LaunchProcess {
    child: Option<Child>,
    prepared: PreparedLaunch,
    pid: u32,
    exit_logged: bool,
}

impl PreparedLaunch {
    #[tracing::instrument(name = "launch.spawn", skip_all, fields(version = ?self.plan.version.name()), err)]
    pub fn spawn(self) -> Result<LaunchProcess, SpawnError> {
        let target = &self.plan.options.rules.platform.os;
        let host = match std::env::consts::OS {
            "macos" => "osx",
            other => other,
        };
        if target != host {
            return Err(SpawnError::Platform {
                target: target.clone(),
                host,
            });
        }
        let mut command = Command::new(&self.plan.executable);
        command
            .args(self.plan.arguments())
            .current_dir(&self.plan.working);
        let options = &self.plan.options.process;
        configure_environment(&mut command, options);
        command
            .stdin(stdio(options.stdin))
            .stdout(stdio(options.stdout))
            .stderr(stdio(options.stderr));
        tracing::info!(java = ?self.plan.executable, main_class = ?self.plan.main_class, working_directory = ?self.plan.working, "Starting Java process");
        tracing::debug!(executable = ?self.plan.executable, arguments = ?self.plan.redacted_arguments(), "Final Java launch command (credentials redacted)");
        tracing::debug!(stdin = ?options.stdin, stdout = ?options.stdout, stderr = ?options.stderr, clear_environment = options.clear_environment, environment_overrides = options.environment.len(), "Configured process streams and environment");
        let child = command.spawn().map_err(|source| SpawnError::Io {
            executable: self.plan.executable.clone(),
            working_directory: self.plan.working.clone(),
            source,
        })?;
        let pid = child.id();
        tracing::info!(pid, "Java process started");
        Ok(LaunchProcess {
            child: Some(child),
            prepared: self,
            pid,
            exit_logged: false,
        })
    }
}

impl LaunchProcess {
    pub fn id(&self) -> u32 {
        self.pid
    }
    pub fn plan(&self) -> &LaunchPlan {
        &self.prepared.plan
    }
    pub fn child(&self) -> &Child {
        self.child
            .as_ref()
            .expect("child is owned until wait_with_output")
    }
    /// Access pipes and advanced process operations.
    pub fn child_mut(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("child is owned until wait_with_output")
    }
    pub fn kill(&mut self) -> io::Result<()> {
        tracing::info!(pid = self.pid, version = ?self.prepared.plan.version.name(), "Requesting Java process termination");
        self.child_mut()
            .kill()
            .map_err(|source| process_error("kill", self.pid, source))
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        tracing::debug!(pid = self.pid, version = ?self.prepared.plan.version.name(), "Waiting for Java process");
        let status = self
            .child_mut()
            .wait()
            .map_err(|source| process_error("wait for", self.pid, source))?;
        self.record_exit(status);
        Ok(status)
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self
            .child_mut()
            .try_wait()
            .map_err(|source| process_error("poll", self.pid, source))?;
        if let Some(status) = status {
            self.record_exit(status);
        }
        Ok(status)
    }
    /// Drains piped stdout/stderr while waiting, as std::process::Child does.
    pub fn wait_with_output(mut self) -> io::Result<Output> {
        let result = self
            .child
            .take()
            .expect("child is owned until wait_with_output")
            .wait_with_output()
            .map_err(|source| process_error("collect output from", self.pid, source));
        if let Ok(output) = &result {
            self.record_exit(output.status);
        }
        result
    }
    fn record_exit(&mut self, status: ExitStatus) {
        if self.exit_logged {
            return;
        }
        self.exit_logged = true;
        if status.success() {
            tracing::info!(pid = self.pid, version = ?self.prepared.plan.version.name(), %status, "Java process exited");
        } else {
            tracing::warn!(pid = self.pid, version = ?self.prepared.plan.version.name(), %status, "Java process exited unsuccessfully");
        }
    }
}

pub(super) fn configure_environment(command: &mut Command, options: &ProcessOptions) {
    if options.clear_environment {
        command.env_clear();
    }
    for (name, value) in &options.environment {
        if let Some(value) = value {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
    }
}

fn stdio(stream: ProcessStream) -> Stdio {
    match stream {
        ProcessStream::Inherit => Stdio::inherit(),
        ProcessStream::Null => Stdio::null(),
        ProcessStream::Piped => Stdio::piped(),
    }
}
