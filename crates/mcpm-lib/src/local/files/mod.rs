//! Filesystem helpers independent of mod identification, cache policy and presentation.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

mod content;
pub mod hash;
pub(crate) use content::Content;
mod transfer;
pub(crate) use transfer::stage;
pub use transfer::{CacheLinkPreference, TransferMethod};

mod identity;
pub use identity::FileIdentity;

mod protection;
pub use protection::protect_file_contents;
mod removal;
pub(crate) use removal::remove_protected_tree;

/// Creates a hard link without overwriting a destination or falling back to copying.
///
/// Source and destination must not be symbolic links. Both names share mutable
/// bytes: callers must replace files instead of modifying linked files in place.
pub fn hard_link(source: &Path, destination: &Path) -> Result<(), HardLinkError> {
    fs::hard_link(source, destination).map_err(|error| HardLinkError {
        source_path: source.to_owned(),
        destination: destination.to_owned(),
        source: error,
    })
}

/// A hard-link failure retaining the native filesystem error.
#[derive(Debug, Error)]
#[error("failed to hard-link {source_path:?} to {destination:?}: {source}")]
pub struct HardLinkError {
    pub source_path: PathBuf,
    pub destination: PathBuf,
    #[source]
    pub source: io::Error,
}

impl HardLinkError {
    /// Only filesystem capability failures permit an explicit copy fallback.
    /// Existing destinations, access failures and missing paths never do.
    pub fn allows_copy_fallback(&self) -> bool {
        if matches!(
            self.source.kind(),
            io::ErrorKind::CrossesDevices | io::ErrorKind::Unsupported
        ) {
            return true;
        }
        // Windows: ERROR_INVALID_FUNCTION, ERROR_NOT_SAME_DEVICE,
        // ERROR_NOT_SUPPORTED. Some filesystems report these without a more
        // specific portable ErrorKind.
        #[cfg(windows)]
        if matches!(self.source.raw_os_error(), Some(1 | 17 | 50)) {
            return true;
        }
        false
    }
}

pub(crate) use protection::DirectoryProtection;
