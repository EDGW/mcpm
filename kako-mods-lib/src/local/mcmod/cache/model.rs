//! Cache identifiers, availability states and versioned on-disk digest records.

use std::{collections::BTreeMap, fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};

use super::super::cached_meta::CachedModMeta;

pub const CACHE_FORMAT_VERSION: u32 = 2;
/// Bump whenever identification rules change cached metadata semantics.
pub const MOD_METADATA_VERSION: u32 = 1;

/// Content identity: equal BLAKE3-256 values denote the same cached content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CacheId(pub(super) [u8; 32]);

impl CacheId {
    pub(super) fn from_hash(hash: blake3::Hash) -> Self {
        Self(*hash.as_bytes())
    }
}

impl fmt::Display for CacheId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        blake3::Hash::from(self.0).to_hex().fmt(f)
    }
}

impl FromStr for CacheId {
    type Err = ParseCacheIdError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ParseCacheIdError);
        }
        blake3::Hash::from_hex(value)
            .map(Self::from_hash)
            .map_err(|_| ParseCacheIdError)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("expected a BLAKE3 digest of 64 lowercase hexadecimal characters")]
pub struct ParseCacheIdError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheStatus {
    /// The cache is available and can be used.
    Available,
    /// The cache is not available and cannot be used.
    Unavailable(CacheUnavailableReason),
    /// Neither the data file nor its digest exists.
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheUnavailableReason {
    /// The data exists, but its .data.json digest is missing.
    MissingDigest,
    /// Missing data, malformed/incompatible digest, or failed integrity checks.
    InvalidData,
    /// An unexpected filesystem or operating-system failure.
    UnexpectedError(String),
}

/// Read-only observation; paths and availability can change after this is returned.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub(super) id: CacheId,
    pub(super) data_path: PathBuf,
    pub(super) digest_path: PathBuf,
    pub(super) status: CacheStatus,
    pub(super) digest: Option<CacheDigest>,
}

impl CacheEntry {
    pub fn id(&self) -> CacheId {
        self.id
    }
    pub fn data_path(&self) -> &std::path::Path {
        &self.data_path
    }
    pub fn digest_path(&self) -> &std::path::Path {
        &self.digest_path
    }
    pub fn status(&self) -> &CacheStatus {
        &self.status
    }
    pub fn digest(&self) -> Option<&CacheDigest> {
        self.digest.as_ref()
    }
}

/// Cached identification results, with version and file verification data.
/// A loader's `None` is successful non-recognition, not a missing digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheDigest {
    pub(super) version: u32,
    pub(super) blake3: String,
    pub(super) size: u64,
    pub(super) sha256: String,
    pub(super) metadata_version: u32,
    /// Absent means unchecked; None means checked but unrecognized.
    pub(super) loaders: BTreeMap<String, Option<CachedModMeta>>,
}

impl CacheDigest {
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn metadata_version(&self) -> u32 {
        self.metadata_version
    }
    /// Returns an identification result without exposing storage-specific field semantics.
    pub fn identification(&self, loader: &str) -> super::CachedIdentification<'_> {
        match self.loaders.get(loader) {
            None => super::CachedIdentification::Missing,
            Some(None) => super::CachedIdentification::Unrecognized,
            Some(Some(meta)) => super::CachedIdentification::Recognized(meta),
        }
    }

    /// Iterates checked loaders as business results, without exposing storage nulls.
    pub fn identifications(
        &self,
    ) -> impl ExactSizeIterator<Item = (&str, super::CachedIdentification<'_>)> {
        self.loaders
            .keys()
            .map(|loader| (loader.as_str(), self.identification(loader)))
    }

    pub(super) fn valid_content_descriptor(&self) -> bool {
        self.version == CACHE_FORMAT_VERSION
            && self.blake3.parse::<CacheId>().is_ok()
            && self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    pub(super) fn valid_metadata(&self) -> bool {
        self.metadata_version == MOD_METADATA_VERSION
            && !self.loaders.is_empty()
            && self.loaders.iter().all(|(loader, meta)| {
                !loader.is_empty()
                    && meta
                        .as_ref()
                        .is_none_or(|meta| meta.validate(loader).is_ok())
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationMode {
    Structure,
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataRefresh {
    MissingOrOutdated,
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    Copy,
    HardLink,
    PreferHardLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMethod {
    Copy,
    HardLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportOutcome {
    pub id: CacheId,
    /// True only when this call published the data name.
    pub published: bool,
    /// None when an existing object was reused; no new link to the source is implied.
    pub method: Option<ImportMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    Copy,
    HardLink,
    PreferHardLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMethod {
    Copy,
    HardLink,
}
