//! Sole JSON codec and validation rules for versioned Loader metadata snapshots.

use crate::local::{cache::hash::Blake3Hash, codec::Codec, mcmod::CachedModMeta};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CACHE_FORMAT_VERSION: u32 = 2;
pub const MOD_METADATA_VERSION: u32 = 1;
pub const METANAME: &str = "meta";

/// Cached identification results, with version and file verification data.
/// A loader's `None` is successful non-recognition, not a missing digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheDigest {
    pub(super) version: u32,
    pub(super) blake3: Blake3Hash,
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

impl Codec for CacheDigest {
    fn decode(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("failed to decode Loader metadata snapshot")
    }
    fn encode(&self) -> Result<Vec<u8>> {
        ensure!(
            self.valid_content_descriptor(),
            "invalid Loader content descriptor"
        );
        serde_json::to_vec(self).context("failed to encode Loader metadata snapshot")
    }
}
