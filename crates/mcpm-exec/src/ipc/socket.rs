//! User-scoped local socket addressing, binding and cleanup.

use std::{fs, io::{self, Read, Write}, path::{Path, PathBuf}};
use thiserror::Error;

/// A deterministic, shortened socket location derived from a connection identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketAddress {
    path: PathBuf,
}

impl SocketAddress {
    /// Creates a platform-specific address from arbitrary identity bytes.
    pub fn for_identity(identity: impl AsRef<[u8]>) -> Self {
        let digest = blake3::hash(identity.as_ref()).to_hex().to_string();
        #[cfg(unix)]
        {
            let root = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("TMPDIR").map(PathBuf::from))
                .unwrap_or_else(std::env::temp_dir);
            return Self { path: root.join("mcpm").join("link").join(&digest[..32]).join("sock") };
        }
        #[cfg(windows)]
        {
            return Self { path: PathBuf::from(format!(r"\\.\pipe\mcpm-link-{digest}")) };
        }
        #[allow(unreachable_code)]
        Self { path: PathBuf::from(digest) }
    }

    pub fn path(&self) -> &Path { &self.path }
    pub fn directory(&self) -> Option<&Path> { self.path.parent() }

    /// Removes an old endpoint while the caller owns the singleton lock.
    pub fn cleanup(&self) -> Result<(), SocketError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(SocketError::Io { path: self.path.clone(), source }),
        }
    }

    #[cfg(unix)]
    pub fn bind(&self) -> Result<std::os::unix::net::UnixListener, SocketError> {
        if let Some(directory) = self.directory() {
            fs::create_dir_all(directory).map_err(|source| SocketError::Io { path: directory.to_owned(), source })?;
            set_private(directory).map_err(|source| SocketError::Io { path: directory.to_owned(), source })?;
        }
        self.cleanup()?;
        let listener = std::os::unix::net::UnixListener::bind(&self.path).map_err(|source| SocketError::Io { path: self.path.clone(), source })?;
        set_private(&self.path).map_err(|source| SocketError::Io { path: self.path.clone(), source })?;
        Ok(listener)
    }

    #[cfg(unix)]
    pub fn connect(&self) -> Result<std::os::unix::net::UnixStream, SocketError> {
        std::os::unix::net::UnixStream::connect(&self.path).map_err(|source| SocketError::Io { path: self.path.clone(), source })
    }
}
#[derive(Debug)]
#[cfg(windows)]
pub struct NamedPipeListener {
    name: Vec<u16>,
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
pub struct NamedPipeStream {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl SocketAddress {
    pub fn bind_named_pipe(&self) -> Result<NamedPipeListener, SocketError> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Security::{Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1}, SECURITY_ATTRIBUTES}, Storage::FileSystem::{CreateNamedPipeW, FILE_FLAG_FIRST_PIPE_INSTANCE}, System::Pipes::{PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT}};
        let name: Vec<u16> = std::ffi::OsStr::new(self.path.to_str().unwrap_or_default()).encode_wide().chain(std::iter::once(0)).collect();
        let descriptor: Vec<u16> = std::ffi::OsStr::new("D:P(A;;GA;;;CU)").encode_wide().chain(std::iter::once(0)).collect();
        let mut security_descriptor = std::ptr::null_mut();
        let valid = unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(descriptor.as_ptr(), SDDL_REVISION_1, &mut security_descriptor, std::ptr::null_mut()) };
        if valid == 0 { return Err(SocketError::Io { path: self.path.clone(), source: io::Error::last_os_error() }); }
        let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: security_descriptor, bInheritHandle: 0 };
        let handle = unsafe { CreateNamedPipeW(name.as_ptr(), PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 16, 65536, 65536, 0, &attributes) };
        unsafe { windows_sys::Win32::System::Memory::LocalFree(security_descriptor as _); }
        if handle == INVALID_HANDLE_VALUE { return Err(SocketError::Io { path: self.path.clone(), source: io::Error::last_os_error() }); }
        Ok(NamedPipeListener { name, handle })
    }

    pub fn connect_named_pipe(&self) -> Result<NamedPipeStream, SocketError> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::{CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, OPEN_EXISTING}, System::Pipes::PIPE_READMODE_BYTE};
        let name: Vec<u16> = std::ffi::OsStr::new(self.path.to_str().unwrap_or_default()).encode_wide().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateFileW(name.as_ptr(), FILE_GENERIC_READ | FILE_GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING, 0, 0) };
        if handle == INVALID_HANDLE_VALUE { return Err(SocketError::Io { path: self.path.clone(), source: io::Error::last_os_error() }); }
        let _ = PIPE_READMODE_BYTE;
        Ok(NamedPipeStream { handle })
    }
}

#[cfg(windows)]
impl NamedPipeListener {
    pub fn accept(&self) -> Result<NamedPipeStream, SocketError> {
        use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
        let ok = unsafe { ConnectNamedPipe(self.handle, std::ptr::null_mut()) };
        if ok == 0 { let error = io::Error::last_os_error(); if error.raw_os_error() != Some(535) { return Err(SocketError::Io { path: PathBuf::from(String::from_utf16_lossy(&self.name)), source: error }); } }
        Ok(NamedPipeStream { handle: self.handle })
    }
}

#[cfg(windows)]
impl Read for NamedPipeStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> { use windows_sys::Win32::Storage::FileSystem::ReadFile; let mut count = 0; let ok = unsafe { ReadFile(self.handle, bytes.as_mut_ptr() as _, bytes.len() as _, &mut count, std::ptr::null_mut()) }; if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(count as usize) } }
}

#[cfg(windows)]
impl Write for NamedPipeStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { use windows_sys::Win32::Storage::FileSystem::WriteFile; let mut count = 0; let ok = unsafe { WriteFile(self.handle, bytes.as_ptr() as _, bytes.len() as _, &mut count, std::ptr::null_mut()) }; if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(count as usize) } }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[cfg(windows)]
impl Drop for NamedPipeStream { fn drop(&mut self) { unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle); } } }
#[cfg(windows)]
impl NamedPipeStream {
    pub fn try_clone(&self) -> io::Result<Self> {
        use std::os::windows::io::RawHandle;
        use windows_sys::Win32::Foundation::{CloseHandle, DuplicateHandle, GetCurrentProcess, DUPLICATE_SAME_ACCESS};
        let mut duplicate = 0;
        let ok = unsafe { DuplicateHandle(GetCurrentProcess(), self.handle, GetCurrentProcess(), &mut duplicate, 0, 0, DUPLICATE_SAME_ACCESS) };
        if ok == 0 { return Err(io::Error::last_os_error()); }
        let _ = CloseHandle as unsafe extern "system" fn(_) -> _;
        Ok(Self { handle: duplicate })
    }
}

/// Cross-platform holder client stream.
#[derive(Debug)]
pub enum LocalStream {
    #[cfg(unix)] Unix(std::os::unix::net::UnixStream),
    #[cfg(windows)] NamedPipe(NamedPipeStream),
}

impl Read for LocalStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> { match self { #[cfg(unix)] Self::Unix(stream) => stream.read(bytes), #[cfg(windows)] Self::NamedPipe(stream) => stream.read(bytes) } }
}

impl Write for LocalStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { match self { #[cfg(unix)] Self::Unix(stream) => stream.write(bytes), #[cfg(windows)] Self::NamedPipe(stream) => stream.write(bytes) } }
    fn flush(&mut self) -> io::Result<()> { match self { #[cfg(unix)] Self::Unix(stream) => stream.flush(), #[cfg(windows)] Self::NamedPipe(stream) => stream.flush() } }
}

impl LocalStream {
    pub fn try_clone(&self) -> io::Result<Self> { match self { #[cfg(unix)] Self::Unix(stream) => stream.try_clone().map(Self::Unix), #[cfg(windows)] Self::NamedPipe(stream) => stream.try_clone().map(Self::NamedPipe) } }
}

#[cfg(unix)]
fn set_private(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[derive(Debug, Error)]
pub enum SocketError {
    #[error("socket operation failed for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}
