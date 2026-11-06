//! Piped Java ownership, process-tree termination and argument-file lifetime for a shim.

use super::LaunchPlan;
use anyhow::{Context, Result};
use std::{
    io::Write,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
};

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
    child: Child,
    reaped: bool,
    _argument_file: Option<tempfile::NamedTempFile>,
    #[cfg(windows)]
    job: platform::Job,
}
impl JavaProcess {
    pub fn spawn(plan: LaunchPlan) -> Result<Self> {
        let mut command = Command::new(&plan.executable);
        command
            .current_dir(&plan.directory)
            .env_clear()
            .envs(&plan.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let argument_file = if plan.argument_file {
            // Keep cleanup independent of later renames of the server directory.
            let mut temporary = tempfile::NamedTempFile::new()
                .context("failed to prepare temporary Java argument file")?;
            for argument in &plan.arguments {
                write!(temporary, "\"")?;
                for ch in argument.chars() {
                    match ch {
                        '\\' => write!(temporary, "\\\\")?,
                        '"' => write!(temporary, "\\\"")?,
                        '\n' => write!(temporary, "\\n")?,
                        '\r' => write!(temporary, "\\r")?,
                        '\t' => write!(temporary, "\\t")?,
                        '\u{c}' => write!(temporary, "\\f")?,
                        other => write!(temporary, "{other}")?,
                    }
                }
                writeln!(temporary, "\"")?;
            }
            temporary.flush()?;
            let mut argument = std::ffi::OsString::from("@");
            argument.push(temporary.path());
            command.arg(argument);
            Some(temporary)
        } else {
            command.args(&plan.arguments);
            None
        };
        platform::configure(&mut command);
        let child = command.spawn().with_context(|| {
            format!(
                "failed to start Java {:?} in {:?}",
                plan.executable, plan.directory
            )
        })?;
        #[cfg(windows)]
        let mut child = child;
        #[cfg(windows)]
        let job = match platform::Job::assign(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            child,
            reaped: false,
            _argument_file: argument_file,
            #[cfg(windows)]
            job,
        })
    }
    /// Identity of the owned child, for local endpoint ownership checks, never discovery killing.
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }
    pub fn try_wait(&mut self) -> Result<Option<LaunchExit>> {
        let Some(status) = self.child.try_wait().context("failed to wait for Java")? else {
            return Ok(None);
        };
        self.reaped = true;
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
        if self.reaped {
            return Ok(());
        }
        #[cfg(unix)]
        {
            platform::force(&mut self.child)
        }
        #[cfg(windows)]
        {
            self.job.kill()
        }
    }
}
impl Drop for JavaProcess {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::unix::process::CommandExt;
    pub(super) fn configure(command: &mut Command) {
        command.process_group(0);
    }
    pub(super) fn force(child: &mut Child) -> Result<()> {
        if unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) } < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error).context("failed to kill Java process group");
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        mem::{size_of, zeroed},
        os::windows::{io::AsRawHandle, process::CommandExt},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW},
        },
    };
    pub(super) fn configure(command: &mut Command) {
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    pub(super) struct Job(HANDLE);
    unsafe impl Send for Job {}
    impl Job {
        pub(super) fn assign(child: &Child) -> Result<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            anyhow::ensure!(
                !handle.is_null(),
                "failed to create Java job: {}",
                std::io::Error::last_os_error()
            );
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error())
                    .context("failed to configure Java job");
            }
            if unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("failed to assign Java to job");
            }
            Ok(job)
        }
        pub(super) fn kill(&self) -> Result<()> {
            if unsafe { TerminateJobObject(self.0, 1) } == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("failed to terminate Java job");
            }
            Ok(())
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
