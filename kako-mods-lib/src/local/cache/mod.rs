//! Generic immutable content cache, typed metadata access and rebuildable quick lookup.
//!
//! Metadata file access belongs to `CacheDir` and `CacheContent`; adapters use their
//! typed operations and own metadata interpretation through `Codec` implementations.

pub mod codec;
pub mod hash;
pub mod mods;

mod cache;
pub use cache::{CacheDir, ContentHashMismatch};
mod content;
pub use content::{CacheContent, ContentVerification};
mod inventory;
pub use inventory::{
    CacheEntry, CacheFileKind, CacheInspection, CacheMetadataEntry, CacheScanEntry,
};
mod transfer;
pub use transfer::{CacheLinkPreference, ExportOutcome, ImportOutcome, TransferMethod};

mod directory;
mod index;
mod layout;
mod lookup;
pub use lookup::CachedContentLookupError;
mod storage;

mod status;
pub use status::VerificationMode;
