//! Piped Java ownership, process-tree termination and argument-file lifetime for a shim.

use super::{LaunchPlan, plan::attach_executable_tip};
use crate::local::java::process::{ManagedProcess, ProcessSpec};
use anyhow::{Context, Result};
use std::process::{ChildStderr, ChildStdin, ChildStdout};

/// A process result, not an operation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LaunchExit {
    Code(u32),
    Signal(i32),
}

impl LaunchExit {
    pub fn shell_code(self) -> u32 {
        match self {
            Self::Code(code) => code,
            Self::Signal(signal) => 128 + signal as u32,
        }
    }
}

/// Owns Java and its private resources, independent of any attached frontend.
pub struct JavaProcess {
    inner: ManagedProcess,
}

impl JavaProcess {
    pub fn spawn(plan: LaunchPlan) -> Result<Self> {
        let inner = ManagedProcess::spawn(ProcessSpec {
            executable: &plan.executable,
            directory: &plan.directory,
            environment: &plan.environment,
            arguments: &plan.arguments,
            argument_file: plan.argument_file,
        })
        .map_err(attach_executable_tip)?;
        Ok(Self { inner })
    }

    /// Identity of the owned child, for local endpoint ownership checks, never discovery killing.
    pub fn id(&self) -> u32 {
        self.inner.id()
    }

    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.inner.take_stdin()
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.inner.take_stdout()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.inner.take_stderr()
    }

    pub fn try_wait(&mut self) -> Result<Option<LaunchExit>> {
        let Some(status) = self.inner.try_wait()? else {
            return Ok(None);
        };
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                return Ok(Some(LaunchExit::Signal(signal)));
            }
        }
        Ok(Some(LaunchExit::Code(
            status
                .code()
                .context("Java exited without code or signal")? as u32,
        )))
    }

    pub fn kill(&mut self) -> Result<()> {
        self.inner.kill()
    }
}
