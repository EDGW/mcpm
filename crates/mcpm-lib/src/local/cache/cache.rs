//! Public cache root access, verified content handles and generic metadata operations.

use super::{
    CacheContent, ContentVerification, VerificationMode, directory,
    hash::Blake3Hash,
    layout,
    storage::{self, Content},
};
use crate::local::codec::Codec;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// A versioned cache of immutable content and independently encoded metadata.
///
/// Hard links share content protection with their sources and exported names.
/// Replace linked files instead of modifying their bytes in place.
///
/// # Implementations
/// - [`Self::new`]: cache root access, verified content and generic metadata operations.
/// - [`Self::entry`]: inventory, inspection and removal (`inventory.rs`).
/// - [`Self::find_content`]: read-only content lookup (`lookup.rs`).
/// - [`Self::find_file`]: key lookup and index rebuilding (`lookup.rs`).
/// - [`Self::import`]: content import and export (`transfer.rs`).
#[derive(Debug, Clone)]
pub struct CacheDir {
    pub(super) path: PathBuf,
}

impl CacheDir {
    /// Opens an existing cache without creating or migrating it.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            path: directory::open(&path.into())?,
        })
    }

    /// Initializes an absent or empty root, or accepts an already valid cache.
    /// Never adopts nonempty uninitialized directories or migrates old formats.
    pub fn initialize(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            path: directory::initialize(path.as_ref())?,
        })
    }

    pub fn metadata_path(&self) -> PathBuf {
        self.path.join(layout::MANIFEST)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Pins and fully verifies content. Missing content returns `None`.
    pub fn open_content(&self, key: &Blake3Hash) -> Result<Option<CacheContent>> {
        Ok(self.open_verified(key)?.map(|content| {
            CacheContent::new(
                self.path.clone(),
                *key,
                content,
                ContentVerification::Blake3,
            )
        }))
    }

    /// Opens content with an explicit verification policy. Structure mode pins a
    /// regular file but does not establish its full content identity.
    pub fn open_content_with_mode(
        &self,
        key: &Blake3Hash,
        mode: VerificationMode,
    ) -> Result<Option<CacheContent>> {
        match mode {
            VerificationMode::Content => self.open_content(key),
            VerificationMode::Structure => Ok(self.open_raw(key)?.map(|content| {
                CacheContent::new(
                    self.path.clone(),
                    *key,
                    content,
                    ContentVerification::Structure,
                )
            })),
        }
    }

    /// Returns the canonical location, without asserting existence or validity.
    pub fn content_path(&self, key: &Blake3Hash) -> PathBuf {
        self.data_path(key)
    }

    /// Returns an owned metadata location for diagnostics, without reading or decoding it.
    /// `name` is a metadata name; the cache appends the `.json` file suffix.
    /// Access metadata through the cache's typed read and write operations.
    pub fn metadata_file_path(&self, key: &Blake3Hash, name: &str) -> Result<PathBuf> {
        layout::sidecar_path(&self.path, key, name)
    }

    /// Fully verifies content without modifying it; missing content returns `None`.
    /// Hash mismatches, unexpected file types and I/O failures remain errors.
    pub fn check(&self, key: &Blake3Hash) -> Result<Option<()>> {
        Ok(self.open_verified(key)?.map(|_| ()))
    }

    /// Reads metadata after full content verification. For repeated reads or
    /// identity-based lookup, retain a `CacheContent` handle instead.
    /// `metaname` does not include the storage layer's `.json` suffix.
    pub fn try_read_metadata<T: Codec>(
        &self,
        key: &Blake3Hash,
        metaname: &str,
    ) -> Result<Option<T>> {
        layout::sidecar_path(&self.path, key, metaname)?;
        let Some(content) = self.open_content(key)? else {
            return Ok(None);
        };
        content.try_read_metadata(metaname)
    }

    /// Atomically replaces a complete metadata snapshot for verified content.
    /// `metaname` does not include the storage layer's `.json` suffix.
    /// The caller owns the semantic association between metadata and bytes.
    /// Returns `None` when content is missing; concurrent removal may leave an orphan.
    pub fn write_metadata<T: Codec>(
        &self,
        key: &Blake3Hash,
        metaname: &str,
        metadata: &T,
    ) -> Result<Option<()>> {
        let path = layout::sidecar_path(&self.path, key, metaname)?;
        let Some(content) = self.open_verified(key)? else {
            return Ok(None);
        };
        let bytes = metadata
            .encode()
            .with_context(|| format!("failed to encode cached metadata {path:?}"))?;
        content.validate()?;
        storage::write_atomic(&path, &bytes)?;
        content.validate()?;
        Ok(Some(()))
    }

    pub(super) fn data_path(&self, key: &Blake3Hash) -> PathBuf {
        layout::data_path(&self.path, key)
    }

    pub(super) fn open_raw(&self, key: &Blake3Hash) -> Result<Option<Content>> {
        if !layout::shard_exists(&self.path, key)? {
            return Ok(None);
        }
        Content::open(&self.data_path(key))
    }

    pub(super) fn open_verified(&self, key: &Blake3Hash) -> Result<Option<Content>> {
        let Some(mut content) = self.open_raw(key)? else {
            return Ok(None);
        };
        let actual = content.hash()?;
        if actual != *key {
            return Err(ContentHashMismatch {
                path: self.data_path(key),
                expected: *key,
                actual,
            }
            .into());
        }
        Ok(Some(content))
    }
}

/// Content at a canonical key has a different complete BLAKE3 digest.
#[derive(Debug, thiserror::Error)]
#[error("cached content at {path:?} has BLAKE3 {actual}; expected {expected}")]
pub struct ContentHashMismatch {
    pub path: PathBuf,
    pub expected: Blake3Hash,
    pub actual: Blake3Hash,
}
