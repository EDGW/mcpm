//! Native file identities for comparing hard links without reading file contents.

use std::{fs::File, io, path::Path};

/// Stable only while the filesystem object exists; identifiers can be reused after deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    volume: u64,
    file: u128,
}

impl FileIdentity {
    #[cfg(unix)]
    pub fn read(path: &Path) -> io::Result<Self> {
        Ok(Self::from_metadata(&std::fs::metadata(path)?))
    }

    #[cfg(windows)]
    pub fn read(path: &Path) -> io::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
        };
        let file = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        Self::from_file(&file)
    }

    #[cfg(not(any(unix, windows)))]
    pub fn read(_path: &Path) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native file identities are unsupported on this platform",
        ))
    }

    #[cfg(unix)]
    pub fn from_file(file: &File) -> io::Result<Self> {
        Ok(Self::from_metadata(&file.metadata()?))
    }

    #[cfg(unix)]
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            volume: metadata.dev(),
            file: metadata.ino().into(),
        }
    }

    #[cfg(windows)]
    pub fn from_file(file: &File) -> io::Result<Self> {
        use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
        };
        let mut info = MaybeUninit::<FILE_ID_INFO>::uninit();
        // The handle remains owned by `file`, and the output has exactly FILE_ID_INFO's size.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                info.as_mut_ptr().cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let info = unsafe { info.assume_init() };
        Ok(Self {
            volume: info.VolumeSerialNumber,
            file: u128::from_le_bytes(info.FileId.Identifier),
        })
    }

    #[cfg(not(any(unix, windows)))]
    pub fn from_file(_file: &File) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native file identities are unsupported on this platform",
        ))
    }
}
