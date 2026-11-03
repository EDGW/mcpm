//! Platform handle redirection and graceful shutdown notifications for link processes.

use anyhow::{Context, Result};
use std::{fs, path::Path};

pub(super) fn duplicate_stdout() -> Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        Ok(std::io::stdout().as_fd().try_clone_to_owned()?.into())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        Ok(std::io::stdout().as_handle().try_clone_to_owned()?.into())
    }
}

pub(super) fn redirect(path: &Path) -> Result<()> {
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::dup2(log.as_raw_fd(), libc::STDOUT_FILENO) } < 0 {
            return Err(std::io::Error::last_os_error()).context("failed to redirect link stdout");
        }
        if unsafe { libc::dup2(log.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
            return Err(std::io::Error::last_os_error()).context("failed to redirect link stderr");
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, IntoRawHandle};
        use windows_sys::Win32::System::Console::{
            STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
        };
        let stdout = log.try_clone()?;
        if unsafe { SetStdHandle(STD_OUTPUT_HANDLE, stdout.as_raw_handle()) } == 0 {
            return Err(std::io::Error::last_os_error()).context("failed to redirect link stdout");
        }
        if unsafe { SetStdHandle(STD_ERROR_HANDLE, log.as_raw_handle()) } == 0 {
            return Err(std::io::Error::last_os_error()).context("failed to redirect link stderr");
        }
        let _ = stdout.into_raw_handle();
        let _ = log.into_raw_handle();
    }
    Ok(())
}

pub(super) async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler is available");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(windows)]
    {
        let mut close =
            tokio::signal::windows::ctrl_close().expect("CTRL_CLOSE handler is available");
        let mut interrupt =
            tokio::signal::windows::ctrl_break().expect("CTRL_BREAK handler is available");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = close.recv() => {}, _ = interrupt.recv() => {} }
    }
}
