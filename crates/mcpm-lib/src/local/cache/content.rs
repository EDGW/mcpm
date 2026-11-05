//! Pinned read-only cache content and metadata access without repeated full hashing.

use super::{hash::Blake3Hash, layout, storage::Content};
use crate::local::codec::Codec;
use anyhow::{Context, Result};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

/// Read-only access retaining the filesystem observation that established a match.
/// Names may disappear; `validate` detects ordinary replacement or content changes.
#[derive(Debug)]
pub struct CacheContent {
    root: PathBuf,
    key: Blake3Hash,
    pub(super) content: Content,
    verification: ContentVerification,
}

impl CacheContent {
    pub(super) fn new(
        root: PathBuf,
        key: Blake3Hash,
        content: Content,
        verification: ContentVerification,
    ) -> Self {
        Self {
            root,
            key,
            content,
            verification,
        }
    }
    pub fn key(&self) -> Blake3Hash {
        self.key
    }
    pub fn path(&self) -> &Path {
        self.content.path()
    }
    /// The read-only handle starts at offset zero. Reads and clones share its cursor.
    pub fn file(&self) -> &File {
        self.content.file()
    }
    pub fn size(&self) -> u64 {
        self.content.size()
    }
    pub fn verification(&self) -> ContentVerification {
        self.verification
    }
    pub fn validate(&self) -> Result<()> {
        self.content.validate()
    }

    /// Reads a complete metadata snapshot through its codec, without rehashing
    /// immutable content. Missing metadata returns `None`; invalid metadata is an error.
    /// `metaname` does not include the storage layer's `.json` suffix.
    pub fn try_read_metadata<T: Codec>(&self, metaname: &str) -> Result<Option<T>> {
        let path = layout::sidecar_path(&self.root, &self.key, metaname)?;
        self.validate()?;
        let Some(mut sidecar) = Content::open_snapshot(&path)? else {
            self.validate()?;
            return Ok(None);
        };
        let data = sidecar.read()?;
        let metadata = T::decode(&data)
            .with_context(|| format!("failed to decode cached metadata {path:?}"))?;
        self.validate()?;
        Ok(Some(metadata))
    }
}

/// How a content lookup established correspondence with a canonical object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentVerification {
    /// Only file structure and its pinned observation were checked.
    Structure,
    /// The content was checked against its key using a complete BLAKE3 digest.
    Blake3,
    /// A sampled candidate shares the query's pinned native file identity.
    /// Relies on cached contents remaining immutable, including through hard links.
    FileIdentity,
}
