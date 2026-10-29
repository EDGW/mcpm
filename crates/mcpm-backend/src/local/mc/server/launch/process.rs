//! Foreground child ownership, typed stopping and platform terminal process-group adaptation.

use super::LaunchPlan;
use anyhow::{Context, Result};
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

/// Standard streams are inherited verbatim; mcpm never consumes Minecraft console input.
#[derive(Debug, Clone, Copy)]
pub enum ForegroundStreams {
    Inherit,
}

/// Requests originate in the frontend's signal adapter, not backend signal handlers.
#[derive(Debug, Clone, Copy)]
pub enum StopRequest {
    Interrupt,
    Terminate,
    /// Windows console already delivered Ctrl+C to parent and child; do not send it twice.
    ConsoleInterrupt,
}

/// Nonzero exit status is a process result, not an operation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Runs only a backend-generated plan and always reaps the child, including error paths.
/// Explicit stop requests get ten seconds to finish; a second request forces termination.
pub fn execute(
    plan: LaunchPlan,
    streams: ForegroundStreams,
    stop: &Receiver<StopRequest>,
) -> Result<LaunchExit> {
    let mut command = Command::new(&plan.executable);
    command
        .current_dir(&plan.directory)
        .env_clear()
        .envs(&plan.environment);
    let argument_file = if plan.argument_file {
        let mut temporary = tempfile::NamedTempFile::new_in(&plan.directory)
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
    match streams {
        ForegroundStreams::Inherit => {
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }
    }
    platform::configure(&mut command);
    let child = command.spawn().with_context(|| {
        format!(
            "failed to start Java executable {:?} in {:?}",
            plan.executable, plan.directory
        )
    })?;
    let mut owned = OwnedChild {
        child,
        reaped: false,
    };
    let _terminal = platform::foreground(owned.child.id())
        .context("failed to attach foreground Java process")?;
    let mut stopping = None;
    loop {
        if let Some(status) = owned
            .child
            .try_wait()
            .context("failed to wait for foreground Java process")?
        {
            owned.reaped = true;
            if let Some(file) = argument_file {
                file.close()
                    .context("Java exited but temporary argument-file cleanup failed")?;
            }
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = status.signal() {
                    return Ok(LaunchExit::Signal(signal));
                }
            }
            return Ok(LaunchExit::Code(
                status
                    .code()
                    .context("Java exited without a code or signal")? as u32,
            ));
        }
        match stop.recv_timeout(Duration::from_millis(20)) {
            Ok(request) => {
                if stopping.is_some() {
                    platform::force(&mut owned.child)?;
                } else {
                    platform::stop(&mut owned.child, request)?;
                    stopping = Some(Instant::now());
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => std::thread::sleep(Duration::from_millis(20)),
        }
        if stopping.is_some_and(|started| started.elapsed() >= Duration::from_secs(10)) {
            platform::force(&mut owned.child)?;
        }
    }
}

struct OwnedChild {
    child: Child,
    reaped: bool,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = platform::force(&mut self.child);
            let _ = self.child.wait();
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::{
        fs::{File, OpenOptions},
        io,
        os::{fd::AsRawFd, unix::process::CommandExt},
    };

    pub(super) fn configure(command: &mut Command) {
        command.process_group(0);
    }

    /// Transfers the controlling terminal to the Java group. The parent no longer receives
    /// terminal Ctrl+C, so explicit requests can always be forwarded without double delivery.
    pub(super) fn foreground(pid: u32) -> Result<Option<Terminal>> {
        let tty = match OpenOptions::new().read(true).write(true).open("/dev/tty") {
            Ok(tty) => tty,
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENXIO | libc::ENODEV | libc::ENOENT)
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error).context("failed to open controlling terminal"),
        };
        let fd = tty.as_raw_fd();
        let previous = unsafe { libc::tcgetpgrp(fd) };
        if previous < 0 {
            return Err(io::Error::last_os_error())
                .context("failed to inspect terminal foreground group");
        }
        if previous != unsafe { libc::getpgrp() } {
            return Ok(None);
        }
        let terminal = Terminal { tty, previous };
        set_foreground(fd, pid as i32)?;
        // A child which read before the transfer may have received SIGTTIN.
        send(pid, libc::SIGCONT)?;
        Ok(Some(terminal))
    }

    pub(super) struct Terminal {
        tty: File,
        previous: libc::pid_t,
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            let _ = set_foreground(self.tty.as_raw_fd(), self.previous);
        }
    }

    fn set_foreground(fd: i32, group: i32) -> Result<()> {
        unsafe {
            let mut mask = std::mem::zeroed();
            let mut previous = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, libc::SIGTTOU);
            let status = libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut previous);
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status))
                    .context("failed to block SIGTTOU during terminal transfer");
            }
            let result = libc::tcsetpgrp(fd, group);
            let error = if result < 0 {
                Some(io::Error::last_os_error())
            } else {
                None
            };
            libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
            if let Some(error) = error {
                return Err(error).context("failed to transfer terminal foreground group");
            }
        }
        Ok(())
    }

    pub(super) fn stop(child: &mut Child, request: StopRequest) -> Result<()> {
        send(
            child.id(),
            match request {
                StopRequest::Interrupt => libc::SIGINT,
                StopRequest::Terminate => libc::SIGTERM,
                StopRequest::ConsoleInterrupt => return Ok(()),
            },
        )
    }
    pub(super) fn force(child: &mut Child) -> Result<()> {
        send(child.id(), libc::SIGKILL)
    }

    fn send(pid: u32, signal: i32) -> Result<()> {
        if unsafe { libc::kill(-(pid as i32), signal) } < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error).context("failed to signal Java process group");
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    pub(super) fn configure(_command: &mut Command) {}
    pub(super) fn foreground(_pid: u32) -> Result<Option<()>> {
        Ok(None)
    }
    pub(super) fn stop(child: &mut Child, request: StopRequest) -> Result<()> {
        // Parent and Java share the console group and both receive Ctrl+C. The frontend
        // handler keeps mcpm alive; Java runs its own shutdown hooks. Never turn Ctrl+C
        // into Ctrl+Break (which requests a HotSpot thread dump) or deliver it twice.
        match request {
            StopRequest::ConsoleInterrupt => Ok(()),
            StopRequest::Terminate => force(child),
            StopRequest::Interrupt => anyhow::bail!(
                "programmatic Java interrupt is unsupported on Windows; use the shared foreground console Ctrl+C notification"
            ),
        }
    }
    pub(super) fn force(child: &mut Child) -> Result<()> {
        child.kill().context("failed to terminate Java child")
    }
}
