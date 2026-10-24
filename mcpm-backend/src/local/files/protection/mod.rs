//! Platform-specific content write protection that preserves unlink and replacement.

use std::{io, path::Path};

/// Protects file contents without making the parent directory immutable.
///
/// Unix removes all write bits. Windows adds a deny ACE for Everyone covering
/// FILE_WRITE_DATA and FILE_APPEND_DATA, preserving other access rights and the
/// delete behavior of the file/parent directory. No read-only attribute is set.
/// Owners/administrators can still change permissions; this prevents ordinary
/// in-place writes rather than providing an adversarial immutability boundary.
pub fn protect_file_contents(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        protect(path)
    }
    #[cfg(windows)]
    {
        windows::deny(path, windows::CONTENT_WRITES)?.persist();
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        protect(path)
    }
}

#[cfg(unix)]
fn protect(path: &Path) -> io::Result<()> {
    use std::{fs, os::unix::fs::PermissionsExt};
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() & !0o222);
    fs::set_permissions(path, permissions)
}

#[cfg(windows)]
mod windows;

#[cfg(not(any(unix, windows)))]
fn protect(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "content write protection is unsupported on this platform",
    ))
}

/// Restores temporary directory protection unless publication commits it.
pub(crate) struct DirectoryProtection {
    #[cfg(unix)]
    path: std::path::PathBuf,
    #[cfg(unix)]
    original: Option<std::fs::Permissions>,
    #[cfg(windows)]
    original: Option<windows::Protection>,
}
impl DirectoryProtection {
    /// Temporarily permits unlinking children, without changing any file permissions.
    pub(crate) fn allow_removal(path: &Path) -> io::Result<Self> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a regular directory",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let original = metadata.permissions();
            let mut writable = original.clone();
            writable.set_mode(original.mode() | 0o200);
            std::fs::set_permissions(path, writable)?;
            Ok(Self {
                path: path.to_owned(),
                original: Some(original),
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                original: Some(windows::allow_removal(path)?),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "directory protection is unsupported",
            ))
        }
    }

    pub(crate) fn protect(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let original = std::fs::metadata(path)?.permissions();
            protect(path)?;
            Ok(Self {
                path: path.to_owned(),
                original: Some(original),
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                original: Some(windows::deny(path, windows::DIRECTORY_WRITES)?),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "directory protection is unsupported",
            ))
        }
    }
    pub(crate) fn persist(mut self) {
        #[cfg(unix)]
        {
            self.original = None;
        }
        #[cfg(windows)]
        {
            self.original.take().unwrap().persist();
        }
    }
}
impl Drop for DirectoryProtection {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(original) = &self.original {
            let _ = std::fs::set_permissions(&self.path, original.clone());
        }
    }
}
