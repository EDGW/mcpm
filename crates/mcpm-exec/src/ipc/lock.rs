//! OS-backed exclusive lock files for holder and shim singletons.

use std::{fs::{File, OpenOptions}, path::{Path, PathBuf}};
use thiserror::Error;

/// An owned lock file; dropping it releases the operating-system lock.
#[derive(Debug)]
pub struct ExclusiveLock {
    file: File,
    path: PathBuf,
}

impl ExclusiveLock {
    /// Acquires an exclusive, non-blocking lock, creating the lock file if needed.
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self, LockError> {
        let path = path.as_ref().to_owned();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LockError::Io { path: path.clone(), source })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| LockError::Io { path: path.clone(), source })?;
        lock_file(&file).map_err(|source| LockError::Acquire { path: path.clone(), source })?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path { &self.path }

    /// Keeps the file alive while allowing callers to inspect it in diagnostics.
    pub fn file(&self) -> &File { &self.file }
}

#[cfg(unix)]
fn lock_file(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

#[cfg(windows)]
fn lock_file(file: &File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, OVERLAPPED};
    let mut overlapped = OVERLAPPED::default();
    let result = unsafe { LockFileEx(file.as_raw_handle() as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, u32::MAX, u32::MAX, &mut overlapped) };
    if result != 0 { Ok(()) } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) { Err(error) } else { Err(error) }
    }
}

#[cfg(not(any(unix, windows)))]
fn lock_file(_: &File) -> std::io::Result<()> { Ok(()) }

#[derive(Debug, Error)]
pub enum LockError {
    #[error("failed to open IPC lock {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("IPC lock {path} is already held or cannot be acquired: {source}")]
    Acquire { path: PathBuf, source: std::io::Error },
}
